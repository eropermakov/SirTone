// «Настройки → Волна» and «Настройки → Обстановка» (D-148…D-150; PC `settingsWave`,
// `settingsNever`, `settingsPlace`, `settingsNet`): the influences of season, context and
// events, «Никогда в волне», each context signal with its own switch, «Это машина», the asked
// places, «Без интернета», the visualiser's look. Applies at once, no «Save».
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI

struct WaveSettingsView: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    @State private var inf: [Float] = [1, 1, 1]
    @State private var never: [TrackItem] = []

    var body: some View {
        List {
            Section {
                ForEach(Array(["season", "context", "events"].enumerated()), id: \.offset) { i, k in
                    VStack(alignment: .leading, spacing: M.space1) {
                        Text(verbatim: L.t("wavecfg.inf.\(k)"))
                        Slider(value: Binding(
                            get: { Double(inf[i]) },
                            set: { inf[i] = Float(($0 * 10).rounded() / 10) }
                        ), in: 0...2) { editing in
                            if !editing { app.setInfluence(inf) }
                        }
                        .tint(p.accent.text)
                    }
                }
            } header: { Text(verbatim: L.t("wavecfg.inf")) } footer: {
                Text(verbatim: L.t("wavecfg.inf.hint"))
            }
            .listRowBackground(p.bgRaised)

            Section {
                if never.isEmpty {
                    Text(verbatim: L.t("wavecfg.never.empty")).foregroundStyle(p.textSecondary)
                }
                ForEach(never, id: \.id) { t in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(verbatim: t.title).lineLimit(1)
                        Text(verbatim: t.artists).font(.sirtone(Ty.caption, relativeTo: .caption))
                            .foregroundStyle(p.textSecondary).lineLimit(1)
                    }
                }
                .onDelete { idx in
                    let gone = idx.map { never[$0] }
                    never.remove(atOffsets: idx)
                    guard let lib = app.library else { return }
                    Task.detached { for t in gone { try? lib.setNever(trackId: t.id, on: false) } }
                }
            } header: { Text(verbatim: L.t("wavecfg.never")) } footer: {
                Text(verbatim: L.t("wavecfg.never.hint"))
            }
            .listRowBackground(p.bgRaised)
        }
        .font(.sirtone(Ty.body))
        .foregroundStyle(p.textPrimary)
        .scrollContentBackground(.hidden)
        .background(p.bgLayer)
        .navigationTitle(L.t("wavecfg.title"))
        .navigationBarTitleDisplayMode(.inline)
        .task {
            inf = app.influence
            guard let lib = app.library else { return }
            never = await Task.detached { (try? lib.neverTracks()) ?? [] }.value
        }
    }
}

struct ContextSettingsView: View {
    @EnvironmentObject private var app: AppModel
    @ObservedObject var ctx: ContextService
    @Environment(\.palette) private var p
    @AppStorage("ctxOn") private var on = true
    @AppStorage("ctxMotion") private var motion = true
    @AppStorage("ctxCar") private var car = true
    @AppStorage("ctxWeather") private var weather = true
    @AppStorage("ctxPlaces") private var places = true
    @AppStorage("offline") private var offline = false
    @AppStorage("vizMode") private var vizMode = "waves"

    var body: some View {
        List {
            Section {
                Toggle(L.t("ctx.on"), isOn: $on).tint(p.accent.fill)
                if on {
                    Toggle(L.t("ctx.motion"), isOn: $motion).tint(p.accent.fill)
                    Toggle(L.t("ctx.car"), isOn: $car).tint(p.accent.fill)
                    if car, let name = ctx.bluetoothName {
                        Toggle(L.t("ctx.isCar", name), isOn: Binding(
                            get: { ctx.currentIsCar }, set: { ctx.markCar($0) }
                        )).tint(p.accent.fill)
                    }
                    Toggle(L.t("ctx.weather"), isOn: $weather).tint(p.accent.fill)
                    Toggle(L.t("ctx.places"), isOn: $places).tint(p.accent.fill)
                }
            } header: { Text(verbatim: L.t("ctx.title")) } footer: {
                Text(verbatim: L.t("ctx.hint"))
            }
            .listRowBackground(p.bgRaised)

            if on && places {
                Section {
                    ForEach(["home", "work"], id: \.self) { k in
                        HStack {
                            Text(verbatim: L.t("ctx.place.\(k)"))
                            Spacer()
                            if ctx.hasPlace(k) {
                                Button(L.t("ctx.forget")) { ctx.forgetPlace(k) }
                                    .foregroundStyle(p.danger)
                            } else {
                                Text(verbatim: L.t("ctx.place.unset")).foregroundStyle(p.textSecondary)
                            }
                        }
                    }
                } header: { Text(verbatim: L.t("ctx.places")) } footer: {
                    Text(verbatim: L.t("ctx.places.hint"))
                }
                .listRowBackground(p.bgRaised)
            }

            Section {
                Toggle(L.t("net.offline"), isOn: $offline).tint(p.accent.fill)
            } footer: { Text(verbatim: L.t("net.offline.hint")) }
            .listRowBackground(p.bgRaised)

            Section {
                Picker(L.t("viz.mode"), selection: $vizMode) {
                    Text(verbatim: L.t("viz.waves")).tag("waves")
                    Text(verbatim: L.t("viz.milkdrop")).tag("milkdrop")
                }
            } header: { Text(verbatim: L.t("viz.open")) }
            .listRowBackground(p.bgRaised)
        }
        .font(.sirtone(Ty.body))
        .foregroundStyle(p.textPrimary)
        .scrollContentBackground(.hidden)
        .background(p.bgLayer)
        .navigationTitle(L.t("ctx.title"))
        .navigationBarTitleDisplayMode(.inline)
        .onChange(of: [on, motion, car, weather, places, offline]) { _, _ in ctx.refresh() }
    }
}
#endif
