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

    private var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "—"
    }

    var body: some View {
        NavigationStack {
            List {
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
                Section(L.t("settings.storage")) {
                    if let c = app.counts {
                        Text(verbatim: L.t("storage.tracks", Int(c.tracks))).listRowBackground(p.bgRaised)
                        Text(verbatim: L.t("storage.size",
                                           ByteCountFormatter.string(fromByteCount: Int64(c.bytes), countStyle: .file)))
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
                        if slug == "sirkov" {
                            Circle().fill(a.primary)
                                .overlay(Circle().trim(from: 0, to: 0.5).fill(a.fill).rotationEffect(.degrees(-45)))
                                .frame(width: 32, height: 32)
                        } else {
                            Circle().fill(a.fill).frame(width: 32, height: 32)
                        }
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
