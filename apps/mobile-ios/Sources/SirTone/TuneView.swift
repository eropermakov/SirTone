// S07 «Настроить волну» (MR §4, FR-WAVE-003): a bottom sheet with the four compact blocks of
// D-130 — «Под занятие», «По частоте», «Под настроение», «По языку» — and «Ещё настройки» with
// a count of what is on (character, positive/gloomy, time of day, weather, season); it opens
// already unfolded when something there is on. A tap adds, another takes away; it applies at
// once (the wave's queued picks are asked again) and stays until «Сбросить», as on the PC.
// «Подходит N треков» counts live. SF Symbols stand in for lucide (`06` §12).
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI

struct TuneView: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    @Environment(\.dismiss) private var dismiss
    @State private var count: TuningCount?
    @State private var more = false

    private static let blocks: [(block: String, keys: [(String, String)])] = [
        ("activity", [("workout", "dumbbell"), ("work", "briefcase"), ("road", "car"),
                      ("rest", "cup.and.saucer"), ("party", "party.popper"), ("sleep", "moon")]),
        ("frequency", [("loved", "heart"), ("often", "repeat"), ("forgotten", "clock.arrow.circlepath")]),
        ("mood", [("upbeat", "bolt"), ("happy", "sun.max"), ("calm", "leaf"), ("sad", "cloud.rain")]),
        ("language", [("ru", "character.cursor.ibeam"), ("foreign", "globe"), ("none", "music.note")]),
    ]

    /// «Ещё настройки»: several at once …
    private static let moreBlocks: [(block: String, keys: [(String, String)])] = [
        ("character", [("energetic", "hare"), ("aggressive", "flame"), ("calm", "tortoise")]),
        ("tone", [("positive", "face.smiling"), ("negative", "cloud")]),
    ]
    /// … and one of each.
    private static let single: [(block: String, keys: [(String, String)])] = [
        ("daypart", [("0", "sunrise"), ("1", "sun.haze"), ("2", "sun.max"), ("3", "sunset"),
                     ("4", "moon.haze"), ("5", "moon.stars")]),
        ("weather", [("0", "sun.max"), ("1", "cloud"), ("2", "cloud.fog"), ("3", "cloud.rain"),
                     ("4", "snowflake"), ("5", "cloud.bolt")]),
        ("season", [("winter", "snowflake"), ("spring", "leaf"), ("summer", "sun.max"),
                    ("autumn", "wind"), ("off", "xmark")]),
    ]

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: M.space6) {
                    ForEach(Self.blocks, id: \.block) { b in block(b.block, b.keys) }
                    if app.tuning.has("activity", "sleep") { sleepHint }
                    DisclosureGroup(isExpanded: $more) {
                        VStack(alignment: .leading, spacing: M.space6) {
                            ForEach(Self.moreBlocks, id: \.block) { b in block(b.block, b.keys) }
                            ForEach(Self.single, id: \.block) { b in block(b.block, b.keys) }
                        }
                        .padding(.top, M.space4)
                    } label: {
                        Text(verbatim: app.tuning.moreOn == 0
                             ? L.t("tune.more") : L.t("tune.moreOn", app.tuning.moreOn))
                            .font(.sirtone(Ty.bodyStrong))
                            .foregroundStyle(p.textPrimary)
                    }
                    .tint(p.icon)
                    if let count {
                        Text(verbatim: L.count("tune.count", Int(count.tracks))
                            + (count.relaxed ? L.t("tune.relaxed") : ""))
                            .font(.sirtone(Ty.caption, relativeTo: .caption))
                            .foregroundStyle(p.textSecondary)
                            .monospacedDigit()
                    }
                }
                .padding(M.space4)
            }
            .background(p.bgLayer)
            .navigationTitle(L.t("tune.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button(L.t("tune.reset")) { app.setTuning(WaveTuning(stored: "")) }
                        .disabled(app.tuning.isEmpty)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(L.t("tune.done")) { dismiss() }
                }
            }
            .task(id: app.tuning) { count = await app.tuningCount() }
            .onAppear { more = app.tuning.moreOn > 0 }
        }
        .presentationDetents([.medium, .large])
    }

    private func block(_ name: String, _ keys: [(String, String)]) -> some View {
        VStack(alignment: .leading, spacing: M.space2) {
            Text(verbatim: L.t("tune.block.\(name)"))
                .font(.sirtone(Ty.groupLabel, relativeTo: .subheadline))
                .foregroundStyle(p.textSecondary)
            FlowChips(keys: keys, block: name)
        }
    }

    private var sleepHint: some View {
        VStack(alignment: .leading, spacing: M.space2) {
            Text(verbatim: L.t("tune.hintSleep"))
                .font(.sirtone(Ty.body))
                .foregroundStyle(p.textSecondary)
            SecondaryButton(title: L.t("tune.sleepWave"), icon: "moon") {
                app.startSleepWave()
                dismiss()
            }
        }
    }
}

