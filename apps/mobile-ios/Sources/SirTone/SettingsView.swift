// S10 «Настройки» (F10): theme, the colour (Sir.kov + 16, DESIGN.md §9 AccentPicker — applies at
// once, no «Save»), storage, the PC hub, about. S11 «Срок работы» joins «ПК-Сиртон» after П3.
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    @Environment(\.dismiss) private var dismiss
    @State private var askLevel = false
    @State private var confirmTrash = false
    @State private var trash: UInt64 = 0
    @State private var confirmKeep = false

    private static func size(_ b: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(b), countStyle: .file)
    }

    private var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "—"
    }

    var body: some View {
        NavigationStack {
            List {
                // The PC's sections (D-148): sound, the wave, the context.
                Section {
                    NavigationLink { SoundSettingsView() } label: {
                        Label(L.t("sound.title"), systemImage: "slider.vertical.3")
                    }
                    NavigationLink { WaveSettingsView() } label: {
                        Label(L.t("wavecfg.title"), systemImage: "waveform")
                    }
                    NavigationLink { ContextSettingsView(ctx: app.context) } label: {
                        Label(L.t("ctx.title"), systemImage: "location")
                    }
                }
                .listRowBackground(p.bgRaised)
                Section(L.t("settings.appearance")) {
                    Picker(L.t("settings.theme"), selection: $app.theme) {
                        Text(verbatim: L.t("settings.theme.system")).tag("system")
                        Text(verbatim: L.t("settings.theme.dark")).tag("dark")
                        Text(verbatim: L.t("settings.theme.light")).tag("light")
                    }
                    .pickerStyle(.segmented)
                    .listRowBackground(p.bgRaised)
                    VStack(alignment: .leading, spacing: M.space3) {
                        Text(verbatim: L.t("settings.color")).font(.sirtone(Ty.body))
                        AccentPicker(selected: $app.accent)
                    }
                    .padding(.vertical, M.space2)
                    .listRowBackground(p.bgRaised)
                }
                Section(L.t("musiconly.title")) {
                    Toggle(L.t("musiconly.title"), isOn: $app.musicOnlySwitch)
                        .tint(p.accent.fill)
                        .listRowBackground(p.bgRaised)
                    Toggle(L.t("musiconly.lowpower"), isOn: $app.musicOnlyWithLowPower)
                        .tint(p.accent.fill)
                        .listRowBackground(p.bgRaised)
                    Text(verbatim: L.t("musiconly.hint"))
                        .font(.sirtone(Ty.caption, relativeTo: .caption)).foregroundStyle(p.textSecondary)
                        .listRowBackground(p.bgRaised)
                }
                Section(L.t("settings.storage")) {
                    if let c = app.counts {
                        Text(verbatim: L.t("storage.tracks", Int(c.tracks))).listRowBackground(p.bgRaised)
                    }
                    if let s = app.storage {
                        Text(verbatim: L.t("storage.originals", Self.size(s.originalBytes)))
                            .listRowBackground(p.bgRaised)
                        Text(verbatim: L.t("storage.copies", Self.size(s.copyBytes)))
                            .listRowBackground(p.bgRaised)
                    }
                    // Deleted music waits in the app's trash until emptied here (D-165).
                    if trash > 0 {
                        Text(verbatim: L.t("storage.trash", Self.size(trash))).listRowBackground(p.bgRaised)
                        Button(L.t("storage.empty_trash"), role: .destructive) { confirmTrash = true }
                            .listRowBackground(p.bgRaised)
                    }
                    Picker(L.t("quality.setting"), selection: $app.addQuality) {
                        ForEach(["original", "economy", "max"], id: \.self) { q in
                            Text(verbatim: L.t("quality.short.\(q)")).tag(q)
                        }
                    }
                    .listRowBackground(p.bgRaised)
                    if let c = app.copying {
                        ProgressView(value: Double(c.done), total: Double(max(c.total, 1))) {
                            Text(verbatim: L.t("storage.making", c.done, c.total))
                                .font(.sirtone(Ty.caption, relativeTo: .caption))
                        }
                        .tint(p.accent.text)
                        .listRowBackground(p.bgRaised)
                    } else {
                        Button(L.t("storage.make")) { askLevel = true }
                            .foregroundStyle(p.accent.text)
                            .disabled((app.storage?.originalOnly ?? 0) == 0)
                            .listRowBackground(p.bgRaised)
                        Button(L.t("storage.keep"), role: .destructive) { confirmKeep = true }
                            .disabled((app.storage?.both ?? 0) == 0)
                            .listRowBackground(p.bgRaised)
                    }
                }
                Section(L.t("settings.pc")) {
                    Text(verbatim: L.t("settings.pc.hint"))
                        .font(.sirtone(Ty.caption, relativeTo: .caption)).foregroundStyle(p.textSecondary)
                        .listRowBackground(p.bgRaised)
                }
                Section(L.t("settings.about")) {
                    Text(verbatim: L.t("settings.about.line")).listRowBackground(p.bgRaised)
                    Text(verbatim: L.t("settings.version", version))
                        .foregroundStyle(p.textSecondary).listRowBackground(p.bgRaised)
                }
            }
            .font(.sirtone(Ty.body))
            .foregroundStyle(p.textPrimary)
            .scrollContentBackground(.hidden)
            .background(p.bgLayer)
            .confirmationDialog(L.t("storage.make"), isPresented: $askLevel, titleVisibility: .visible) {
                Button(L.t("quality.economy")) { app.makeCopies(level: "economy") }
                Button(L.t("quality.max")) { app.makeCopies(level: "max") }
                Button(L.t("library.cancel"), role: .cancel) {}
            }
            .confirmationDialog(L.t("storage.empty_trash"), isPresented: $confirmTrash, titleVisibility: .visible) {
                Button(L.t("storage.empty_trash"), role: .destructive) {
                    guard let lib = app.library else { return }
                    Task {
                        let n = await Task.detached { (try? lib.emptyTrash()) ?? 0 }.value
                        app.playback.notice = L.t("storage.freed", Self.size(n))
                        trash = 0
                    }
                }
                Button(L.t("library.cancel"), role: .cancel) {}
            } message: {
                Text(verbatim: L.t("storage.empty_trash.confirm"))
            }
            .task(id: app.revision) {
                guard let lib = app.library else { return }
                trash = await Task.detached { lib.trashBytes() }.value
            }
            .confirmationDialog(L.t("storage.keep"), isPresented: $confirmKeep, titleVisibility: .visible) {
                Button(L.t("storage.keep"), role: .destructive) { app.keepCopiesOnly() }
                Button(L.t("library.cancel"), role: .cancel) {}
            } message: {
                Text(verbatim: L.t("storage.keep.confirm"))
            }
            .navigationTitle(L.t("settings.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(L.t("settings.done")) { dismiss() }.foregroundStyle(p.accent.text)
                }
            }
        }
    }
}

