// Playback (M3.1–M3.3, S04–S06): the Rust core decodes (`Player`), the system pulls samples
// through `sirtone_render` from an AVAudioSourceNode (checked in П2); a queue of library tracks,
// «Моя волна» refilling it, the listening journal for the wave, the lock screen, Dynamic Island
// and headphone buttons via MPNowPlayingInfoCenter / MPRemoteCommandCenter.
#if canImport(SirToneCore)
import AVFoundation
import Foundation
import MediaPlayer
import SirToneCore
import UIKit
import sirtone_mobile_ffiFFI

@MainActor
final class Playback: ObservableObject {
    struct Item: Identifiable, Equatable {
        let id = UUID()
        let track: TrackItem
        /// Chosen by the wave (journal source, «Почему этот трек»).
        let fromWave: Bool
    }

    @Published private(set) var queue: [Item] = []
    @Published private(set) var index: Int?
    @Published private(set) var playing = false
    @Published private(set) var positionMs: UInt64 = 0
    @Published private(set) var durationMs: UInt64 = 0
    @Published private(set) var liked = false
    @Published private(set) var cover: UIImage?
    /// The queue continues with the wave when it runs low (S06 «Волна продолжится»).
    @Published private(set) var waveOn = false
    @Published var novelty: Float = 0.5
    /// A short message for the toast (DESIGN.md §9: errors are toasts, never modal).
    @Published var notice: String?
    /// «Сон» (F09, AT-17): when the music stops; the last 10 minutes fade linearly in dB.
    @Published private(set) var sleepAt: Date?

    var current: Item? { index.flatMap { queue.indices.contains($0) ? queue[$0] : nil } }
    var upNext: ArraySlice<Item> { index.map { queue.dropFirst($0 + 1) } ?? queue[...] }

    private let core = Player()
    private let engine = AVAudioEngine()
    private var node: AVAudioSourceNode?
    private var handle: UInt64 = 0
    private var timer: Timer?
    private var observers: [NSObjectProtocol] = []
    private var failures = 0
    private weak var model: AppModel?
    private var refilling = false

    init() {
        setUpCommands()
        observeSession()
    }

    func attach(model: AppModel) { self.model = model }

    private var offsetMin: Int32 { Int32(TimeZone.current.secondsFromGMT() / 60) }

    // MARK: Starting

    func play(_ tracks: [TrackItem], at start: Int = 0, shuffled: Bool = false) {
        guard !tracks.isEmpty else { return }
        logCurrent(skipped: true)
        waveOn = false
        var list = tracks
        var first = min(max(start, 0), list.count - 1)
        if shuffled { list.shuffle(); first = 0 }
        queue = list.map { Item(track: $0, fromWave: false) }
        open(at: first)
    }

    func startWave() {
        guard let wave = model?.wave, let lib = model?.library else { return }
        logCurrent(skipped: true)
        waveOn = true
        let novelty = self.novelty, offset = offsetMin
        Task {
            let picked = await Task.detached { () -> [TrackItem] in
                let ids = (try? wave.next(count: 5, novelty: novelty, utcOffsetMin: offset)) ?? []
                return (try? lib.tracksByIds(ids: ids)) ?? []
            }.value
            guard !picked.isEmpty else { waveOn = false; return }
            queue = picked.map { Item(track: $0, fromWave: true) }
            open(at: 0)
        }
    }

    func playNext(_ t: TrackItem) {
        let at = (index ?? -1) + 1
        queue.insert(Item(track: t, fromWave: false), at: min(at, queue.count))
        if index == nil { open(at: 0) }
    }

    func remove(at offsets: IndexSet) {
        // Only the «up next» part is editable (the playing one stays).
        guard let i = index else { return }
        let absolute = IndexSet(offsets.map { $0 + i + 1 })
        queue.remove(atOffsets: absolute)
    }

    func move(from: IndexSet, to: Int) {
        guard let i = index else { return }
        queue.move(fromOffsets: IndexSet(from.map { $0 + i + 1 }), toOffset: to + i + 1)
    }

    // MARK: Transport

    func toggle() {
        if playing { pause() } else if index == nil { if !queue.isEmpty { open(at: 0) } } else { resume() }
    }

    func resume() {
        activateSession()
        if !engine.isRunning { try? engine.start() }
        core.play()
        playing = true
        startTimer()
        updateNowPlaying()
    }

    func pause() {
        core.pause()
        playing = false
        updateNowPlaying()
    }

    /// `user`: the person pressed «next» (the journal counts it as a skip when early).
    func next(user: Bool = true) {
        guard let i = index else { return }
        logCurrent(skipped: user)
        if i + 1 < queue.count {
            open(at: i + 1)
        } else if waveOn {
            refill(thenPlay: true)
        } else {
            pause()
        }
    }

    func previous() {
        guard let i = index else { return }
        if positionMs > 3_000 || i == 0 { seek(to: 0); return }
        logCurrent(skipped: true)
        open(at: i - 1)
    }

