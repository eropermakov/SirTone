// The app's state (M4.1, MR §5, §11): the library in Application Support (excluded from iCloud
// backup), the wave over it, imports from the picker, «Поделиться» and «Входящие», and packages
// the PC hub left in Documents/Transfer/Inbox (applied when the app opens; `.complete` marks a
// whole package). The core is called off the main thread only (MR §7).
#if canImport(SirToneCore)
import Foundation
import SirToneCore
import SirToneDesign
import SwiftUI
import UIKit

@MainActor
final class AppModel: ObservableObject {
    @Published private(set) var library: Library?
    @Published private(set) var wave: Wave? {
        didSet { if wave !== oldValue { configureWave() } }
    }
    /// «Обстановка» (D-149): motion, car, places, weather — steering «Моя волна».
    let context = ContextService()
    /// The running wave's profile: `my_wave` or `sleep` («Засыпательная волна»).
    @Published private(set) var waveProfile = "my_wave"
    @Published private(set) var counts: LibraryCounts?
    /// Bumped after anything changes the library: lists reload.
    @Published private(set) var revision = 0
    @Published private(set) var importing = false {
        didSet { if importing != oldValue { watchWork() } }
    }
    /// Files done of files to do while the core imports or applies a package from the PC.
    @Published private(set) var work: WorkProgress?
    private var workTask: Task<Void, Never>?

    let playback = Playback()

    /// Theme: "system" | "dark" | "light"; colour: an accent slug (DESIGN.md §4).
    @AppStorage("theme") var theme = "system"
    @AppStorage("accent") var accent = "sirkov"
    /// «Качество при добавлении» (D-146): "original" | "economy" | "max"; «Оригинал» by default.
    @AppStorage("addQuality") var addQuality = "original"
    /// «Только музыка» (D-147): the switch, and «вместе с энергосбережением iPhone».
    @AppStorage("musicOnly") var musicOnlySwitch = false
    /// «Настроить волну» (S07), kept until «Сбросить» as on the PC.
    @AppStorage("waveTuning") private var tuningStored = ""
    var tuning: WaveTuning { WaveTuning(stored: tuningStored) }
    @AppStorage("musicOnlyWithLowPower") var musicOnlyWithLowPower = true
    /// Files waiting for the person to choose their quality («Поделиться», «Открыть в»).
    @Published var pendingImport: [URL] = []
    /// «Хранилище»: what the library keeps; making copies shows its progress here.
    @Published private(set) var storage: StorageInfo?
    @Published private(set) var copying: (done: Int, total: Int)?

    /// Inbox folder names the hub and the app agree on (`sirtone-devicelink::appfiles`).
    static let transferInbox = "Transfer/Inbox"
    static let complete = ".complete"

    private var opened = false

    private var powerObserver: NSObjectProtocol?

