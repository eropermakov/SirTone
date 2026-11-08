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
            if controls { overlay.transition(.opacity) }
        }
        .background(Color.black)
        .statusBarHidden(!controls)
        .contentShape(Rectangle())
        .onTapGesture {
            withAnimation(reduceMotion ? nil : .easeOut(duration: 0.2)) { controls.toggle() }
        }
        .gesture(DragGesture().onEnded { g in if g.translation.height > 120 { dismiss() } })
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
                Picker(L.t("viz.mode"), selection: $mode) {
                    Text(verbatim: L.t("viz.waves")).tag("waves")
                    Text(verbatim: L.t("viz.milkdrop")).tag("milkdrop")
                }
                .pickerStyle(.segmented)
                .frame(maxWidth: 220)
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

    override init() {
        let cfg = WKWebViewConfiguration()
        cfg.mediaTypesRequiringUserActionForPlayback = .all
        web = WKWebView(frame: .zero, configuration: cfg)
        web.isOpaque = false
        web.backgroundColor = .black
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
                   playback: Playback, library: Library?) {
        self.playback = playback
        self.library = library
        start = ["mode": mode, "accent": accent, "reduced": reduced, "level": level]
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
        js("sirtoneViz.frame('\(frame.base64EncodedString())')")
    }

    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        switch m.name {
        case "ready":
            ready = true
            begin()
        case "failed":
            failed = true
        case "stats":
            if let d = m.body as? [String: Any], let mode = d["mode"] as? String {
                stats[mode] = d
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
        try? data.write(to: dir.appendingPathComponent("viz-stats.json"))
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
