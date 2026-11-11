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

    /// Two decks (D-148): one plays; over a transition the other comes in, then they swap.
    /// With transitions off only `active` is ever used — the path П2 checked.
    private final class Deck {
        let core = Player()
        var node: AVAudioSourceNode?
        var handle: UInt64 = 0
    }
    private var active = Deck()
    private var standby = Deck()
    private var core: Player { active.core }
    private let engine = AVAudioEngine()
    /// Bumped when the decks swap: the visualiser takes the new deck's feed.
    @Published private(set) var deckGen = 0
    /// The handover planned for the track after the current one.
    private var plan: (next: Item.ID, mix: MixPlan)?
    private var mixing: Task<Void, Never>?
    private var fadingOut: Deck?
    /// The sleep fade's level (both decks follow it).
    private var level: Float = 1
    private var timer: Timer?
    private var observers: [NSObjectProtocol] = []
    private var failures = 0
    private weak var model: AppModel?
    private var refilling = false

    init() {
        setUpCommands()
        observeSession()
        applyEq()
        // D-167: the decks know each other and the host clock — the next track starts on the
        // outgoing one's cue, to the sample.
        active.core.pair(other: standby.core)
        var tb = mach_timebase_info_data_t()
        mach_timebase_info(&tb)
        let ticks = 1e9 * Double(tb.denom) / Double(max(tb.numer, 1))
        active.core.setHostClock(ticksPerSecond: ticks)
        standby.core.setHostClock(ticksPerSecond: ticks)
    }

    func attach(model: AppModel) { self.model = model }

    /// The visualiser's feed (F22): while it lives, the core copies what plays aside.
    func vizFeed() -> VizFeed { active.core.viz() }

    private var offsetMin: Int32 { Int32(TimeZone.current.secondsFromGMT() / 60) }

    // MARK: Starting

    func play(_ tracks: [TrackItem], at start: Int = 0, shuffled: Bool = false) {
        guard !tracks.isEmpty else { return }
        finishMix()
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
        if index == nil { open(at: 0) } else { planNext() }
    }

    func remove(at offsets: IndexSet) {
        // Only the «up next» part is editable (the playing one stays).
        guard let i = index else { return }
        let absolute = IndexSet(offsets.map { $0 + i + 1 })
        queue.remove(atOffsets: absolute)
        planNext()
    }

    func move(from: IndexSet, to: Int) {
        guard let i = index else { return }
        queue.move(fromOffsets: IndexSet(from.map { $0 + i + 1 }), toOffset: to + i + 1)
        planNext()
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
        finishMix()
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
        finishMix()
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
        setLevel(1)
    }

    /// «+15 мин»: later, and the volume comes back at once.
    func sleepLater() {
        guard let at = sleepAt else { return }
        sleepAt = max(at, Date()).addingTimeInterval(15 * 60)
        setLevel(1)
    }

    func sleepCancel() {
        sleepAt = nil
        setLevel(1)
    }

    private func sleepTick() {
        guard let at = sleepAt else { return }
        let left = at.timeIntervalSinceNow
        if left <= 0 {
            pause()
            sleepAt = nil
            setLevel(1)
            return
        }
        guard left < Self.fadeSpan else { return }
        // Linear in dB from 0 to the floor over the last 10 minutes; the final 30 s at the floor.
        let t = Float(max(0, left - 30) / (Self.fadeSpan - 30))
        let db = Self.fadeFloorDb * (1 - t)
        setLevel(powf(10, db / 20))
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

    /// Deleted tracks (D-165) leave the queue; the playing one hands over to the next first.
    func forget(_ ids: Set<Int64>) {
        guard !ids.isEmpty else { return }
        if let c = current, ids.contains(c.track.id) { next(user: false) }
        guard let i = index, queue.indices.contains(i) else {
            queue.removeAll { ids.contains($0.track.id) }
            return
        }
        let now = queue[i]
        queue.removeAll { ids.contains($0.track.id) && $0 != now }
        index = queue.firstIndex(of: now)
        planNext()
    }

    /// «Никогда в волне»: the wave never picks it; it leaves the wave's queue at once.
    func never(_ t: TrackItem) {
        guard let lib = model?.library else { return }
        Task.detached { try? lib.setNever(trackId: t.id, on: true) }
        let keep = (index ?? -1) + 1
        queue = queue.enumerated()
            .filter { $0.offset < keep || !($0.element.fromWave && $0.element.track.id == t.id) }
            .map(\.element)
        if current?.track.id == t.id, current?.fromWave == true { next(user: false) }
        notice = L.t("never.done")
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
    /// A track is being opened: the core still reports the old one as ended until it is in, so
    /// the timer must not move on a second time.
    private var opening = false

    private func open(at i: Int) {
        guard queue.indices.contains(i), let lib = model?.library else { return }
        finishMix()
        plan = nil
        index = i
        let item = queue[i]
        positionMs = 0
        durationMs = item.track.durationMs
        cover = nil
        heardFrom = 0
        opening = true
        let core = self.core
        Task {
            defer { if index == i { opening = false } }
            let gainTarget = SoundPrefs.normTarget
            let opened = await Task.detached { () -> (TrackFormat, UInt64, Bool, UIImage?)? in
                // The gain before the open: the new decoder starts at it (no slide from the last).
                core.setGainDb(db: gainTarget.map { lib.normGainDb(trackId: item.track.id, targetLufs: $0) } ?? 0)
                guard let path = try? lib.trackPath(trackId: item.track.id),
                      let f = try? core.open(path: path) else { return nil }
                core.fadeTo(target: 1, ms: 0)
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
            planNext()
        }
    }

    /// «Настроить волну» changed: the wave's picks after the current track are asked again.
    func retune() {
        guard waveOn else { return }
        let keep = (index ?? -1) + 1
        queue = queue.enumerated().filter { $0.offset < keep || !$0.element.fromWave }.map(\.element)
        planNext()
        refill(thenPlay: false)
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
            if thenPlay, queue.count > start { open(at: start) } else if thenPlay { pause() } else { planNext() }
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
    private func attachNode(handle h: UInt64, format f: TrackFormat, to deck: Deck? = nil) {
        let d = deck ?? active
        guard let n = Self.makeNode(handle: h, format: f) else { return }
        let old = d.node
        let oldHandle = d.handle
        engine.attach(n)
        engine.connect(n, to: engine.mainMixerNode, format: n.outputFormat(forBus: 0))
        d.node = n
        d.handle = h
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
        return AVAudioSourceNode(format: fmt) { _, ts, frames, abl -> OSStatus in
            let buffers = UnsafeMutableAudioBufferListPointer(abl)
            if let data = buffers.first?.mData?.assumingMemoryBound(to: Float.self) {
                // The host time the block sounds at: a transition's start keys on it (D-167).
                let host = ts.pointee.mFlags.contains(.hostTimeValid) ? ts.pointee.mHostTime : 0
                _ = sirtone_render(handle, data, frames, channels, host)
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
        guard index != nil, !opening else { return }
        positionMs = core.positionMs()
        if let (nextId, mix) = plan, mixing == nil, playing,
           positionMs + 1_500 >= mix.aOutMs,
           let i = index, queue.indices.contains(i + 1), queue[i + 1].id == nextId {
            handOver(to: i + 1, mix: mix)
            return
        }
        if playing && mixing == nil && core.ended() { next(user: false) }
    }

    // MARK: Transitions (D-148: «Мягко», «Умно», crossfade — the PC's planner, `Library.mixPlan`)

    /// On screen the decoder stays ~0.4 s ahead (settings heard at once); off screen or locked
    /// 1.5 s, as nothing needs a quick answer there and the thread may be throttled.
    func setOnScreen(_ on: Bool) {
        for d in [active, standby] { d.core.setFillMs(ms: on ? 400 : 1_500) }
    }

    /// Both decks follow the sleep fade.
    private func setLevel(_ v: Float) {
        level = v
        active.core.setVolume(volume: v)
        standby.core.setVolume(volume: v)
    }

    /// The equalizer from the settings, on both decks (heard within 0.4 s).
    func applyEq() {
        let eq = SoundPrefs.eq
        for d in [active, standby] {
            d.core.setEq(enabled: eq.on, preampDb: eq.preamp, bandsDb: eq.bands)
        }
    }

    /// Normalization or transitions changed: the open track's gain again, the next handover again.
    func applySound() {
        applyEq()
        if let t = current?.track, let lib = model?.library {
            let target = SoundPrefs.normTarget
            let core = self.core
            Task.detached {
                core.setGainDb(db: target.map { lib.normGainDb(trackId: t.id, targetLufs: $0) } ?? 0)
            }
        }
        planNext()
    }

    /// Asks the planner how the current track hands over to the next one.
    private func planNext() {
        plan = nil
        // The outgoing curve is armed with the plan; never under a running transition.
        if mixing == nil { core.clearMix() }
        guard let mode = SoundPrefs.transition, let i = index, queue.indices.contains(i + 1),
              let lib = model?.library else { return }
        let a = queue[i], b = queue[i + 1]
        let sleeping = sleepAt != nil
        Task {
            let mix = await Task.detached {
                lib.mixPlan(aId: a.track.id, bId: b.track.id, mode: mode,
                            albumInOrder: false, sleep: sleeping)
            }.value
            guard let mix, index == i, current?.id == a.id else { return }
            plan = (b.id, mix)
            // A's side by its own frames, set now — long before the decoder gets there (D-167).
            if mixing == nil {
                core.setMix(outgoing: true, startMs: mix.aOutMs, lenMs: mix.overlapMs,
                            equalPower: mix.aEqualPower, stopMs: mix.aStopMs,
                            swapHz: mix.swapHz, swapAtMs: mix.swapAtMs)
            }
        }
    }

    /// Opens the next track on the other deck and starts it on the current one's cue, to the
    /// sample (D-167): the core plays both curves and the bass swap by frame, as the PC; the app
    /// only swaps the decks when B has started and lets A go after the overlap.
    private func handOver(to i: Int, mix: MixPlan) {
        guard let lib = model?.library else { return }
        plan = nil  // one attempt: a failed open falls back to the plain next
        let item = queue[i]
        let incoming = standby, outgoing = active
        let core = incoming.core
        let gainTarget = SoundPrefs.normTarget
        let vol = level
        mixing = Task {
            let opened = await Task.detached { () -> (TrackFormat, UInt64, Bool, UIImage?)? in
                core.setGainDb(db: gainTarget.map { lib.normGainDb(trackId: item.track.id, targetLufs: $0) } ?? 0)
                guard let path = try? lib.trackPath(trackId: item.track.id),
                      let f = try? core.open(path: path) else { return nil }
                if mix.bStartMs > 0 { try? core.seek(ms: mix.bStartMs) }
                core.setVolume(volume: vol)
                core.fadeTo(target: 1, ms: 0)
                core.setMix(outgoing: false, startMs: mix.bStartMs, lenMs: mix.overlapMs,
                            equalPower: mix.bEqualPower, stopMs: nil,
                            swapHz: mix.swapHz, swapAtMs: mix.swapAtMs)
                core.waitForCue()
                let liked = (try? lib.isLiked(trackId: item.track.id)) ?? false
                let art = lib.trackCover(trackId: item.track.id)
                    .flatMap { lib.coverPath(hash: $0, size: 512) }
                    .flatMap { UIImage(contentsOfFile: $0) }
                return (f, core.takeRenderHandle(), liked, art)
            }.value
            guard !Task.isCancelled, let (f, h, liked, art) = opened, index == i - 1 else {
                if let opened { sirtone_render_release(opened.1) }
                core.clearMix()
                // A plays to its own end: no fade or stop for a track that is not coming.
                outgoing.core.clearMix()
                mixing = nil
                return
            }
            attachNode(handle: h, format: f, to: incoming)
            // B renders silence until A's callback reaches the cue, then its first sample.
            core.play()
            outgoing.core.cuePartnerAt(ms: mix.aOutMs)
            // Bounded: A may end before its cue (a guessed length) or stall — then B starts now.
            let ahead = max(0, Int64(mix.aOutMs) - Int64(outgoing.core.positionMs()))
            let deadline = Date().addingTimeInterval(Double(ahead + 1_500) / 1000)
            var fallback = false
            while core.waiting() {
                if outgoing.core.ended() || Date() > deadline {
                    fallback = true
                    core.clearMix()
                    break
                }
                try? await Task.sleep(for: .milliseconds(20))
                guard !Task.isCancelled else { return }
            }
            MixStats.record(mix: mix, report: core.cueReport(), fallback: fallback)
            // The new track is the current one from here (A's journal row up to this moment).
            positionMs = outgoing.core.positionMs()
            logCurrent(skipped: false)
            active = incoming
            standby = outgoing
            fadingOut = outgoing
            deckGen += 1
            index = i
            lastFormat = f
            positionMs = core.positionMs()
            heardFrom = positionMs
            durationMs = f.durationMs.flatMap { $0 > 0 ? $0 : nil } ?? item.track.durationMs
            self.liked = liked
            self.cover = art
            updateNowPlaying()
            if waveOn, queue.count - i <= 2 { refill(thenPlay: false) }
            try? await Task.sleep(for: .milliseconds(Int(max(mix.overlapMs, 50))))
            if Task.isCancelled { return }  // finishMix stopped the old deck already
            stop(outgoing)
            outgoing.core.clearMix()
            fadingOut = nil
            mixing = nil
            planNext()
        }
    }

    /// A transition in progress ends at once: the old deck stops, the new one is at full level.
    private func finishMix() {
        guard mixing != nil || fadingOut != nil else { return }
        mixing?.cancel()
        mixing = nil
        if let o = fadingOut { stop(o) }
        fadingOut = nil
        if standby.node != nil { stop(standby) }
        // The one playing on is whole from here; no cue is left behind.
        active.core.clearMix()
        standby.core.clearMix()
        active.core.fadeTo(target: 1, ms: 0)
    }

    private func stop(_ d: Deck) {
        d.core.pause()
        if let n = d.node { engine.detach(n) }
        if d.handle != 0 { sirtone_render_release(d.handle) }
        d.node = nil
        d.handle = 0
        d.core.fadeTo(target: 1, ms: 0)
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
    @MainActor static func tap() {
        // «Только музыка» (D-147): no vibration motor either.
        let d = UserDefaults.standard
        let musicOnly = d.bool(forKey: "musicOnly")
            || ((d.object(forKey: "musicOnlyWithLowPower") as? Bool ?? true)
                && ProcessInfo.processInfo.isLowPowerModeEnabled)
        if !musicOnly { UIImpactFeedbackGenerator(style: .light).impactOccurred() }
    }
}

/// What each transition did (D-167), for the PC to read after a listen (`phone pull`):
/// `Documents/mix-stats.json` — the last 50 starts, planned vs actual host time.
enum MixStats {
    static func record(mix: MixPlan, report: CueReport, fallback: Bool) {
        guard let dir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
        else { return }
        let url = dir.appendingPathComponent("mix-stats.json")
        var all = (try? JSONSerialization.jsonObject(with: Data(contentsOf: url))) as? [[String: Any]] ?? []
        var tb = mach_timebase_info_data_t()
        mach_timebase_info(&tb)
        let diff = Int64(bitPattern: report.actualHost &- report.plannedHost)
        let us = report.plannedHost > 1 && report.actualHost > 0
            ? Double(diff) * Double(tb.numer) / Double(max(tb.denom, 1)) / 1000 : nil
        all.append([
            "at": Date().timeIntervalSince1970,
            "aOutMs": mix.aOutMs, "overlapMs": mix.overlapMs,
            "aEqualPower": mix.aEqualPower, "bEqualPower": mix.bEqualPower,
            "swapHz": mix.swapHz, "swapAtMs": mix.swapAtMs,
            "hostClock": report.plannedHost > 1, "startErrorUs": us as Any,
            "fallback": fallback,
        ])
        if all.count > 50 { all.removeFirst(all.count - 50) }
        if let data = try? JSONSerialization.data(withJSONObject: all, options: [.prettyPrinted]) {
            try? data.write(to: url)
        }
    }
}
#endif
