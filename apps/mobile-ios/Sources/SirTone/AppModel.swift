// The app's state (M4.1, MR §5, §11): the library in Application Support (excluded from iCloud
// backup), the wave over it, imports from the picker, «Поделиться» and «Входящие», and packages
// the PC hub left in Documents/Transfer/Inbox (applied when the app opens; `.complete` marks a
// whole package). The core is called off the main thread only (MR §7).
#if canImport(SirToneCore)
import Foundation
import SirToneCore
import SirToneDesign
import SwiftUI

@MainActor
final class AppModel: ObservableObject {
    @Published private(set) var library: Library?
    @Published private(set) var wave: Wave?
    @Published private(set) var counts: LibraryCounts?
    /// Bumped after anything changes the library: lists reload.
    @Published private(set) var revision = 0
    @Published private(set) var importing = false

    let playback = Playback()

    /// Theme: "system" | "dark" | "light"; colour: an accent slug (DESIGN.md §4).
    @AppStorage("theme") var theme = "system"
    @AppStorage("accent") var accent = "sirkov"

    /// Inbox folder names the hub and the app agree on (`sirtone-devicelink::appfiles`).
    static let transferInbox = "Transfer/Inbox"
    static let complete = ".complete"

    private var opened = false

    init() { playback.attach(model: self) }

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
            guard let (lib, wave) = made else { return }
            library = lib
            self.wave = wave
            await refresh()
            await pickUp()
        }
    }

    /// On every return to the app: packages from the PC and files dropped into «Входящие».
    func becameActive() {
        guard library != nil else { return }
        Task { await pickUp() }
    }

    func refresh() async {
        guard let lib = library else { return }
        counts = await Task.detached { try? lib.counts() }.value
        revision += 1
    }

    // MARK: Import (S09)

    /// Files or folders from the system picker or «Поделиться» (security-scoped URLs).
    func importURLs(_ urls: [URL]) {
        guard let lib = library, !urls.isEmpty else { return }
        importing = true
        Task {
            let summary = await Task.detached { () -> ImportSummary? in
                var granted: [URL] = []
                for u in urls where u.startAccessingSecurityScopedResource() { granted.append(u) }
                defer { granted.forEach { $0.stopAccessingSecurityScopedResource() } }
                return try? lib.importFiles(paths: urls.map(\.path))
            }.value
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

    private func pickUp() async {
        guard let lib = library else { return }
        let fm = FileManager.default
        // Packages from the PC hub.
        let inbox = documents.appendingPathComponent(Self.transferInbox)
        let packages = ((try? fm.contentsOfDirectory(at: inbox, includingPropertiesForKeys: nil)) ?? [])
            .filter { fm.fileExists(atPath: $0.appendingPathComponent(Self.complete).path) }
        var moved = 0
        for p in packages {
            let r = await Task.detached { try? lib.applyPackage(dir: p.path) }.value
            if let r {
                moved += Int(r.tracks)
                try? fm.removeItem(at: p)
            }
        }
        if moved > 0 { playback.notice = L.t("transfer.done", moved) }
        // Files dropped into «Входящие»: imported, then removed from there (the library keeps
        // its own copy).
        let dropped = (try? fm.contentsOfDirectory(at: incoming, includingPropertiesForKeys: nil)) ?? []
        if !dropped.isEmpty {
            importing = true
            let s = await Task.detached { try? lib.importFiles(paths: dropped.map(\.path)) }.value
            importing = false
            if s != nil { dropped.forEach { try? fm.removeItem(at: $0) } }
            report(s)
        }
        if moved > 0 || !dropped.isEmpty {
            if let w = await Task.detached(operation: { try? Wave.start(library: lib) }).value {
                wave = w
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
