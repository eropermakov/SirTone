// The wave tab's extras (D-148, D-149, D-150): «Мои волны» — the PC's profiles one tap away,
// the context card («Похоже, вы в пути — волна под дорогу» with «Отменить», as the PC's hint by
// the active app) and the one-time question «Это дом?» / «Это работа?».
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI

/// «Мои волны» (S01): Моя волна, Сезонная, Для всех, Засыпательная.
struct MyWaves: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p

    private static let all: [(profile: String, icon: String)] = [
        ("my_wave", "waveform"), ("seasonal", "leaf"), ("for_all", "person.2"), ("sleep", "moon"),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: M.space2) {
            Text(verbatim: L.t("waves.title"))
                .font(.sirtone(Ty.groupLabel, relativeTo: .subheadline))
                .foregroundStyle(p.textSecondary)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: M.space2) {
                    ForEach(Self.all, id: \.profile) { w in
                        let on = app.waveProfile == w.profile && app.playback.waveOn
                        Button {
                            Haptics.tap()
                            if w.profile == "sleep" { app.startSleepWave() } else { app.startWave(profile: w.profile) }
                        } label: {
                            HStack(spacing: M.space15) {
                                Image(systemName: w.icon)
                                Text(verbatim: L.t("waves.\(w.profile)")).lineLimit(1)
                            }
                            .font(.sirtone(Ty.menu))
                            .padding(.horizontal, M.space3)
                            .frame(minHeight: M.chipHeight)
                            .foregroundStyle(on ? p.onPrimary : p.textPrimary)
                            .background(on ? p.accent.primary : Color.clear, in: Capsule())
                            .overlay(Capsule().stroke(on ? Color.clear : p.border, lineWidth: 1))
                            .contentShape(Capsule())
                        }
                        .buttonStyle(.plain)
                        .disabled(app.library == nil)
                        .accessibilityAddTraits(on ? .isSelected : [])
                    }
                }
            }
        }
    }
}

/// The context card and the place question; nothing when there is nothing to say.
struct ContextCards: View {
    @ObservedObject var ctx: ContextService
    @Environment(\.palette) private var p

    var body: some View {
        VStack(spacing: M.space3) {
            if let kind = ctx.ask {
                card(icon: kind == "home" ? "house" : "briefcase", text: L.t("ctx.ask.\(kind)")) {
                    Button(L.t("ctx.no")) { ctx.answer(false) }.foregroundStyle(p.textSecondary)
                    Button(L.t("ctx.yes")) { ctx.answer(true) }.foregroundStyle(p.accent.text)
                }
            }
            if let a = ctx.shown {
                card(icon: a == "road_long" ? "car" : "location.north", text: L.t("ctx.now.\(a)")) {
                    Button(L.t("ctx.undo")) { ctx.dismiss() }.foregroundStyle(p.accent.text)
                }
            }
        }
    }

    private func card(icon: String, text: String,
                      @ViewBuilder actions: () -> some View) -> some View {
        HStack(spacing: M.space3) {
            Image(systemName: icon).foregroundStyle(p.icon)
            Text(verbatim: text).font(.sirtone(Ty.body)).foregroundStyle(p.textPrimary)
            Spacer(minLength: M.space2)
            actions().font(.sirtone(Ty.bodyStrong)).buttonStyle(.plain)
        }
        .padding(M.space3)
        .background(p.bgRaised, in: RoundedRectangle(cornerRadius: M.radiusLg, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: M.radiusLg, style: .continuous).stroke(p.border))
    }
}
#endif
