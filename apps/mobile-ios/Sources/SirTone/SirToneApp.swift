// П1 screen (docs/mobile/checks.md): shows the answer of the Rust core. Real screens come in М3.
import SwiftUI
import SirToneDesign
#if canImport(SirToneCore)
import SirToneCore
#endif

@main
struct SirToneApp: App {
    var body: some Scene {
        WindowGroup { CoreCheckView() }
    }
}

struct CoreCheckView: View {
    @State private var answer = ""

    var body: some View {
        VStack(spacing: SirToneTokens.Metric.space4) {
            Text(verbatim: "Сиртон")
                .font(.system(size: SirToneTokens.Typography.pageTitle.size, weight: .bold))
            Text(verbatim: answer)
                .font(.system(size: SirToneTokens.Typography.body.size))
                .foregroundStyle(SirToneTokens.Dark.textSecondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(SirToneTokens.Dark.bgBase)
        .foregroundStyle(SirToneTokens.Dark.textPrimary)
        .task { answer = await Self.ask() }
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