    func jump(to item: Item) {
        guard let i = queue.firstIndex(of: item) else { return }
        logCurrent(skipped: true)
        open(at: i)
    }

    func seek(to ms: UInt64) {
        guard index != nil else { return }
        let core = self.core
        Task {
            let h = await Task.detached { () -> UInt64? in
                (try? core.seek(ms: ms)) == nil ? nil : core.takeRenderHandle()
            }.value
            guard let h, let f = lastFormat else { return }
            attachNode(handle: h, format: f)
            positionMs = ms
            updateNowPlaying()
        }
    }

    // MARK: Sleep (F09)

    private static let fadeSpan: TimeInterval = 600
    private static let fadeFloorDb: Float = -40

    func sleep(minutes: Int) {
        sleepAt = Date().addingTimeInterval(TimeInterval(minutes * 60))
        core.setVolume(volume: 1)
    }

    /// «+15 мин»: later, and the volume comes back at once.
    func sleepLater() {
        guard let at = sleepAt else { return }
        sleepAt = max(at, Date()).addingTimeInterval(15 * 60)
        core.setVolume(volume: 1)
    }

    func sleepCancel() {
        sleepAt = nil
        core.setVolume(volume: 1)
    }

    private func sleepTick() {
        guard let at = sleepAt else { return }
        let left = at.timeIntervalSinceNow
        if left <= 0 {
            pause()
            sleepAt = nil
            core.setVolume(volume: 1)
            return
        }
        guard left < Self.fadeSpan else { return }
        // Linear in dB from 0 to the floor over the last 10 minutes; the final 30 s at the floor.
        let t = Float(max(0, left - 30) / (Self.fadeSpan - 30))
        let db = Self.fadeFloorDb * (1 - t)
        core.setVolume(volume: powf(10, db / 20))
    }

    // MARK: Marks

    /// ♡ — optimistic (DESIGN.md §1): the heart changes at once, a failure rolls it back quietly.
    func toggleLike() {
        guard let t = current?.track, let lib = model?.library else { return }
        let on = !liked
        liked = on
        Haptics.tap()
        Task {
            let ok = await Task.detached { (try? lib.setLike(trackId: t.id, on: on)) != nil }.value
            if !ok, current?.track.id == t.id { liked = !on }
        }
    }

    /// «Не сейчас»: a minus for this moment only, then the next track.
    func notNow() {
        guard let t = current?.track, let wave = model?.wave else { next(); return }
        let offset = offsetMin
        Task.detached { try? wave.notNow(trackId: t.id, utcOffsetMin: offset) }
        Haptics.tap()
        next(user: false)
    }

    func why() async -> [String] {
        guard let t = current?.track, let wave = model?.wave else { return [] }
        return await Task.detached { wave.why(trackId: t.id) }.value
    }

    // MARK: Inside

    private var lastFormat: TrackFormat?
    private var heardFrom: UInt64 = 0

    private func open(at i: Int) {
        guard queue.indices.contains(i), let lib = model?.library else { return }
        index = i
        let item = queue[i]
        positionMs = 0
        durationMs = item.track.durationMs
        cover = nil
        heardFrom = 0
        let core = self.core
        Task {
            let opened = await Task.detached { () -> (TrackFormat, UInt64, Bool, UIImage?)? in
                guard let path = try? lib.trackPath(trackId: item.track.id),
                      let f = try? core.open(path: path) else { return nil }
                let liked = (try? lib.isLiked(trackId: item.track.id)) ?? false
                let art = lib.trackCover(trackId: item.track.id)
                    .flatMap { lib.coverPath(hash: $0, size: 512) }
                    .flatMap { UIImage(contentsOfFile: $0) }
                return (f, core.takeRenderHandle(), liked, art)
            }.value
            guard index == i else { return }
            guard let (f, h, liked, art) = opened else {
                failures += 1
                notice = L.t("player.open_failed")
                if failures < 5 { next(user: false) } else { pause(); failures = 0 }
                return
            }
            failures = 0
            lastFormat = f
            if let d = f.durationMs, d > 0 { durationMs = d }
            self.liked = liked
            self.cover = art
            attachNode(handle: h, format: f)
            resume()
            Haptics.tap()
            if waveOn, queue.count - i <= 2 { refill(thenPlay: false) }
        }
    }

    /// Adds the wave's next picks to the end of the queue.
    private func refill(thenPlay: Bool) {
        guard !refilling, let wave = model?.wave, let lib = model?.library else { return }
        refilling = true
        let novelty = self.novelty, offset = offsetMin
        Task {
            let picked = await Task.detached { () -> [TrackItem] in
                let ids = (try? wave.next(count: 5, novelty: novelty, utcOffsetMin: offset)) ?? []
                return (try? lib.tracksByIds(ids: ids)) ?? []
            }.value
            refilling = false
            let start = queue.count
            queue += picked.map { Item(track: $0, fromWave: true) }
            if thenPlay, queue.count > start { open(at: start) } else if thenPlay { pause() }
        }
    }