/// 17 circles 32 pt (44 pt touch): the selected one has a 2 pt ring with a 3 pt gap.
struct AccentPicker: View {
    @Binding var selected: String
    @Environment(\.palette) private var p

    private let cols = Array(repeating: GridItem(.fixed(44), spacing: M.space2), count: 6)

    var body: some View {
        LazyVGrid(columns: cols, alignment: .leading, spacing: M.space2) {
            ForEach(Palette.accentOrder, id: \.self) { slug in
                let pair = SirToneTokens.accents[slug]!
                let a = p.dark ? pair.dark : pair.light
                Button {
                    Haptics.tap()
                    withAnimation(.easeInOut(duration: 0.4)) { selected = slug }
                } label: {
                    ZStack {
                        Circle().fill(a.fill).frame(width: 32, height: 32)
                        if selected == slug {
                            Circle().stroke(p.textPrimary, lineWidth: 2).frame(width: 42, height: 42)
                            Image(systemName: "checkmark").font(.system(size: 12, weight: .bold))
                                .foregroundStyle(slug == "sirkov" ? p.onPrimary : a.on)
                        }
                    }
                    .frame(width: 44, height: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(L.t("color.\(slug)"))
                .accessibilityAddTraits(selected == slug ? .isSelected : [])
            }
        }
    }
}
#endif
