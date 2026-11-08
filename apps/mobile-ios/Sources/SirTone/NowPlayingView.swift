// S05 «Сейчас играет» and S06 «Очередь»: big cover over a blurred field of its own colours
// (raw cover colours only in the background, DESIGN.md §4), progress, Play 72 pt, ♡,
// «Не сейчас», «Почему этот трек», the queue with «Волна продолжится».
#if canImport(SirToneCore)
import AVKit
import SirToneCore
import SirToneDesign
import SwiftUI

struct NowPlayingView: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var scrubbing: Double?
    @State private var showQueue = false
    @State private var why: WhyList?
    @State private var showViz = false

    var body: some View {
        GeometryReader { geo in
            let side = min(geo.size.width - M.space8 * 2, geo.size.height * 0.45)
            // The blurred cover lies in `.background`: a filled picture there cannot widen the
            // layout (in a ZStack it pushed the player off-screen on non-square covers).
            VStack(spacing: M.space6) {
                    ZStack {
                        Capsule().fill(Color.white.opacity(0.4)).frame(width: 40, height: 5)
                            .accessibilityHidden(true)
                        // F22: the visualiser; not in «Только музыка» (D-147).
                        if !app.musicOnly {
                            HStack {
                                Spacer()
                                Button { showViz = true } label: {
                                    Image(systemName: "waveform").font(.system(size: 20))
                                        .frame(width: 44, height: 44)
                                }
                                .buttonStyle(.plain)
                                .opacity(0.85)
                                .accessibilityLabel(L.t("viz.open"))
                            }
                        }
                    }
                    .padding(.top, M.space2)
                    Spacer(minLength: 0)
                    cover(side)
                    titles
                    progress
                    controls
                    secondary
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, M.space8)
                .padding(.bottom, M.space6)
                .frame(width: geo.size.width, height: geo.size.height)
                .background { background }
        }
        .foregroundStyle(Color.white)
        .gesture(DragGesture().onEnded { g in if g.translation.height > 120 { dismiss() } })
        .fullScreenCover(isPresented: $showViz) { VizView() }
        .sheet(isPresented: $showQueue) { QueueView().presentationDetents([.medium, .large]) }
        // A stored item: one built inside a Binding got a new id on every redraw, and the sheet
        // closed and opened again forever.
        .sheet(item: $why) { w in WhySheet(codes: w.codes).presentationDetents([.fraction(0.3)]) }
    }

    private var background: some View {
        ZStack {
            p.bgBase
            if !app.musicOnly {
                // The waves of the track's colours behind the player, as on the PC (D-148); the
                // blurred cover shows through until the page draws.
                if let img = playback.cover {
                    Color.clear
                        .overlay { Image(uiImage: img).resizable().scaledToFill() }
                        .clipped()
                        .blur(radius: 100)
                        .opacity(0.9)
                }
                VizBackground(paused: showViz)
            }
            p.artworkScrim.ignoresSafeArea()
        }
        .ignoresSafeArea()
    }

    @ViewBuilder private func cover(_ side: CGFloat) -> some View {
        Group {
            if let img = playback.cover {
                Image(uiImage: img).resizable().scaledToFill()
            } else {
                ZStack {
                    p.artworkPlaceholder
                    Image(systemName: "music.note").font(.system(size: side * 0.25))
                        .foregroundStyle(p.textTertiary)
                }
            }
        }
        .frame(width: side, height: side)
        .clipShape(RoundedRectangle(cornerRadius: M.radiusLg, style: .continuous))
        .shadow(color: .black.opacity(0.24), radius: 16, y: 4)
        .scaleEffect(playback.playing || reduceMotion || app.musicOnly ? 1 : 0.9)
        .animation(reduceMotion || app.musicOnly ? nil : .easeOut(duration: 0.25), value: playback.playing)
        .accessibilityHidden(true)
    }

    private var titles: some View {
        HStack(alignment: .center, spacing: M.space3) {
            VStack(alignment: .leading, spacing: 4) {
                Text(verbatim: playback.current?.track.title ?? "")
                    .font(.sirtone(Ty.shelfTitle, relativeTo: .title2)).lineLimit(1)
                Text(verbatim: playback.current?.track.artists ?? "")
                    .font(.sirtone(Ty.body)).opacity(0.7).lineLimit(1)
            }
            Spacer()
            Button { playback.toggleLike() } label: {
                Image(systemName: playback.liked ? "heart.fill" : "heart")
                    .font(.system(size: 22))
                    .foregroundStyle(playback.liked ? p.accent.text : Color.white.opacity(0.8))
                    .frame(width: 44, height: 44)
            }
            .accessibilityLabel(playback.liked ? L.t("player.unlike") : L.t("player.like"))
        }
    }

    private var progress: some View {
        let total = max(Double(playback.durationMs), 1)
        let now = scrubbing ?? Double(playback.positionMs)
        return VStack(spacing: M.space1) {
            Slider(value: Binding(get: { now }, set: { scrubbing = $0 }), in: 0...total) { editing in
                if !editing, let v = scrubbing {
                    playback.seek(to: UInt64(v))
                    scrubbing = nil
                }
            }
            .tint(p.accent.text)
            HStack {
                Text(verbatim: Clock.text(UInt64(now)))
                Spacer()
                Text(verbatim: "-" + Clock.text(UInt64(max(0, total - now))))
            }
            .font(.sirtone(Ty.timecode, relativeTo: .caption2)).monospacedDigit()
            .opacity(0.7)
        }
    }

    private var controls: some View {
        HStack {
            Button { playback.previous() } label: {
                Image(systemName: "backward.fill").font(.system(size: 28)).frame(width: 64, height: 64)
            }
            .accessibilityLabel(L.t("player.previous"))
            Spacer()
            Button { Haptics.tap(); playback.toggle() } label: {
                Image(systemName: playback.playing ? "pause.fill" : "play.fill")
                    .font(.system(size: 30, weight: .semibold))
                    .foregroundStyle(p.accent.on)
                    .frame(width: 72, height: 72)
                    .background(p.accent.fill, in: Circle())
            }
            .accessibilityLabel(playback.playing ? L.t("player.pause") : L.t("player.play"))
            Spacer()
            Button { playback.next() } label: {
                Image(systemName: "forward.fill").font(.system(size: 28)).frame(width: 64, height: 64)
            }
            .accessibilityLabel(L.t("player.next"))
        }
        .buttonStyle(.plain)
    }

    private var secondary: some View {
        HStack {
            if playback.current?.fromWave == true {
                Button { playback.notNow() } label: {
                    Image(systemName: "hand.raised").frame(width: 44, height: 44)
                }
                .accessibilityLabel(L.t("player.not_now"))
                Spacer()
                Button { Task { why = WhyList(codes: await playback.why()) } } label: {
                    Image(systemName: "questionmark.circle").frame(width: 44, height: 44)
                }
                .accessibilityLabel(L.t("player.why"))
                Spacer()
            }
            SleepMenu()
            Spacer()
            RoutePicker().frame(width: 44, height: 44)
            Spacer()
            Button { showQueue = true } label: {
                Image(systemName: "list.bullet").frame(width: 44, height: 44)
            }
            .accessibilityLabel(L.t("player.queue"))
        }
        .font(.system(size: 20))
        .opacity(0.85)
        .buttonStyle(.plain)
    }
}

