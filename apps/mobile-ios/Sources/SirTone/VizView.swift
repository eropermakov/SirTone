// F22 «Визуализация» (D-148): full screen from «Сейчас играет». The PC's «Волны» and MilkDrop
// pages run unchanged in a WKWebView (`apps/desktop/src/mobile-viz`, built into Resources/viz);
// the core's visualiser feed gives the PC's S-04 frames, pushed on every display refresh and
// drawn by the page on requestAnimationFrame. Window end = what is heard now: the system's output
// latency is passed in (AirPods ≈ 200 ms). Closed, in the background or in «Только музыка» —
// no feed, no copying on the audio thread.
#if canImport(SirToneCore)
import AVFoundation
import SirToneCore
import SirToneDesign
import SwiftUI
import WebKit

struct VizView: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var phase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @AppStorage("vizMode") private var mode = "waves"
    @State private var controls = true
    @StateObject private var bridge = VizBridge()

    var body: some View {
        ZStack(alignment: .bottom) {
            VizWeb(bridge: bridge).ignoresSafeArea()
            // Taps and the swipe down live on their own layer under the controls: on the whole
            // screen they took the mode buttons' taps (MilkDrop did not switch, 2026-11-08).
            Color.clear
                .contentShape(Rectangle())
                .ignoresSafeArea()
                .onTapGesture {
                    withAnimation(reduceMotion ? nil : .easeOut(duration: 0.2)) { controls.toggle() }
                }
                .gesture(DragGesture().onEnded { g in if g.translation.height > 120 { dismiss() } })
            if controls { overlay.transition(.opacity) }
        }
        .background(Color.black)
        .statusBarHidden(!controls)
        .onAppear { configure() }
        .onChange(of: mode) { _, _ in configure() }
        .onChange(of: playback.current?.track.id) { _, _ in bridge.sendColors() }
        // A transition swapped the decks: the feed follows the one now playing.
        .onChange(of: playback.deckGen) { _, _ in bridge.halt(); bridge.run() }
        .onChange(of: phase) { _, now in now == .active ? bridge.run() : bridge.halt() }
        .onDisappear { bridge.halt() }
        .task {
            // The controls step aside after a moment, as a video player's.
            try? await Task.sleep(for: .seconds(3))
            withAnimation(reduceMotion ? nil : .easeOut(duration: 0.3)) { controls = false }
        }
    }

    private func configure() {
        let low = ProcessInfo.processInfo.isLowPowerModeEnabled
        bridge.configure(
            mode: mode, accent: p.accent.fill.hex, reduced: reduceMotion, level: low ? 1 : 3,
            playback: playback, library: app.library)
    }

    private var overlay: some View {
        VStack(spacing: M.space4) {
            HStack {
                Button { dismiss() } label: {
                    Image(systemName: "chevron.down").frame(width: 44, height: 44)
                }
                .accessibilityLabel(L.t("viz.close"))
                Spacer()
                HStack(spacing: 0) {
                    ForEach(["waves", "milkdrop"], id: \.self) { m in
                        Button { mode = m } label: {
                            Text(verbatim: L.t("viz.\(m)"))
                                .font(.sirtone(Ty.menu))
                                .foregroundStyle(mode == m ? Color.black : Color.white)
                                .padding(.horizontal, M.space4)
                                .frame(height: 36)
                                .background(mode == m ? Color.white : Color.clear, in: Capsule())
                                .contentShape(Capsule())
                        }
                        .buttonStyle(.plain)
                        .accessibilityAddTraits(mode == m ? .isSelected : [])
                    }
                }
                .padding(3)
                .background(Color.white.opacity(0.18), in: Capsule())
                .accessibilityLabel(L.t("viz.mode"))
                Spacer()
                Color.clear.frame(width: 44, height: 44)
            }
            Spacer()
            if bridge.failed {
                Text(verbatim: L.t("viz.failed")).font(.sirtone(Ty.body)).opacity(0.8)
            }
            VStack(spacing: 4) {
                Text(verbatim: playback.current?.track.title ?? "")
                    .font(.sirtone(Ty.cardTitle)).lineLimit(1)
                Text(verbatim: playback.current?.track.artists ?? "")
                    .font(.sirtone(Ty.body)).opacity(0.7).lineLimit(1)
            }
            HStack(spacing: M.space8) {
                Button { playback.previous() } label: {
                    Image(systemName: "backward.fill").font(.system(size: 24)).frame(width: 56, height: 56)
                }
                .accessibilityLabel(L.t("player.previous"))
                Button { Haptics.tap(); playback.toggle() } label: {
                    Image(systemName: playback.playing ? "pause.fill" : "play.fill")
                        .font(.system(size: 26, weight: .semibold))
                        .foregroundStyle(p.accent.on)
                        .frame(width: 64, height: 64)
                        .background(p.accent.fill, in: Circle())
                }
                .accessibilityLabel(playback.playing ? L.t("player.pause") : L.t("player.play"))
                Button { playback.next() } label: {
                    Image(systemName: "forward.fill").font(.system(size: 24)).frame(width: 56, height: 56)
                }
                .accessibilityLabel(L.t("player.next"))
            }
            .buttonStyle(.plain)
        }
        .foregroundStyle(Color.white)
        .padding(.horizontal, M.space6)
        .padding(.vertical, M.space4)
        .background(
            LinearGradient(colors: [.black.opacity(0.5), .clear, .clear, .black.opacity(0.6)],
                           startPoint: .top, endPoint: .bottom)
                .ignoresSafeArea()
                .allowsHitTesting(false))
    }
}

