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

    pub fn library(&self) -> Vec<Track> {
        let mut out = Vec::new();
        if let Ok(mut st) = self.conn.prepare(
            "SELECT id,title,artist,album,duration,art,source FROM tracks ORDER BY added DESC",
        ) {
            if let Ok(rows) = st.query_map([], track_from_row) {
                for r in rows.flatten() {
                    out.push(r);
                }
            }
        }
        out
    }

    pub fn playlists(&self) -> Vec<Playlist> {
        let mut out = Vec::new();
        if let Ok(mut st) = self.conn.prepare(
            "SELECT p.id, p.name, COUNT(i.track_id) FROM playlists p
             LEFT JOIN playlist_items i ON i.playlist_id = p.id
             GROUP BY p.id ORDER BY p.created ASC",
        ) {
            if let Ok(rows) = st.query_map([], |r| {
                Ok(Playlist {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    count: r.get(2)?,
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

    #[allow(dead_code)]
    pub fn history_add(&self, id: &str) {
        let _ = self.conn.execute(
            "INSERT INTO tracks(id,title,artist,album,duration,art,source,added)
             SELECT id,title,artist,album,duration,art,source,strftime('%s','now')
             FROM tracks WHERE id=?1",
            params![id],
        );
    }
}
