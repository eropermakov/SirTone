// S01 «Волна» (FR-MOB-S01, F07): greeting by the time of day, one big start, the
// «Знакомое ↔ Неожиданное» slider, «Продолжить прослушивание» while something is queued.
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI

struct WaveView: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p
    @State private var settings = false
    @State private var tuning = false

    private var greeting: String {
        switch Calendar.current.component(.hour, from: Date()) {
        case 5..<12: L.t("wave.morning")
        case 12..<17: L.t("wave.day")
        case 17..<23: L.t("wave.evening")
        default: L.t("wave.night")
        }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: M.space8) {
                    PageTitle(text: greeting)
                    if (app.counts?.tracks ?? 0) == 0 {
                        EmptyState(title: L.t("wave.empty"))
                    } else {
                        start
                        novelty
                        if let item = playback.current, !playback.playing {
                            resume(item)
                        }
                    }
                }
                .padding(.horizontal, M.space4)
                .padding(.top, M.space2)
            }
            .background(p.bgLayer)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    // S01: the wave's settings icon in the corner; filled while a filter is on.
                    Button { tuning = true } label: {
                        Image(systemName: app.tuning.isEmpty
                              ? "slider.horizontal.3" : "slider.horizontal.below.square.filled.and.square")
                            .foregroundStyle(app.tuning.isEmpty ? p.icon : p.accent.text)
                    }
                    .accessibilityLabel(L.t("tune.title"))
                    .disabled(app.wave == nil)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    // «⋯» next to the settings (D-147): extra functions live here, not on the screen.
                    Menu {
                        Toggle(isOn: $app.musicOnlySwitch) {
                            Label(L.t("musiconly.title"), systemImage: "leaf")
                        }
                        Button { settings = true } label: {
                            Label(L.t("settings.title"), systemImage: "gearshape")
                        }
                    } label: {
                        Image(systemName: app.musicOnly ? "leaf.fill" : "ellipsis.circle")
                            .foregroundStyle(app.musicOnly ? p.accent.text : p.icon)
                    }
                    .accessibilityLabel(L.t("menu.more"))
                }
            }
            .sheet(isPresented: $settings) { SettingsView() }
            .sheet(isPresented: $tuning) { TuneView() }
        }
    }

    /// The brand panel with the screen's only Primary action (DESIGN.md §9 Panel).
    private var start: some View {
        VStack(alignment: .leading, spacing: M.space4) {
            Text(verbatim: L.t("wave.title"))
                .font(.sirtone(Ty.shelfTitle, relativeTo: .title2))
                .foregroundStyle(p.onPrimary)
            Text(verbatim: L.t("wave.subtitle"))
                .font(.sirtone(Ty.body))
                .foregroundStyle(p.onPrimary.opacity(0.8))
            PrimaryButton(
                title: playback.waveOn && playback.playing ? L.t("wave.playing") : L.t("wave.start"),
                icon: "play.fill"
            ) {
                if playback.waveOn && playback.current != nil { playback.toggle() } else { app.startWave() }
            }
            .disabled(app.wave == nil)
        }
        .padding(M.space6)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(p.accent.primary, in: RoundedRectangle(cornerRadius: M.radiusXl, style: .continuous))
    }

    private var novelty: some View {
        VStack(alignment: .leading, spacing: M.space2) {
            Text(verbatim: L.t("wave.novelty"))
                .font(.sirtone(Ty.groupLabel, relativeTo: .subheadline))
                .foregroundStyle(p.textSecondary)
            Slider(value: Binding(
                get: { Double(playback.novelty) },
                set: { playback.novelty = Float($0) }
            ), in: 0...1) {
                Text(verbatim: L.t("wave.novelty"))
            } minimumValueLabel: {
                Text(verbatim: L.t("wave.familiar")).font(.sirtone(Ty.caption, relativeTo: .caption))
            } maximumValueLabel: {
                Text(verbatim: L.t("wave.surprise")).font(.sirtone(Ty.caption, relativeTo: .caption))
            }
            .tint(p.accent.text)
            .foregroundStyle(p.textSecondary)
        }
    }

    private func resume(_ item: Playback.Item) -> some View {
        VStack(alignment: .leading, spacing: M.space3) {
            Text(verbatim: L.t("wave.continue"))
                .font(.sirtone(Ty.shelfTitle, relativeTo: .title3))
                .foregroundStyle(p.accent.primaryText)
            Button { playback.resume() } label: {
                HStack(spacing: M.space3) {
                    TrackArtwork(trackId: item.track.id, size: 56, radius: M.radiusMd)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(verbatim: item.track.title).font(.sirtone(Ty.bodyStrong))
                            .foregroundStyle(p.textPrimary).lineLimit(1)
                        Text(verbatim: item.track.artists).font(.sirtone(Ty.caption, relativeTo: .caption))
                            .foregroundStyle(p.textSecondary).lineLimit(1)
                    }
                    Spacer()
                    Image(systemName: "play.fill").foregroundStyle(p.accent.text)
                }
                .padding(M.space3)
                .background(p.bgRaised, in: RoundedRectangle(cornerRadius: M.radiusXl, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: M.radiusXl, style: .continuous).stroke(p.border))
            }
            .buttonStyle(.plain)
        }
    }
}

/// S04 mini-player above the tabs (64 pt; Play 44 pt): tap opens «Сейчас играет»,
/// a horizontal swipe changes the track.
struct MiniPlayer: View {
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p
    @Binding var expanded: Bool

    var body: some View {
        if let item = playback.current {
            HStack(spacing: M.space3) {
                TrackArtwork(trackId: item.track.id, size: 44)
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: item.track.title).font(.sirtone(Ty.bodyStrong))
                        .foregroundStyle(p.textPrimary).lineLimit(1)
                    Text(verbatim: item.track.artists).font(.sirtone(Ty.caption, relativeTo: .caption))
                        .foregroundStyle(p.textSecondary).lineLimit(1)
                }
                Spacer(minLength: M.space2)
                Button { Haptics.tap(); playback.toggle() } label: {
                    Image(systemName: playback.playing ? "pause.fill" : "play.fill")
                        .font(.system(size: 18, weight: .semibold))
                        .foregroundStyle(p.accent.on)
                        .frame(width: 44, height: 44)
                        .background(p.accent.fill, in: Circle())
                }
                .accessibilityLabel(playback.playing ? L.t("player.pause") : L.t("player.play"))
                Button { playback.next() } label: {
                    Image(systemName: "forward.fill").foregroundStyle(p.icon).frame(width: 44, height: 44)
                }
                .accessibilityLabel(L.t("player.next"))
            }
            .padding(.horizontal, M.space3)
            .frame(height: 64)
            .background(p.bgRaised, in: RoundedRectangle(cornerRadius: M.radiusLg, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: M.radiusLg, style: .continuous).stroke(p.border))
            .padding(.horizontal, M.space2)
            .padding(.bottom, M.space1)
            .contentShape(Rectangle())
            .onTapGesture { expanded = true }
            .gesture(DragGesture(minimumDistance: 30).onEnded { g in
                if g.translation.width < -60 { playback.next() } else if g.translation.width > 60 { playback.previous() }
            })
            .accessibilityElement(children: .contain)
        }
    }
}
#endif