    /// The journal row for the track that is ending (the wave learns from it).
    private func logCurrent(skipped: Bool) {
        guard let item = current, let wave = model?.wave else { return }
        let heard = positionMs > heardFrom ? positionMs - heardFrom : 0
        guard heard >= 1_000 else { return }
        let early = skipped && (heard < 30_000 || heard * 2 < durationMs)
        let duration = durationMs, offset = offsetMin
        Task.detached {
            try? wave.listened(
                trackId: item.track.id, playedMs: heard, durationMs: duration,
                skipped: early, fromWave: item.fromWave, utcOffsetMin: offset)
        }
        heardFrom = positionMs
    }

    /// A fresh source node per open/seek: the track's own rate and channel count (the mixer
    /// converts to the hardware); the old node is detached before its handle is freed.
    private func attachNode(handle h: UInt64, format f: TrackFormat) {
        guard let n = Self.makeNode(handle: h, format: f) else { return }
        let old = node
        let oldHandle = handle
        engine.attach(n)
        engine.connect(n, to: engine.mainMixerNode, format: n.outputFormat(forBus: 0))
        node = n
        handle = h
        if let old { engine.detach(old) }
        if oldHandle != 0 { sirtone_render_release(oldHandle) }
    }

    /// Built outside the main actor: the render block runs on the real-time audio thread.
    private nonisolated static func makeNode(handle: UInt64, format f: TrackFormat) -> AVAudioSourceNode? {
        let channels = f.channels
        guard let fmt = AVAudioFormat(
            commonFormat: .pcmFormatFloat32, sampleRate: Double(f.sampleRate),
            channels: AVAudioChannelCount(channels), interleaved: true
        ) else { return nil }
        return AVAudioSourceNode(format: fmt) { _, _, frames, abl -> OSStatus in
            let buffers = UnsafeMutableAudioBufferListPointer(abl)
            if let data = buffers.first?.mData?.assumingMemoryBound(to: Float.self) {
                _ = sirtone_render(handle, data, frames, channels)
            }
            return noErr
        }
    }

    private func startTimer() {
        guard timer == nil else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
    }

    private func tick() {
        sleepTick()
        guard index != nil else { return }
        positionMs = core.positionMs()
        if playing && core.ended() { next(user: false) }
    }

    // MARK: Session, lock screen, buttons

    private func activateSession() {
        let s = AVAudioSession.sharedInstance()
        try? s.setCategory(.playback, mode: .default)
        try? s.setActive(true)
    }

    private func observeSession() {
        let nc = NotificationCenter.default
        observers.append(nc.addObserver(
            forName: AVAudioSession.interruptionNotification, object: nil, queue: .main
        ) { [weak self] note in
            let raw = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt
            let began = raw == AVAudioSession.InterruptionType.began.rawValue
            MainActor.assumeIsolated { if began { self?.pause() } }
        })
        observers.append(nc.addObserver(
            forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main
        ) { [weak self] note in
            // Headphones pulled out: pause, as every player does.
            let raw = note.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt
            let gone = raw == AVAudioSession.RouteChangeReason.oldDeviceUnavailable.rawValue
            MainActor.assumeIsolated { if gone { self?.pause() } }
        })
        observers.append(nc.addObserver(
            forName: .AVAudioEngineConfigurationChange, object: engine, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, self.playing else { return }
                try? self.engine.start()
            }
        })
    }

    private func setUpCommands() {
        let c = MPRemoteCommandCenter.shared()
        c.playCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.resume() }
            return .success
        }
        c.pauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.pause() }
            return .success
        }
        c.togglePlayPauseCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.toggle() }
            return .success
        }
        c.nextTrackCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.next() }
            return .success
        }
        c.previousTrackCommand.addTarget { [weak self] _ in
            MainActor.assumeIsolated { self?.previous() }
            return .success
        }
        c.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let e = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            let ms = UInt64(max(0, e.positionTime) * 1000)
            MainActor.assumeIsolated { self?.seek(to: ms) }
            return .success
        }
    }

    private func updateNowPlaying() {
        guard let t = current?.track else {
            MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
            return
        }
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: t.title,
            MPMediaItemPropertyArtist: t.artists,
            MPNowPlayingInfoPropertyElapsedPlaybackTime: Double(core.positionMs()) / 1000,
            MPNowPlayingInfoPropertyPlaybackRate: playing ? 1.0 : 0.0,
            MPMediaItemPropertyPlaybackDuration: Double(durationMs) / 1000,
        ]
        if let a = t.album { info[MPMediaItemPropertyAlbumTitle] = a }
        if let img = cover {
            info[MPMediaItemPropertyArtwork] = MPMediaItemArtwork(boundsSize: img.size) { _ in img }
        }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
        MPNowPlayingInfoCenter.default().playbackState = playing ? .playing : .paused
    }
}

/// Tactile answer on Play, ♡, track change and drop (MR §4).
enum Haptics {
    @MainActor static func tap() { UIImpactFeedbackGenerator(style: .light).impactOccurred() }
}
#endif
