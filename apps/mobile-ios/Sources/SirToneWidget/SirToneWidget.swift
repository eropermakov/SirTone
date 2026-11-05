// Widget stub for check П5 (extension + App Group on a free Apple ID). Real widget — М3.
import SwiftUI
import WidgetKit
import SirToneDesign

struct Entry: TimelineEntry { let date: Date }

struct Provider: TimelineProvider {
    func placeholder(in context: Context) -> Entry { Entry(date: .now) }
    func getSnapshot(in context: Context, completion: @escaping (Entry) -> Void) { completion(Entry(date: .now)) }
    func getTimeline(in context: Context, completion: @escaping (Timeline<Entry>) -> Void) {
        completion(Timeline(entries: [Entry(date: .now)], policy: .never))
    }
}

@main
struct SirToneWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "studio.serkov.sirtone.nowplaying", provider: Provider()) { _ in
            Text(verbatim: "Сиртон")
                .foregroundStyle(SirToneTokens.Dark.textPrimary)
                .containerBackground(SirToneTokens.Dark.bgBase, for: .widget)
        }
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}
