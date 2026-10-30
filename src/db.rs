use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::model::{Playlist, Track};

pub struct Db {
    conn: Connection,
}

fn track_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        album: row.get(3)?,
        duration: row.get(4)?,
        art: row.get(5)?,
        source: row.get(6)?,
    })
}

impl Db {
    pub fn open() -> Result<Self> {
        let dir = crate::paths::data_dir();
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(dir.join("marmota-music.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS tracks(
                id TEXT PRIMARY KEY, title TEXT NOT NULL, artist TEXT, album TEXT,
                duration REAL, art TEXT, source TEXT, added INTEGER DEFAULT 0);
             CREATE TABLE IF NOT EXISTS playlists(
                id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT UNIQUE NOT NULL,
                created INTEGER DEFAULT 0);
             CREATE TABLE IF NOT EXISTS playlist_items(
                playlist_id INTEGER NOT NULL, track_id TEXT NOT NULL, pos INTEGER DEFAULT 0,
                PRIMARY KEY(playlist_id, track_id));
             CREATE TABLE IF NOT EXISTS lyrics(
                track_id TEXT PRIMARY KEY, synced TEXT, plain TEXT);",
        )?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            // v1: "library" was really a play history. Split it into saved
            // songs and recently played, add remote playlists, and retry
            // lyrics that only had plain text with the better fetcher.
            for sql in [
                "ALTER TABLE tracks ADD COLUMN saved INTEGER DEFAULT 0",
                "ALTER TABLE tracks ADD COLUMN played INTEGER DEFAULT 0",
                "ALTER TABLE playlists ADD COLUMN remote TEXT DEFAULT ''",
                "ALTER TABLE playlists ADD COLUMN art TEXT DEFAULT ''",
            ] {
                let _ = conn.execute(sql, []);
            }
            conn.execute_batch(
                "UPDATE tracks SET played = added WHERE played = 0;
                 DELETE FROM lyrics WHERE synced IS NULL OR synced = '';
                 PRAGMA user_version = 1;",
            )?;
        }
        Ok(Self { conn })
    }

    pub fn save_track(&self, t: &Track) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tracks(id,title,artist,album,duration,art,source,added)
             VALUES(?1,?2,?3,?4,?5,?6,?7,strftime('%s','now'))
             ON CONFLICT(id) DO UPDATE SET title=?2, artist=?3, album=?4,
               duration=?5, art=?6, source=?7",
            params![t.id, t.title, t.artist, t.album, t.duration, t.art, t.source],
        )?;
        Ok(())
    }

    fn tracks(&self, sql: &str) -> Vec<Track> {
        let mut out = Vec::new();
        if let Ok(mut st) = self.conn.prepare(sql) {
            if let Ok(rows) = st.query_map([], track_from_row) {
                for r in rows.flatten() {
                    out.push(r);
                }
            }
        }
        out
    }

    /// Songs the user saved with "Guardar".
    pub fn library(&self) -> Vec<Track> {
        self.tracks(
            "SELECT id,title,artist,album,duration,art,source FROM tracks
             WHERE saved > 0 ORDER BY saved DESC",
        )
    }

    pub fn history(&self, limit: usize) -> Vec<Track> {
        self.tracks(&format!(
            "SELECT id,title,artist,album,duration,art,source FROM tracks
             WHERE played > 0 ORDER BY played DESC LIMIT {limit}"
        ))
    }

    pub fn set_saved(&self, t: &Track, saved: bool) -> Result<()> {
        self.save_track(t)?;
        self.conn.execute(
            "UPDATE tracks SET saved = CASE WHEN ?2 THEN strftime('%s','now') ELSE 0 END WHERE id=?1",
            params![t.id, saved],
        )?;
        Ok(())
    }

    pub fn is_saved(&self, id: &str) -> bool {
        self.conn
            .query_row("SELECT saved FROM tracks WHERE id=?1", params![id], |r| r.get::<_, i64>(0))
            .map(|v| v > 0)
            .unwrap_or(false)
    }

    pub fn touch_played(&self, t: &Track) {
        if self.save_track(t).is_ok() {
            let _ = self.conn.execute(
                "UPDATE tracks SET played = strftime('%s','now') WHERE id=?1",
                params![t.id],
            );
        }
    }

    pub fn playlists(&self) -> Vec<Playlist> {
        let mut out = Vec::new();
        if let Ok(mut st) = self.conn.prepare(
            "SELECT p.id, p.name, COUNT(i.track_id), COALESCE(p.remote,''), COALESCE(p.art,'')
             FROM playlists p
             LEFT JOIN playlist_items i ON i.playlist_id = p.id
             GROUP BY p.id ORDER BY p.created ASC, p.id ASC",
        ) {
            if let Ok(rows) = st.query_map([], |r| {
                Ok(Playlist {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    count: r.get(2)?,
                    remote: r.get(3)?,
                    art: r.get(4)?,
                })
            }) {
                for r in rows.flatten() {
                    out.push(r);
                }
            }
        }
        out
    }

    pub fn create_playlist(&self, name: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO playlists(name,created) VALUES(?1,strftime('%s','now'))",
            params![name],
        )?;
        let id = self.conn.query_row(
            "SELECT id FROM playlists WHERE name = ?1",
            params![name],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// Create or refresh a playlist that mirrors one on YouTube Music.
    pub fn upsert_remote_playlist(&self, name: &str, remote: &str, art: &str) -> Result<i64> {
        let existing: Option<i64> = self
            .conn
            .query_row("SELECT id FROM playlists WHERE remote=?1", params![remote], |r| r.get(0))
            .optional()?;
        if let Some(id) = existing {
            self.conn.execute(
                "UPDATE playlists SET art=?2 WHERE id=?1 AND ?2 != ''",
                params![id, art],
            )?;
            return Ok(id);
        }
        // Names are unique; suffix clashes with local lists.
        let mut candidate = name.to_string();
        let mut n = 2;
        while self
            .conn
            .query_row("SELECT 1 FROM playlists WHERE name=?1", params![candidate], |_| Ok(()))
            .optional()?
            .is_some()
        {
            candidate = format!("{name} ({n})");
            n += 1;
        }
        self.conn.execute(
            "INSERT INTO playlists(name,created,remote,art) VALUES(?1,strftime('%s','now'),?2,?3)",
            params![candidate, remote, art],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Replace a playlist's cached songs (used for remote playlists).
    pub fn set_playlist_tracks(&mut self, playlist: i64, tracks: &[Track]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM playlist_items WHERE playlist_id=?1", params![playlist])?;
        for (i, t) in tracks.iter().enumerate() {
            tx.execute(
                "INSERT INTO tracks(id,title,artist,album,duration,art,source,added)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,strftime('%s','now'))
                 ON CONFLICT(id) DO UPDATE SET title=?2, artist=?3, album=?4,
                   duration=CASE WHEN ?5 > 0 THEN ?5 ELSE duration END, art=?6",
                params![t.id, t.title, t.artist, t.album, t.duration, t.art, t.source],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO playlist_items(playlist_id,track_id,pos) VALUES(?1,?2,?3)",
                params![playlist, t.id, i as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_playlist(&self, playlist: i64) -> Result<()> {
        self.conn.execute("DELETE FROM playlist_items WHERE playlist_id=?1", params![playlist])?;
        self.conn.execute("DELETE FROM playlists WHERE id=?1", params![playlist])?;
        Ok(())
    }

    pub fn remove_from_playlist(&self, playlist: i64, track: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM playlist_items WHERE playlist_id=?1 AND track_id=?2",
            params![playlist, track],
        )?;
        Ok(())
    }

    pub fn add_to_playlist(&self, playlist: i64, t: &Track) -> Result<()> {
        self.save_track(t)?;
        let pos: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(pos),0)+1 FROM playlist_items WHERE playlist_id=?1",
            params![playlist],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO playlist_items(playlist_id,track_id,pos) VALUES(?1,?2,?3)",
            params![playlist, t.id, pos],
        )?;
        Ok(())
    }

    pub fn playlist_tracks(&self, playlist: i64) -> Vec<Track> {
        let mut out = Vec::new();
        if let Ok(mut st) = self.conn.prepare(
            "SELECT t.id,t.title,t.artist,t.album,t.duration,t.art,t.source
             FROM playlist_items i JOIN tracks t ON t.id = i.track_id
             WHERE i.playlist_id = ?1 ORDER BY i.pos ASC",
        ) {
            if let Ok(rows) = st.query_map(params![playlist], track_from_row) {
                for r in rows.flatten() {
                    out.push(r);
                }
            }
        }
        out
    }

    pub fn lyrics(&self, id: &str) -> Option<(String, String)> {
        self.conn
            .query_row(
                "SELECT COALESCE(synced,''), COALESCE(plain,'') FROM lyrics WHERE track_id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn save_lyrics(&self, id: &str, synced: Option<&str>, plain: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO lyrics(track_id,synced,plain) VALUES(?1,?2,?3)
             ON CONFLICT(track_id) DO UPDATE SET synced=?2, plain=?3",
            params![id, synced, plain],
        )?;
        Ok(())
    }
}