/// Owns the page, the core's feed and the display link.
@MainActor
final class VizBridge: NSObject, ObservableObject, WKScriptMessageHandler {
    @Published private(set) var failed = false
    let web: WKWebView
    private var feed: VizFeed?
    private var link: CADisplayLink?
    private var ready = false
    private var start: [String: Any] = [:]
    private weak var playback: Playback?
    private var library: Library?
    private var stats: [String: Any] = [:]
    private var mode = "waves"
    private var role = "full"

    override init() {
        let cfg = WKWebViewConfiguration()
        cfg.mediaTypesRequiringUserActionForPlayback = .all
        web = WKWebView(frame: .zero, configuration: cfg)
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.backgroundColor = .clear
        web.scrollView.isScrollEnabled = false
        web.isUserInteractionEnabled = false
        super.init()
        for name in ["ready", "stats", "failed"] {
            cfg.userContentController.add(WeakHandler(self), name: name)
        }
        if let page = Bundle.module.url(forResource: "index", withExtension: "html", subdirectory: "viz") {
            web.loadFileURL(page, allowingReadAccessTo: page.deletingLastPathComponent())
        } else {
            failed = true
        }
    }

    func configure(mode: String, accent: String, reduced: Bool, level: Int,
                   playback: Playback, library: Library?, role: String = "full") {
        self.playback = playback
        self.library = library
        self.mode = mode
        self.role = role
        start = ["mode": mode, "accent": accent, "reduced": reduced, "level": level, "role": role,
                 "lowPower": ProcessInfo.processInfo.isLowPowerModeEnabled]
        failed = false
        if ready { begin() }
        run()
    }

    private func begin() {
        guard let json = try? JSONSerialization.data(withJSONObject: start),
              let s = String(data: json, encoding: .utf8) else { return }
        js("sirtoneViz.start(\(s))")
        sendColors()
    }

    /// Fire-and-forget into the page (the async overload would throw inside tasks).
    private func js(_ code: String) {
        web.evaluateJavaScript(code, completionHandler: nil)
    }

    func sendColors() {
        guard ready, let lib = library, let id = playback?.current?.track.id else { return }
        Task {
            let json = await Task.detached {
                lib.vizColors(trackId: id, cover: lib.trackCover(trackId: id))
            }.value
            js("sirtoneViz.colors(\(json))")
        }
    }