/// «Сон» (F09): 15–60 minutes; while on — what is left, «+15 мин», cancel.
private struct SleepMenu: View {
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p

    var body: some View {
        Menu {
            if let at = playback.sleepAt {
                Text(verbatim: L.t("sleep.left", Clock.text(UInt64(max(0, at.timeIntervalSinceNow) * 1000))))
                Button(L.t("sleep.later")) { playback.sleepLater() }
                Button(L.t("sleep.cancel"), role: .destructive) { playback.sleepCancel() }
            } else {
                ForEach([15, 30, 45, 60], id: \.self) { m in
                    Button(L.t("sleep.minutes", m)) { playback.sleep(minutes: m) }
                }
            }
        } label: {
            Image(systemName: playback.sleepAt == nil ? "moon" : "moon.fill")
                .foregroundStyle(playback.sleepAt == nil ? Color.white : p.accent.text)
                .frame(width: 44, height: 44)
        }
        .accessibilityLabel(L.t("sleep.title"))
    }
}

/// The system output picker (AirPlay, AirPods): iOS chooses the route (MR §3).
private struct RoutePicker: UIViewRepresentable {
    func makeUIView(context: Context) -> AVRoutePickerView {
        let v = AVRoutePickerView()
        v.tintColor = .white
        return v
    }
    func updateUIView(_ uiView: AVRoutePickerView, context: Context) {}
}

private struct WhyList: Identifiable {
    let id = UUID()
    let codes: [String]
}

/// «Почему этот трек»: one or two plain reasons.
private struct WhySheet: View {
    let codes: [String]
    @Environment(\.palette) private var p

    private func text(_ code: String) -> String {
        let parts = code.split(separator: ":").map(String.init)
        let base = parts.first ?? code
        // The phone's context names itself (D-149): «Вы в пути».
        if base == "auto", parts.count > 1 { return L.t("why.auto.\(parts[1])") }
        return L.t("why.\(base)")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: M.space3) {
            Text(verbatim: L.t("player.why"))
                .font(.sirtone(Ty.cardTitle)).foregroundStyle(p.textPrimary)
            ForEach(codes.isEmpty ? ["none"] : codes, id: \.self) { c in
                Text(verbatim: text(c)).font(.sirtone(Ty.body)).foregroundStyle(p.textSecondary)
            }
            Spacer()
        }
        .padding(M.space6)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(p.bgFlyout)
    }
}

/// S06: what plays next; drag to reorder, swipe to remove; «Волна продолжится».
struct QueueView: View {
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p

    var body: some View {
        NavigationStack {
            List {
                if playback.upNext.isEmpty {
                    Text(verbatim: L.t("queue.empty")).foregroundStyle(p.textSecondary)
                        .listRowBackground(p.bgFlyout)
                }
                ForEach(Array(playback.upNext)) { item in
                    TrackRowView(track: item.track) { playback.jump(to: item) }
                        .listRowBackground(p.bgFlyout)
                }
                .onMove { playback.move(from: $0, to: $1) }
                .onDelete { playback.remove(at: $0) }
                if playback.waveOn {
                    Label(L.t("queue.wave_continues"), systemImage: "water.waves")
                        .font(.sirtone(Ty.caption, relativeTo: .caption))
                        .foregroundStyle(p.accent.text)
                        .listRowBackground(p.bgFlyout)
                }
            }
            .scrollContentBackground(.hidden)
            .background(p.bgFlyout)
            .navigationTitle(L.t("queue.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { EditButton() }
        }
    }
}
#endif
