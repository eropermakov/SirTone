// Check screens П1 and П2 (docs/mobile/checks.md): the Rust core's answer, then the player over the
// files in the app's folder. Real screens (DESIGN, S01–S11) come in М3.
import SwiftUI
import SirToneDesign
#if canImport(SirToneCore)
import SirToneCore
#endif

@main
struct SirToneApp: App {
    var body: some Scene {
        WindowGroup { CheckView() }
    }
}

struct CheckView: View {
    @State private var answer = ""
    #if canImport(SirToneCore)
    @StateObject private var player = PhonePlayer()
    @Environment(\.scenePhase) private var phase
    #endif

    var body: some View {
        VStack(alignment: .leading, spacing: SirToneTokens.Metric.space4) {
            Text(verbatim: "Сиртон")
                .font(.system(size: SirToneTokens.Typography.pageTitle.size, weight: .bold))
            Text(verbatim: answer)
                .font(.system(size: SirToneTokens.Typography.body.size))
                .foregroundStyle(SirToneTokens.Dark.textSecondary)
            #if canImport(SirToneCore)
            PlayerCheck(player: player)
            #endif
        }
        .padding(SirToneTokens.Metric.space4)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(SirToneTokens.Dark.bgBase)
        .foregroundStyle(SirToneTokens.Dark.textPrimary)
        .task { answer = await Self.ask() }
        #if canImport(SirToneCore)
        .onChange(of: phase) { _, p in if p == .active { player.reload() } }
        .onAppear { player.reload() }
        #endif
    }

    /// Off the main actor: the core is never called from the UI thread (docs/mobile/README.md §7).
    private static func ask() async -> String {
        await Task.detached {
            #if canImport(SirToneCore)
            let info = coreInfo()
            return "Rust \(info.version) · \(info.target) · \(normText(text: "Ёлка — Live") ?? "")"
            #else
            return "core not linked"
            #endif
        }.value
    }
}

#if canImport(SirToneCore)
struct PlayerCheck: View {
    @ObservedObject var player: PhonePlayer

    var body: some View {
        VStack(alignment: .leading, spacing: SirToneTokens.Metric.space3) {
            if player.files.isEmpty {
                Text(verbatim: "Нет файлов. Положите FLAC или Opus: Файлы → На iPhone → Сиртон.")
                    .foregroundStyle(SirToneTokens.Dark.textSecondary)
            } else {
                List(player.files.indices, id: \.self) { i in
                    Button { player.play(at: i) } label: {
                        Text(verbatim: player.files[i].lastPathComponent)
                            .lineLimit(1)
                            .fontWeight(player.index == i ? .bold : .regular)
                    }
                    .listRowBackground(SirToneTokens.Dark.bgBase)
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
            }
            if let name = player.failed {
                Text(verbatim: "Не открывается: \(name)")
                    .foregroundStyle(SirToneTokens.Dark.textSecondary)
            }
            Text(verbatim: status)
                .font(.system(size: SirToneTokens.Typography.body.size).monospacedDigit())
                .foregroundStyle(SirToneTokens.Dark.textSecondary)
            HStack(spacing: SirToneTokens.Metric.space6) {
                Button { player.previous() } label: { Image(systemName: "backward.fill") }
                Button { player.toggle() } label: {
                    Image(systemName: player.playing ? "pause.fill" : "play.fill")
                }
                Button { player.next() } label: { Image(systemName: "forward.fill") }
            }
            .font(.system(size: SirToneTokens.Typography.pageTitle.size))
            .frame(maxWidth: .infinity)
            .disabled(player.files.isEmpty)
        }
    }

    private var status: String {
        let pos = Self.clock(player.positionMs)
        guard let f = player.format else { return pos }
        let total = f.durationMs.map(Self.clock) ?? "—"
        return "\(pos) / \(total) · \(f.codec) · \(f.sampleRate) Гц · \(f.channels) кан."
    }

    private static func clock(_ ms: UInt64) -> String {
        let s = ms / 1000
        return String(format: "%d:%02d", s / 60, s % 60)
    }
}
#endif