    init() {
        playback.attach(model: self)
        context.attach(model: self)
        // «Только музыка» follows Low Power Mode as it flips (D-147).
        powerObserver = NotificationCenter.default.addObserver(
            forName: .NSProcessInfoPowerStateDidChange, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.objectWillChange.send() }
        }
    }

    /// Long work (import, copies) keeps going for a while when the app leaves the screen.
    private func background<T: Sendable>(_ work: @escaping @Sendable () -> T) async -> T {
        let id = UIApplication.shared.beginBackgroundTask(withName: "sirtone.work")
        defer { if id != .invalid { UIApplication.shared.endBackgroundTask(id) } }
        return await Task.detached(operation: work).value
    }

    var documents: URL { FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0] }

    /// «Входящие» in Files → On My iPhone → Сиртон.
    var incoming: URL { documents.appendingPathComponent(L.t("folder.incoming")) }

    func open() {
        guard !opened else { return }
        opened = true
        let fm = FileManager.default
        var root = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("SirTone", isDirectory: true)
        try? fm.createDirectory(at: root, withIntermediateDirectories: true)
        var rv = URLResourceValues()
        rv.isExcludedFromBackup = true
        try? root.setResourceValues(rv)
        try? fm.createDirectory(at: incoming, withIntermediateDirectories: true)
        let path = root.path
        Task {
            let made = await Task.detached { () -> (Library, Wave?)? in
                guard let lib = try? Library.open(root: path) else { return nil }
                return (lib, try? Wave.start(library: lib))
            }.value
            if let w = made?.1 { _ = try? w.tune(tuning: tuning) }
            guard let (lib, wave) = made else { return }
            library = lib
            self.wave = wave
            context.refresh()
            await refresh()
            await pickUp()
        }
    }

    /// On every return to the app: packages from the PC and files dropped into «Входящие».
    func becameActive() {
        guard library != nil else { return }
        context.refresh()
        Task { await pickUp() }
    }

    func refresh() async {
        guard let lib = library else { return }
        counts = await Task.detached { try? lib.counts() }.value
        storage = await Task.detached { try? lib.storageInfo() }.value
        revision += 1
    }

    // MARK: Storage (D-146)

    /// «Сделать экономные копии для всего» at `level`, batch by batch with progress.
    func makeCopies(level: String) {
        guard let lib = library, copying == nil else { return }
        let total = Int((storage?.originalOnly ?? 0))
        copying = (0, max(total, 1))
        Task {
            var after: Int64 = 0
            var done = 0
            while true {
                let from = after
                guard let b = await background({
                    try? lib.makeCopies(levelName: level, after: from, limit: 5)
                }) else { break }
                done += Int(b.made + b.failed)
                copying = (done, max(total, done))
                after = b.nextAfter
                if !b.more { break }
            }
            copying = nil
            await refresh()
        }
    }

    /// «Удалить оригиналы, оставить копии».
    func keepCopiesOnly() {
        guard let lib = library else { return }
        Task {
            let f = await Task.detached { try? lib.keepCopiesOnly() }.value
            if let f {
                playback.notice = L.t("storage.freed",
                    ByteCountFormatter.string(fromByteCount: Int64(f.bytes), countStyle: .file))
            }
            await refresh()
        }
    }

    // MARK: «Только музыка» (D-147)

    /// The mode in effect: the switch, or iPhone's Low Power Mode when that is allowed.
    var musicOnly: Bool {
        musicOnlySwitch || (musicOnlyWithLowPower && ProcessInfo.processInfo.isLowPowerModeEnabled)
    }

    // MARK: Import (S09)

    /// Files or folders from the system picker or «Поделиться» (security-scoped URLs).
    func importURLs(_ urls: [URL], quality: String? = nil) {
        guard let lib = library, !urls.isEmpty else { return }
        let level = quality ?? addQuality
        importing = true
        Task {
            let summary = await background { () -> ImportSummary? in
                var granted: [URL] = []
                for u in urls where u.startAccessingSecurityScopedResource() { granted.append(u) }
                defer { granted.forEach { $0.stopAccessingSecurityScopedResource() } }
                return try? lib.importFilesAs(paths: urls.map(\.path), levelName: level)
            }
            importing = false
            report(summary)
            await refresh()
        }
    }

    private func report(_ s: ImportSummary?) {
        guard let s else { playback.notice = L.t("import.failed"); return }
        playback.notice = s.added + s.unknown == 0
            ? L.t("import.nothing")
            : L.t("import.done", Int(s.added), Int(s.unknown))
    }

    /// «Настроить волну»: applies at once — the wave's queued picks are asked again.
    func setTuning(_ t: WaveTuning) {
        tuningStored = t.stored
        objectWillChange.send()
        guard let wave else { return }
        _ = try? wave.tune(tuning: t)
        playback.retune()
    }

    /// The big start: «Моя волна» (back from «Засыпательная», if that was the last one).
    func startWave(profile: String = "my_wave") {
        guard let lib = library else { return }
        context.promptOnce()
        if profile == waveProfile, wave != nil {
            playback.startWave()
            return
        }
        let t = tuning
        Task {
            let w = await Task.detached { () -> Wave? in
                guard let w = try? Wave.startProfile(library: lib, profile: profile) else { return nil }
                if profile == "my_wave" { _ = try? w.tune(tuning: t) }
                return w
            }.value
            guard let w else { return }
            wave = w
            waveProfile = profile
            playback.startWave()
        }
    }

    /// «Сила влияний» (S07): season, context, events, 0…2 (1 — as the profile says).
    var influence: [Float] {
        ["infSeason", "infContext", "infEvents"].map {
            Float(UserDefaults.standard.object(forKey: $0) as? Double ?? 1)
        }
    }

    func setInfluence(_ v: [Float]) {
        for (k, x) in zip(["infSeason", "infContext", "infEvents"], v) {
            UserDefaults.standard.set(Double(x), forKey: k)
        }
        configureWave()
        playback.retune()
    }

    /// A new wave gets the influences and the phone's context again.
    private func configureWave() {
        guard let w = wave else { return }
        let i = influence
        Task.detached { w.setInfluence(season: i[0], context: i[1], events: i[2]) }
        context.reapply()
    }

    /// «Засыпательная волна» (as on the PC): 30 minutes of the sleep timer, then quiet.
    func startSleepWave() {
        playback.sleep(minutes: 30)
        startWave(profile: "sleep")
    }

    /// «Подходит N треков» for the current choice.
    func tuningCount() async -> TuningCount? {
        guard let wave else { return nil }
        let t = tuning
        return await Task.detached { try? wave.tuningCount(tuning: t) }.value
    }

    /// Twice a second while importing: the core counts without its lock, so this never waits.
    private func watchWork() {
        workTask?.cancel()
        guard importing, let lib = library else {
            work = nil
            return
        }
        workTask = Task { [weak self] in
            while !Task.isCancelled {
                let w = lib.workProgress()
                self?.work = w.total > 0 ? w : nil
                try? await Task.sleep(nanoseconds: 500_000_000)
            }
        }
    }

    private func pickUp() async {
        guard let lib = library else { return }
        let fm = FileManager.default
        // Packages from the PC hub.
        let inbox = documents.appendingPathComponent(Self.transferInbox)
        let packages = ((try? fm.contentsOfDirectory(at: inbox, includingPropertiesForKeys: nil)) ?? [])
            .filter { fm.fileExists(atPath: $0.appendingPathComponent(Self.complete).path) }
        var moved = 0
        // The core is busy while it applies a package: the library shows that it is working.
        if !packages.isEmpty { importing = true }
        defer { importing = false }
        for p in packages {
            let r = await Task.detached { try? lib.applyPackage(dir: p.path) }.value
            if let r {
                moved += Int(r.tracks)
                try? fm.removeItem(at: p)
            }
        }
        if moved > 0 { playback.notice = L.count("transfer.done", moved) }
        // Files dropped into «Входящие»: imported, then removed from there (the library keeps
        // its own copy).
        let dropped = (try? fm.contentsOfDirectory(at: incoming, includingPropertiesForKeys: nil)) ?? []
        if !dropped.isEmpty {
            importing = true
            let level = addQuality
            let s = await Task.detached {
                try? lib.importFilesAs(paths: dropped.map(\.path), levelName: level)
            }.value
            importing = false
            if s != nil { dropped.forEach { try? fm.removeItem(at: $0) } }
            report(s)
        }
        if moved > 0 || !dropped.isEmpty {
            if let w = await Task.detached(operation: { try? Wave.start(library: lib) }).value {
                _ = try? w.tune(tuning: tuning)
                wave = w
                waveProfile = "my_wave"
            }
            await refresh()
        }
    }

    // MARK: Theme

    func palette(system: ColorScheme) -> Palette {
        let dark = theme == "dark" || (theme == "system" && system == .dark)
        return Palette.make(dark: dark, accent: accent)
    }

    var preferredScheme: ColorScheme? {
        theme == "dark" ? .dark : theme == "light" ? .light : nil
    }
}
#endif
