// Phone player for check П2 (docs/mobile/checks.md) and the base of М3: the Rust core decodes
// (symphonia + libopus, `Player`), the system pulls samples through `sirtone_render` from an
// AVAudioSourceNode render callback; lock screen, Dynamic Island and headphone buttons via
// MPNowPlayingInfoCenter / MPRemoteCommandCenter (Apple «Becoming a now playable app»).
#if canImport(SirToneCore)
import AVFoundation
import Foundation
import MediaPlayer
import SirToneCore
import sirtone_mobile_ffiFFI

@MainActor
final class PhonePlayer: ObservableObject {
    @Published private(set) var files: [URL] = []
    @Published private(set) var index: Int?
    @Published private(set) var playing = false
    @Published private(set) var positionMs: UInt64 = 0
    @Published private(set) var format: TrackFormat?
    @Published private(set) var failed: String?

    private let core = Player()
    private let engine = AVAudioEngine()
    private var node: AVAudioSourceNode?
    private var handle: UInt64 = 0
    private var timer: Timer?
    private var observers: [NSObjectProtocol] = []
    /// Unreadable files in a row: stop instead of spinning through a folder of broken files.
    private var failures = 0

    static let audioExtensions: Set<String> = ["flac", "opus", "ogg", "oga", "mp3", "m4a", "wav", "aiff", "aif"]

    init() {
        setUpCommands()
        observeSession()
    }

    // MARK: Library (the app's Documents folder: Files → On My iPhone → Сиртон)

    func reload() {
        let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let found = FileManager.default.enumerator(at: docs, includingPropertiesForKeys: nil)?
            .compactMap { $0 as? URL }
            .filter { Self.audioExtensions.contains($0.pathExtension.lowercased()) } ?? []
        files = found.sorted { $0.path.localizedStandardCompare($1.path) == .orderedAscending }
    }

    // MARK: Transport

    func play(at i: Int) {
        guard files.indices.contains(i) else { return }
        index = i
        failed = nil
        let url = files[i]
        let core = self.core
        Task {
            // Opening reads the file: off the main thread (docs/mobile/README.md §7).
            let opened = await Task.detached { () -> (TrackFormat, UInt64)? in
                guard let f = try? core.open(path: url.path) else { return nil }
                return (f, core.takeRenderHandle())
            }.value
            guard let (f, h) = opened else {
                failed = url.lastPathComponent
                failures += 1
                if failures < files.count { next() } else { pause() }
                return
            }
            failures = 0
            format = f
            attach(handle: h, format: f)
            resume()
        }
    }

    func toggle() {
        if playing { pause() } else if index == nil { play(at: 0) } else { resume() }
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

    func next() {
        guard !files.isEmpty else { return }
        play(at: ((index ?? -1) + 1) % files.count)
    }

    func previous() {
        guard !files.isEmpty else { return }
        if positionMs > 3_000, let i = index { play(at: i); return }
        play(at: ((index ?? 1) - 1 + files.count) % files.count)
    }

    func seek(to ms: UInt64) {
        guard let f = format else { return }
        let core = self.core
        Task {
            let h = await Task.detached { () -> UInt64? in
                (try? core.seek(ms: ms)) == nil ? nil : core.takeRenderHandle()
            }.value
            guard let h else { return }
            attach(handle: h, format: f)
            positionMs = ms
            updateNowPlaying()
        }
    }

    // MARK: Output

    /// A fresh source node per open/seek: the track's own rate and channel count (the mixer
    /// converts to the hardware); the old node is detached before its handle is freed.
    private func attach(handle h: UInt64, format f: TrackFormat) {
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

    /// Built outside the main actor: the render block runs on the real-time audio thread and must
    /// not inherit main-actor isolation.
    private nonisolated static func makeNode(handle: UInt64, format f: TrackFormat) -> AVAudioSourceNode? {
        let channels = f.channels
        guard let fmt = AVAudioFormat(
            commonFormat: .pcmFormatFloat32,
            sampleRate: Double(f.sampleRate),
            channels: AVAudioChannelCount(channels),
            interleaved: true
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
        positionMs = core.positionMs()
        if playing && core.ended() { next() }
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
        guard let i = index, files.indices.contains(i) else {
            MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
            return
        }
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: files[i].deletingPathExtension().lastPathComponent,
            MPMediaItemPropertyAlbumTitle: "Сиртон",
            MPNowPlayingInfoPropertyElapsedPlaybackTime: Double(core.positionMs()) / 1000,
            MPNowPlayingInfoPropertyPlaybackRate: playing ? 1.0 : 0.0,
        ]
        if let d = format?.durationMs { info[MPMediaItemPropertyPlaybackDuration] = Double(d) / 1000 }
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
        MPNowPlayingInfoCenter.default().playbackState = playing ? .playing : .paused
    }
}
#endif