/// One block's chips (DESIGN.md §9 Chip: 32 pt, selected — Primary fill).
private struct FlowChips: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    let keys: [(String, String)]
    let block: String

    var body: some View {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 132), spacing: M.space2, alignment: .leading)],
                  alignment: .leading, spacing: M.space2) {
            ForEach(keys, id: \.0) { key, icon in
                let on = app.tuning.has(block, key)
                Button {
                    Haptics.tap()
                    app.setTuning(app.tuning.toggled(block, key))
                } label: {
                    HStack(spacing: M.space15) {
                        Image(systemName: icon)
                        Text(verbatim: L.t("tune.\(block).\(key)")).lineLimit(1)
                    }
                    .font(.sirtone(Ty.menu))
                    .padding(.horizontal, M.space3)
                    .frame(maxWidth: .infinity, minHeight: M.chipHeight, alignment: .leading)
                    .foregroundStyle(on ? p.onPrimary : p.textPrimary)
                    .background(on ? p.accent.primary : Color.clear, in: Capsule())
                    .overlay(Capsule().stroke(on ? Color.clear : p.border, lineWidth: 1))
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(on ? .isSelected : [])
            }
        }
    }
}

extension WaveTuning {
    private static let tones: Set<String> = ["positive", "negative"]

    var isEmpty: Bool { self == WaveTuning(stored: "") }

    /// What «Ещё настройки» has on (its label shows the number).
    var moreOn: Int {
        character.count + mood.filter(Self.tones.contains).count
            + (daypart == nil ? 0 : 1) + (weather == nil ? 0 : 1) + (season == nil ? 0 : 1)
    }

    func has(_ block: String, _ key: String) -> Bool {
        switch block {
        case "activity": activity.contains(key)
        case "frequency": frequency.contains(key)
        case "mood", "tone": mood.contains(key)
        case "language": language.contains(key)
        case "character": character.contains(key)
        case "daypart": daypart.map(String.init) == key
        case "weather": weather.map(String.init) == key
        case "season": season == key
        default: false
        }
    }

    func toggled(_ block: String, _ key: String) -> WaveTuning {
        func flip(_ l: [String]) -> [String] {
            l.contains(key) ? l.filter { $0 != key } : l + [key]
        }
        var t = self
        let on = has(block, key)
        switch block {
        case "activity": t.activity = flip(activity)
        case "frequency": t.frequency = flip(frequency)
        case "mood", "tone": t.mood = flip(mood)
        case "language": t.language = flip(language)
        case "character": t.character = flip(character)
        case "daypart": t.daypart = on ? nil : UInt8(key)
        case "weather": t.weather = on ? nil : UInt8(key)
        case "season": t.season = on ? nil : key
        default: break
        }
        return t
    }

    /// Kept between launches as «a,b|c|d|e|f|daypart|weather|season».
    var stored: String {
        let lists = [activity, frequency, mood, language, character].map { $0.joined(separator: ",") }
        return (lists + [daypart.map(String.init) ?? "", weather.map(String.init) ?? "", season ?? ""])
            .joined(separator: "|")
    }

    init(stored: String) {
        let parts = stored.split(separator: "|", omittingEmptySubsequences: false).map(String.init)
        func list(_ i: Int) -> [String] {
            i < parts.count ? parts[i].split(separator: ",").map(String.init) : []
        }
        func one(_ i: Int) -> String? { i < parts.count && !parts[i].isEmpty ? parts[i] : nil }
        self.init(activity: list(0), frequency: list(1), mood: list(2), language: list(3),
                  character: list(4), daypart: one(5).flatMap { UInt8($0) },
                  weather: one(6).flatMap { UInt8($0) }, season: one(7))
    }
}
#endif
