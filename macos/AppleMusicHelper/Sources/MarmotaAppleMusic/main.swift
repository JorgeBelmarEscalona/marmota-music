// Marmota Apple Music: a windowless helper that plays Apple Music songs with
// MusicKit's ApplicationMusicPlayer (macOS 14+), driven by the notch.
//
// Protocol: one JSON object per line.
//   stdin  {"cmd":"status"}                      authorization + subscription
//          {"cmd":"auth"}                        ask for access (system prompt)
//          {"cmd":"play","id":"1440650428"}      catalog song id
//          {"cmd":"pause"} {"cmd":"resume"} {"cmd":"stop"}
//          {"cmd":"seek","pos":42.5}
//          {"cmd":"interval","sec":1.0}           state reports while playing
//          {"cmd":"lyrics","id":"…"}              diagnostics, see README
//   stdout {"ev":"status","auth":"authorized","canPlay":true}
//          {"ev":"state","id":"…","status":"playing","pos":12.3,"dur":354.0}
//          {"ev":"ended","id":"…"}
//          {"ev":"error","id":"…","msg":"…"}
//
// `marmota-apple-music --authorize` asks for access once and exits (used by
// `marmota-music setup apple`). The helper exits when stdin closes, so it
// never outlives the notch.

import AppKit
import Combine
import Foundation
import MusicKit

final class Bridge {
    let player = ApplicationMusicPlayer.shared
    var currentID = ""
    var duration: Double = 0
    var interval: Double = 1.0
    var timer: Timer?
    var watchers: [AnyCancellable] = []
    /// The song really started (so a later stop is its end, not a load).
    var started = false
    var userPaused = false
    var endedSent = false
    var lastPos: Double = 0
    /// Queue entry of our song, to notice the player moving on by itself.
    var entryID: String?

    init() {
        watchers.append(player.state.objectWillChange.sink { [weak self] _ in
            // objectWillChange fires before the value changes.
            DispatchQueue.main.async { self?.changed() }
        })
        watchers.append(player.queue.objectWillChange.sink { [weak self] _ in
            DispatchQueue.main.async { self?.changed() }
        })
    }

    // MARK: output

