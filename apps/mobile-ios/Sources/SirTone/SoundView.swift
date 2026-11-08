// «Настройки → Звук» (D-148, PC `settingsEq`, `settingsSound`, `settingsMix`): the equalizer
// with the PC's presets and ten bands, loudness normalization (Spotify's three targets, as on the
// PC) and transitions between tracks — «Мягко» / «Умно» from the PC's planner over the analysis
// that came from the PC, or a plain crossfade. Applies at once (heard within 0.4 s).
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI

/// The sound settings as the player reads them (UserDefaults; the screen writes them).
enum SoundPrefs {
    private static var d: UserDefaults { .standard }

    struct Eq { let on: Bool; let preamp: Float; let bands: [Float] }

    static var eq: Eq {
        let bands = (d.string(forKey: "eqBands") ?? "")
            .split(separator: ",").compactMap { Float($0) }
        return Eq(on: d.bool(forKey: "eqOn"), preamp: 0,
                  bands: bands.count == 10 ? bands : Array(repeating: 0, count: 10))
    }

    /// The target, LUFS; `nil` — normalization off.
    static var normTarget: Float? {
        let t = d.object(forKey: "normTarget") as? Double ?? -14
        return d.object(forKey: "normOn") as? Bool ?? true ? Float(t) : nil
    }

    /// The planner's mode: `soft`, `smart`, `crossfade:<s>`; `nil` — one after another.
    static var transition: String? {
        switch d.string(forKey: "transition") ?? "off" {
        case "soft": "soft"
        case "smart": "smart"
        case "crossfade": "crossfade:\(d.object(forKey: "crossfadeS") as? Int ?? 6)"
        default: nil
        }
    }
}

struct SoundSettingsView: View {
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p
    @AppStorage("eqOn") private var eqOn = false
    @AppStorage("eqPreset") private var preset = "flat"
    @AppStorage("eqBands") private var bandsStored = ""
    @AppStorage("normOn") private var normOn = true
    @AppStorage("normTarget") private var normTarget = -14.0
    @AppStorage("transition") private var transition = "off"
    @AppStorage("crossfadeS") private var crossfadeS = 6
    @State private var bands: [Float] = Array(repeating: 0, count: 10)

    private let presets = eqPresets()
    private let hz = eqBandsHz()

    var body: some View {
        List {
            Section {
                Toggle(L.t("sound.eq"), isOn: $eqOn).tint(p.accent.fill)
                if eqOn {
                    Picker(L.t("sound.preset"), selection: $preset) {
                        ForEach(presets, id: \.id) { pr in
                            Text(verbatim: L.t("eq.\(pr.id)")).tag(pr.id)
                        }
                        Text(verbatim: L.t("eq.custom")).tag("custom")
                    }
                    ForEach(Array(bands.indices), id: \.self) { i in
                        band(i)
                    }
                }
            } header: { Text(verbatim: L.t("sound.eq")) }
            .listRowBackground(p.bgRaised)

            Section {
                Toggle(L.t("sound.norm"), isOn: $normOn).tint(p.accent.fill)
                if normOn {
                    Picker(L.t("sound.norm.level"), selection: $normTarget) {
                        Text(verbatim: L.t("sound.norm.quiet")).tag(-19.0)
                        Text(verbatim: L.t("sound.norm.normal")).tag(-14.0)
                        Text(verbatim: L.t("sound.norm.loud")).tag(-11.0)
                    }
                    .pickerStyle(.segmented)
                }
            } header: { Text(verbatim: L.t("sound.norm")) } footer: {
                Text(verbatim: L.t("sound.norm.hint"))
            }
            .listRowBackground(p.bgRaised)

            Section {
                Picker(L.t("sound.mix"), selection: $transition) {
                    Text(verbatim: L.t("sound.mix.off")).tag("off")
                    Text(verbatim: L.t("sound.mix.soft")).tag("soft")
                    Text(verbatim: L.t("sound.mix.smart")).tag("smart")
                    Text(verbatim: L.t("sound.mix.crossfade")).tag("crossfade")
                }
                if transition == "crossfade" {
                    Stepper(value: $crossfadeS, in: 1...12) {
                        Text(verbatim: L.t("sound.mix.seconds", crossfadeS))
                    }
                }
            } header: { Text(verbatim: L.t("sound.mix")) } footer: {
                Text(verbatim: L.t("sound.mix.hint.\(transition)"))
            }
            .listRowBackground(p.bgRaised)
        }
        .font(.sirtone(Ty.body))
        .foregroundStyle(p.textPrimary)
        .scrollContentBackground(.hidden)
        .background(p.bgLayer)
        .navigationTitle(L.t("sound.title"))
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { bands = SoundPrefs.eq.bands }
        .onChange(of: preset) { _, id in
            guard let pr = presets.first(where: { $0.id == id }) else { return }
            bands = pr.bandsDb
            store()
        }
        .onChange(of: eqOn) { _, _ in playback.applyEq() }
        .onChange(of: normOn) { _, _ in playback.applySound() }
        .onChange(of: normTarget) { _, _ in playback.applySound() }
        .onChange(of: transition) { _, _ in playback.applySound() }
        .onChange(of: crossfadeS) { _, _ in playback.applySound() }
    }

    private func band(_ i: Int) -> some View {
        HStack(spacing: M.space3) {
            Text(verbatim: label(hz.indices.contains(i) ? hz[i] : 0))
                .font(.sirtone(Ty.caption, relativeTo: .caption)).monospacedDigit()
                .frame(width: 44, alignment: .leading)
            Slider(value: Binding(
                get: { Double(bands[i]) },
                set: { v in
                    bands[i] = Float((v * 2).rounded() / 2)
                    if preset != "custom" { preset = "custom" }
                    store()
                }
            ), in: -12...12)
            .tint(p.accent.text)
            Text(verbatim: String(format: "%+.1f", bands[i]))
                .font(.sirtone(Ty.caption, relativeTo: .caption)).monospacedDigit()
                .frame(width: 44, alignment: .trailing)
        }
        .accessibilityElement(children: .combine)
    }

    private func label(_ f: Float) -> String {
        f >= 1000 ? L.t("sound.khz", Int(f / 1000)) : L.t("sound.hz", Int(f.rounded()))
    }

    private func store() {
        bandsStored = bands.map { String($0) }.joined(separator: ",")
        playback.applyEq()
    }
}
#endif
