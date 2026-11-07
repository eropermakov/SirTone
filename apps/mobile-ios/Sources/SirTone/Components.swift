// Building blocks after DESIGN.md §9 on the phone (MR §4: touch targets ≥ 44 pt).
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI

typealias M = SirToneTokens.Metric
typealias Ty = SirToneTokens.Typography

/// A cover picture by hash, loaded off the main thread; placeholder until then (no spinners).
struct Artwork: View {
    let hash: String?
    var size: CGFloat
    var radius: CGFloat = M.radiusMd
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    @State private var image: UIImage?

    var body: some View {
        ZStack {
            p.artworkPlaceholder
            if let image {
                Image(uiImage: image).resizable().scaledToFill()
            } else {
                Image(systemName: "music.note")
                    .font(.system(size: size * 0.3))
                    .foregroundStyle(p.textTertiary)
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: radius, style: .continuous))
        .accessibilityHidden(true)
        .task(id: hash) {
            image = nil
            guard let hash, let lib = app.library else { return }
            // «Только музыка»: small pictures only (less decoding, less memory).
            let px = app.musicOnly ? 128 : UInt32(size * UIScreen.main.scale)
            image = await Task.detached {
                lib.coverPath(hash: hash, size: px).flatMap { UIImage(contentsOfFile: $0) }
            }.value
        }
    }
}

/// The cover of a track's album (rows, mini-player).
struct TrackArtwork: View {
    let trackId: Int64
    var size: CGFloat
    var radius: CGFloat = M.radiusSm
    @EnvironmentObject private var app: AppModel
    @State private var hash: String?

    var body: some View {
        Artwork(hash: hash, size: size, radius: radius)
            .task(id: trackId) {
                guard let lib = app.library else { return }
                let id = trackId
                hash = await Task.detached { lib.trackCover(trackId: id) }.value
            }
    }
}

enum Clock {
    static func text(_ ms: UInt64) -> String {
        let s = ms / 1000
        return s >= 3600
            ? String(format: "%d:%02d:%02d", s / 3600, s / 60 % 60, s % 60)
            : String(format: "%d:%02d", s / 60, s % 60)
    }
}

/// TrackRow (DESIGN.md §9): the whole row plays; the playing one shows in `accent-text`.
struct TrackRowView: View {
    let track: TrackItem
    var showArt = true
    var number: Int?
    let onPlay: () -> Void
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p

    var body: some View {
        let isNow = playback.current?.track.id == track.id
        Button(action: onPlay) {
            HStack(spacing: M.space3) {
                if showArt {
                    TrackArtwork(trackId: track.id, size: 44)
                } else if let number {
                    Text(verbatim: "\(number)")
                        .font(.sirtone(Ty.timecode)).monospacedDigit()
                        .foregroundStyle(isNow ? p.accent.text : p.textTertiary)
                        .frame(minWidth: 24)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(verbatim: track.title)
                        .font(.sirtone(Ty.body))
                        .foregroundStyle(isNow ? p.accent.text : p.textPrimary)
                        .lineLimit(1)
                    Text(verbatim: track.artists.isEmpty ? L.t("player.unknown_artist") : track.artists)
                        .font(.sirtone(Ty.caption, relativeTo: .caption))
                        .foregroundStyle(p.textSecondary)
                        .lineLimit(1)
                }
                Spacer(minLength: M.space2)
                Text(verbatim: Clock.text(track.durationMs))
                    .font(.sirtone(Ty.timecode, relativeTo: .caption2)).monospacedDigit()
                    .foregroundStyle(p.textTertiary)
            }
            .frame(minHeight: M.rowHeight)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button { playback.playNext(track) } label: {
                Label(L.t("menu.play_next"), systemImage: "text.insert")
            }
        }
    }
}

/// Primary pill (one per screen): `accent-fill` with `accent-on`.
struct PrimaryButton: View {
    let title: String
    var icon: String?
    let action: () -> Void
    @Environment(\.palette) private var p

    var body: some View {
        Button(action: { Haptics.tap(); action() }) {
            HStack(spacing: M.space2) {
                if let icon { Image(systemName: icon) }
                Text(verbatim: title).font(.sirtone(Ty.bodyStrong))
            }
            .padding(.horizontal, M.space6)
            .frame(minHeight: 48)
            .foregroundStyle(p.accent.on)
            .background(p.accent.fill, in: Capsule())
        }
        .buttonStyle(.plain)
    }
}

