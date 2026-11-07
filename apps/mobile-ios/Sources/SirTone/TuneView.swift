// S07 «Настроить волну» (MR §4, FR-WAVE-003): a bottom sheet with the four compact blocks of
// D-130 — «Под занятие», «По частоте», «Под настроение», «По языку». A tap adds, another takes
// away; it applies at once (the wave's queued picks are asked again) and stays until «Сбросить»,
// as on the PC. «Подходит N треков» counts live. SF Symbols stand in for lucide (`06` §12).
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI

struct TuneView: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    @Environment(\.dismiss) private var dismiss
    @State private var count: TuningCount?

    private static let blocks: [(block: String, keys: [(String, String)])] = [
        ("activity", [("workout", "dumbbell"), ("work", "briefcase"), ("road", "car"),
                      ("rest", "cup.and.saucer"), ("party", "party.popper"), ("sleep", "moon")]),
        ("frequency", [("loved", "heart"), ("often", "repeat"), ("forgotten", "clock.arrow.circlepath")]),
        ("mood", [("upbeat", "bolt"), ("happy", "sun.max"), ("calm", "leaf"), ("sad", "cloud.rain")]),
        ("language", [("ru", "character.cursor.ibeam"), ("foreign", "globe"), ("none", "music.note")]),
    ]

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: M.space6) {
                    ForEach(Self.blocks, id: \.block) { b in
                        VStack(alignment: .leading, spacing: M.space2) {
                            Text(verbatim: L.t("tune.block.\(b.block)"))
                                .font(.sirtone(Ty.groupLabel, relativeTo: .subheadline))
                                .foregroundStyle(p.textSecondary)
                            FlowChips(keys: b.keys, block: b.block)
                        }
                    }
                    if app.tuning.has("activity", "sleep") {
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
                    Button(L.t("tune.reset")) { app.setTuning(WaveTuning(activity: [], frequency: [], mood: [], language: [])) }
                        .disabled(app.tuning.isEmpty)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(L.t("tune.done")) { dismiss() }
                }
            }
            .task(id: app.tuning) { count = await app.tuningCount() }
        }
        .presentationDetents([.medium, .large])
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
    var isEmpty: Bool { activity.isEmpty && frequency.isEmpty && mood.isEmpty && language.isEmpty }

    private func list(_ block: String) -> [String] {
        switch block {
        case "activity": activity
        case "frequency": frequency
        case "mood": mood
        default: language
        }
    }

    func has(_ block: String, _ key: String) -> Bool { list(block).contains(key) }

    func toggled(_ block: String, _ key: String) -> WaveTuning {
        var l = list(block)
        if let i = l.firstIndex(of: key) { l.remove(at: i) } else { l.append(key) }
        var t = self
        switch block {
        case "activity": t.activity = l
        case "frequency": t.frequency = l
        case "mood": t.mood = l
        default: t.language = l
        }
        return t
    }

    /// Kept between launches as «a,b|c|d|e» (four blocks).
    var stored: String {
        [activity, frequency, mood, language].map { $0.joined(separator: ",") }.joined(separator: "|")
    }

    init(stored: String) {
        let parts = stored.split(separator: "|", omittingEmptySubsequences: false)
            .map { $0.split(separator: ",").map(String.init) }
        func at(_ i: Int) -> [String] { i < parts.count ? parts[i] : [] }
        self.init(activity: at(0), frequency: at(1), mood: at(2), language: at(3))
    }
}
#endif