    func emit(_ d: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: d) else { return }
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([0x0A]))
    }

    func fail(_ msg: String, id: String? = nil) {
        emit(["ev": "error", "id": id ?? currentID, "msg": msg])
    }

    func statusName(_ s: MusicPlayer.PlaybackStatus) -> String {
        switch s {
        case .playing: return "playing"
        case .paused: return "paused"
        case .stopped: return "stopped"
        case .interrupted: return "interrupted"
        case .seekingForward, .seekingBackward: return "seeking"
        @unknown default: return "unknown"
        }
    }

    func authName(_ s: MusicAuthorization.Status) -> String {
        switch s {
        case .authorized: return "authorized"
        case .denied: return "denied"
        case .restricted: return "restricted"
        case .notDetermined: return "notDetermined"
        @unknown default: return "unknown"
        }
    }

    func emitState() {
        let pos = player.playbackTime
        if player.state.playbackStatus == .playing { lastPos = pos }
        emit([
            "ev": "state",
            "id": currentID,
            "status": statusName(player.state.playbackStatus),
            "pos": pos,
            "dur": duration,
        ])
    }

    // MARK: playback state

    func changed() {
        let status = player.state.playbackStatus
        if status == .playing {
            if !started { entryID = player.queue.currentEntry?.id }
            started = true
            userPaused = false
        }
        emitState()
        restartTimer()
        // The notch owns the queue: if the player went on to something else
        // (autoplay), our song is over.
        if started && !endedSent, let now = player.queue.currentEntry?.id, let ours = entryID, now != ours {
            endedSent = true
            player.pause()
            emit(["ev": "ended", "id": currentID])
            return
        }
        // A single-song queue: when it stops on its own, the song is over.
        if started && !endedSent && !userPaused && status != .playing && status != .seekingForward
            && status != .seekingBackward && status != .interrupted
        {
            let pos = player.playbackTime
            let nearEnd = duration > 0 && (pos >= duration - 1.5 || lastPos >= duration - 2.5)
            if player.queue.currentEntry == nil || nearEnd {
                endedSent = true
                emit(["ev": "ended", "id": currentID])
            }
        }
    }

    /// Position reports only while a song plays, at the pace the notch asks
    /// for (fast while it is open, slow while it is hidden).
    func restartTimer() {
        timer?.invalidate()
        timer = nil
        guard player.state.playbackStatus == .playing, interval > 0 else { return }
        let t = Timer(timeInterval: interval, repeats: true) { [weak self] _ in
            self?.emitState()
        }
        t.tolerance = interval * 0.2
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    // MARK: commands

    func status() async {
        let auth = MusicAuthorization.currentStatus
        var out: [String: Any] = ["ev": "status", "auth": authName(auth)]
        if auth == .authorized {
            if let sub = try? await MusicSubscription.current {
                out["canPlay"] = sub.canPlayCatalogContent
            }
        }
        emit(out)
    }

    func authorize() async -> MusicAuthorization.Status {
        if MusicAuthorization.currentStatus == .notDetermined {
            // An accessory app has to come forward for the prompt to show.
            NSApp.activate(ignoringOtherApps: true)
        }
        return await MusicAuthorization.request()
    }

    func play(_ id: String) async {
        currentID = id
        duration = 0
        started = false
        endedSent = false
        userPaused = false
        lastPos = 0
        entryID = nil
        let auth = await authorize()
        guard auth == .authorized else {
            fail("Sin permiso para usar Apple Music (\(authName(auth)))", id: id)
            return
        }
        do {
            let req = MusicCatalogResourceRequest<Song>(matching: \.id, equalTo: MusicItemID(id))
            let resp = try await req.response()
            guard let song = resp.items.first else {
                fail("La canción no está en el catálogo de tu país", id: id)
                return
            }
            guard currentID == id else { return }
            duration = song.duration ?? 0
            player.queue = ApplicationMusicPlayer.Queue(for: [song])
            try await player.play()
            emitState()
        } catch {
            guard currentID == id else { return }
            fail(describe(error), id: id)
        }
    }

    func describe(_ error: Error) -> String {
        let text = String(describing: error)
        if text.contains("developerToken") || text.contains("DeveloperToken") {
            return "MusicKit no pudo crear el token: el App ID del ayudante necesita el servicio MusicKit (\(text))"
        }
        if text.contains("privacyAcknowledgement") {
            return "Abre la app Música una vez y acepta sus condiciones"
        }
        if text.contains("subscription") || text.contains("Subscription") {
            return "Tu cuenta no tiene una suscripción de Apple Music activa"
        }
        return text
    }

    /// Diagnostics: ask Apple for the song's lyrics with MusicKit's own
    /// tokens and report what it answers. The notch does not use this.
    func lyrics(_ id: String) async {
        do {
            let sf = try await MusicDataRequest.currentCountryCode
            guard let url = URL(string: "https://api.music.apple.com/v1/catalog/\(sf)/songs/\(id)/lyrics") else { return }
            let resp = try await MusicDataRequest(urlRequest: URLRequest(url: url)).response()
            emit(["ev": "lyrics", "id": id, "status": resp.urlResponse.statusCode,
                  "body": String(decoding: resp.data.prefix(400), as: UTF8.self)])
        } catch {
            emit(["ev": "lyrics", "id": id, "status": 0, "body": String(describing: error)])
        }
    }

    func handle(_ msg: [String: Any]) async {
        switch msg["cmd"] as? String {
        case "status":
            await status()
        case "auth":
            _ = await authorize()
            await status()
        case "play":
            if let id = msg["id"] as? String { await play(id) }
        case "pause":
            userPaused = true
            player.pause()
        case "resume":
            do { try await player.play() } catch { fail(describe(error)) }
        case "stop":
            userPaused = true
            player.stop()
        case "seek":
            if let pos = (msg["pos"] as? NSNumber)?.doubleValue {
                player.playbackTime = max(0, pos)
                emitState()
            }
        case "interval":
            if let sec = (msg["sec"] as? NSNumber)?.doubleValue {
                interval = max(0.25, sec)
                restartTimer()
            }
        case "lyrics":
            if let id = msg["id"] as? String { await lyrics(id) }
        case "quit":
            exit(0)
        default:
            fail("Orden desconocida")
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let bridge = Bridge()

if CommandLine.arguments.contains("--authorize") {
    Task { @MainActor in
        let s = await bridge.authorize()
        var line = "Apple Music: \(bridge.authName(s))"
        if s == .authorized, let sub = try? await MusicSubscription.current {
            line += sub.canPlayCatalogContent ? " · suscripción activa" : " · sin suscripción"
        }
        print(line)
        exit(s == .authorized ? 0 : 1)
    }
} else {
    // Commands come in on a background thread and run on the main one, in order.
    Thread.detachNewThread {
        while let line = readLine() {
            guard let data = line.data(using: .utf8),
                  let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            else { continue }
            let done = DispatchSemaphore(value: 0)
            Task { @MainActor in
                await bridge.handle(obj)
                done.signal()
            }
            // `play` may wait on the authorization prompt; later commands wait too.
            done.wait()
        }
        // The notch went away.
        exit(0)
    }
}

app.run()