/// Secondary pill: transparent with a border.
struct SecondaryButton: View {
    let title: String
    var icon: String?
    let action: () -> Void
    @Environment(\.palette) private var p

    var body: some View {
        Button(action: action) {
            HStack(spacing: M.space2) {
                if let icon { Image(systemName: icon) }
                Text(verbatim: title).font(.sirtone(Ty.bodyStrong))
            }
            .padding(.horizontal, M.space6)
            .frame(minHeight: 48)
            .foregroundStyle(p.textPrimary)
            .overlay(Capsule().stroke(p.border, lineWidth: 1))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
    }
}

/// Page title in `primary-text` (DESIGN.md §5 page-title).
struct PageTitle: View {
    let text: String
    @Environment(\.palette) private var p
    var body: some View {
        Text(verbatim: text)
            .font(.sirtone(Ty.pageTitle, relativeTo: .largeTitle))
            .foregroundStyle(p.accent.primaryText)
            .accessibilityAddTraits(.isHeader)
    }
}

/// Errors and results as a toast at the bottom, never a modal (DESIGN.md §9).
struct Toast: ViewModifier {
    @Binding var text: String?
    @Environment(\.palette) private var p

    func body(content: Content) -> some View {
        content.overlay(alignment: .top) {
            if let text {
                Text(verbatim: text)
                    .font(.sirtone(Ty.menu))
                    .foregroundStyle(p.textPrimary)
                    .padding(.horizontal, M.space4)
                    .padding(.vertical, M.space3)
                    .background(p.bgFlyout, in: Capsule())
                    .overlay(Capsule().stroke(p.border, lineWidth: 1))
                    .padding(.top, M.space2)
                    .transition(.opacity)
                    .task(id: text) {
                        try? await Task.sleep(for: .seconds(3))
                        withAnimation(.easeOut(duration: 0.25)) { self.text = nil }
                    }
                    .accessibilityAddTraits(.updatesFrequently)
            }
        }
    }
}

/// Empty state: a quiet picture of primitives, one line and (optionally) one Primary action.
struct EmptyState: View {
    let title: String
    var hint: String?
    var action: (title: String, run: () -> Void)?
    @Environment(\.palette) private var p

    var body: some View {
        VStack(spacing: M.space4) {
            ZStack {
                Circle().fill(p.artworkPlaceholder).frame(width: 96, height: 96)
                Capsule().fill(p.inactive).frame(width: 48, height: 10).offset(y: 18)
                Circle().fill(p.inactive).frame(width: 28, height: 28).offset(y: -10)
            }
            .accessibilityHidden(true)
            Text(verbatim: title)
                .font(.sirtone(Ty.cardTitle))
                .foregroundStyle(p.textPrimary)
                .multilineTextAlignment(.center)
            if let hint {
                Text(verbatim: hint)
                    .font(.sirtone(Ty.caption, relativeTo: .caption))
                    .foregroundStyle(p.textSecondary)
                    .multilineTextAlignment(.center)
            }
            if let action { PrimaryButton(title: action.title, action: action.run) }
        }
        .padding(M.space8)
        .frame(maxWidth: .infinity)
    }
}
#endif

#if canImport(SirToneCore)
/// «В каком качестве добавить?» (D-146): asked on every way music comes in; «Оригинал» first
/// and the person's default marked.
struct QualityDialog: ViewModifier {
    @Binding var isPresented: Bool
    let choose: (String) -> Void
    @EnvironmentObject private var app: AppModel

    func body(content: Content) -> some View {
        content.confirmationDialog(L.t("quality.title"), isPresented: $isPresented, titleVisibility: .visible) {
            ForEach(["original", "economy", "max"], id: \.self) { q in
                Button(L.t("quality.\(q)") + (app.addQuality == q ? " ✓" : "")) { choose(q) }
            }
            Button(L.t("library.cancel"), role: .cancel) {}
        }
    }
}

extension View {
    func qualityDialog(isPresented: Binding<Bool>, choose: @escaping (String) -> Void) -> some View {
        modifier(QualityDialog(isPresented: isPresented, choose: choose))
    }
}
#endif
