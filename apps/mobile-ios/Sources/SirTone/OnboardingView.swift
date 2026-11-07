// First run (MR §5, FR-MOB-ONB): three steps — hello from Sir.kov Team; theme and colour
// (skippable, defaults: system theme and Sir.kov); add music, or after a transfer from the PC
// the summary and «Слушать». Permissions are never asked here.
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI

struct OnboardingView: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p
    @AppStorage("onboarded") private var onboarded = false
    @State private var step = 0
    @State private var picking = false
    @State private var asking = false
    @State private var quality = "original"

    var body: some View {
        VStack(alignment: .leading, spacing: M.space6) {
            Text(verbatim: "\(step + 1) / 3")
                .font(.sirtone(Ty.caption, relativeTo: .caption)).monospacedDigit()
                .foregroundStyle(p.textTertiary)
            Spacer(minLength: 0)
            switch step {
            case 0: hello
            case 1: look
            default: music
            }
            Spacer(minLength: 0)
        }
        .padding(M.space6)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .background(p.bgLayer)
        .fileImporter(isPresented: $picking, allowedContentTypes: [.audio, .folder],
                      allowsMultipleSelection: true) { r in
            if case let .success(urls) = r { app.importURLs(urls, quality: quality) }
        }
        .qualityDialog(isPresented: $asking) { q in
            quality = q
            picking = true
        }
    }

    private var hello: some View {
        VStack(alignment: .leading, spacing: M.space4) {
            PageTitle(text: L.t("onb.hello"))
            Text(verbatim: L.t("onb.hello.text")).font(.sirtone(Ty.body)).foregroundStyle(p.textSecondary)
            PrimaryButton(title: L.t("onb.next")) { step = 1 }
        }
    }

    private var look: some View {
        VStack(alignment: .leading, spacing: M.space4) {
            PageTitle(text: L.t("onb.look"))
            Text(verbatim: L.t("onb.look.text")).font(.sirtone(Ty.body)).foregroundStyle(p.textSecondary)
            Picker(L.t("settings.theme"), selection: $app.theme) {
                Text(verbatim: L.t("settings.theme.system")).tag("system")
                Text(verbatim: L.t("settings.theme.dark")).tag("dark")
                Text(verbatim: L.t("settings.theme.light")).tag("light")
            }
            .pickerStyle(.segmented)
            AccentPicker(selected: $app.accent)
            HStack(spacing: M.space3) {
                PrimaryButton(title: L.t("onb.next")) { step = 2 }
                SecondaryButton(title: L.t("onb.skip")) { step = 2 }
            }
        }
    }

    private var music: some View {
        let tracks = Int(app.counts?.tracks ?? 0)
        return VStack(alignment: .leading, spacing: M.space4) {
            if tracks > 0 {
                PageTitle(text: L.t("onb.ready", tracks))
                PrimaryButton(title: L.t("onb.listen"), icon: "play.fill") {
                    onboarded = true
                    app.startWave()
                }
            } else {
                PageTitle(text: L.t("onb.music"))
                Text(verbatim: L.t("onb.music.text")).font(.sirtone(Ty.body)).foregroundStyle(p.textSecondary)
                HStack(spacing: M.space3) {
                    PrimaryButton(title: L.t("library.add"), icon: "plus") { asking = true }
                    SecondaryButton(title: L.t("onb.skip")) { onboarded = true }
                }
            }
        }
    }
}
#endif