    /// Starts the feed and the display link (60 Hz; ProMotion is not needed for this).
    func run() {
        guard let playback, link == nil else { return }
        feed = playback.vizFeed()
        js("window.sirtoneViz && sirtoneViz.resume()")
        let l = CADisplayLink(target: self, selector: #selector(tick))
        l.preferredFrameRateRange = CAFrameRateRange(minimum: 30, maximum: 60, preferred: 60)
        l.add(to: .main, forMode: .common)
        link = l
    }

    /// Stops pushing and releases the feed (the audio thread stops copying).
    func halt() {
        link?.invalidate()
        link = nil
        feed = nil
        js("window.sirtoneViz && sirtoneViz.pause()")
        saveStats()
    }

    @objc private func tick() {
        guard ready, let feed else { return }
        let s = AVAudioSession.sharedInstance()
        let latency = UInt32(max(0, (s.outputLatency + s.ioBufferDuration) * 1000))
        let frame = feed.frame(latencyMs: latency)
        // «Волны» read only the header (bands, beat): seven numbers instead of 4 KB of samples.
        if mode == "waves", frame.count >= 30 {
            let f = { (o: Int) in
                Float(bitPattern: UInt32(frame[o]) | UInt32(frame[o + 1]) << 8
                    | UInt32(frame[o + 2]) << 16 | UInt32(frame[o + 3]) << 24)
            }
            var t: UInt64 = 0
            for i in 0..<8 { t |= UInt64(frame[2 + i]) << (8 * UInt64(i)) }
            let playing = frame[1] & 1 == 1
            js("sirtoneViz.bands(\(playing),\(t),\(f(10)),\(f(14)),\(f(18)),\(f(22)),\(f(26)))")
        } else {
            js("sirtoneViz.frame('\(frame.base64EncodedString())')")
        }
    }

    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        switch m.name {
        case "ready":
            ready = true
            begin()
        case "failed":
            failed = true
            stats["error"] = m.body
            saveStats()
        case "stats":
            if let d = m.body as? [String: Any], let mode = d["mode"] as? String {
                stats[mode] = d
                saveStats()
            }
        default:
            break
        }
    }

    /// The spike's numbers (D-148): FPS and the worst frame per mode, read by the PC over the
    /// app's folder (`phone pull-library`).
    private func saveStats() {
        guard !stats.isEmpty,
              let dir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first,
              let data = try? JSONSerialization.data(withJSONObject: stats, options: [.prettyPrinted])
        else { return }
        try? data.write(to: dir.appendingPathComponent("viz-stats-\(role).json"))
    }
}

/// WKUserContentController holds its handlers strongly: a weak hop avoids a cycle.
@MainActor
private final class WeakHandler: NSObject, WKScriptMessageHandler {
    weak var target: WKScriptMessageHandler?
    init(_ t: WKScriptMessageHandler) { target = t }
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        target?.userContentController(c, didReceive: m)
    }
}

/// «Волны» behind «Сейчас играет» (as the PC's player over its waves, D-148): the same page, a
/// lighter level, under the artwork scrim; paused while the full-screen visualiser is open.
struct VizBackground: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p
    @Environment(\.scenePhase) private var phase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let paused: Bool
    @StateObject private var bridge = VizBridge()

    var body: some View {
        VizWeb(bridge: bridge)
            .onAppear {
                // Under the scrim: the lightest level is enough.
                bridge.configure(mode: "waves", accent: p.accent.fill.hex, reduced: reduceMotion,
                                 level: 1, playback: playback, library: app.library, role: "bg")
                if paused { bridge.halt() }
            }
            .onChange(of: playback.current?.track.id) { _, _ in bridge.sendColors() }
            .onChange(of: playback.deckGen) { _, _ in if !paused { bridge.halt(); bridge.run() } }
            .onChange(of: paused) { _, now in now ? bridge.halt() : bridge.run() }
            .onChange(of: phase) { _, now in now == .active && !paused ? bridge.run() : bridge.halt() }
            .onDisappear { bridge.halt() }
    }
}

private struct VizWeb: UIViewRepresentable {
    let bridge: VizBridge
    func makeUIView(context: Context) -> WKWebView { bridge.web }
    func updateUIView(_ uiView: WKWebView, context: Context) {}
}

extension Color {
    /// `#rrggbb` for the page (the accent the «Волны» palette mixes in).
    var hex: String {
        var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0, a: CGFloat = 0
        UIColor(self).getRed(&r, green: &g, blue: &b, alpha: &a)
        return String(format: "#%02x%02x%02x", Int(r * 255), Int(g * 255), Int(b * 255))
    }
}
#endif
