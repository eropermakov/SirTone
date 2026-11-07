// S02 «Медиатека», S08 album / artist / playlist, S09 «Добавить музыку» (M4.1, M4.4).
// Lists come in pages from the core (10 000 tracks scroll without waiting, MR §7).
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI
import UniformTypeIdentifiers

enum LibraryRoute: Hashable {
    case playlists, albums, artists, tracks, liked, unknown
    case album(id: Int64, title: String, cover: String?)
    case artist(id: Int64, name: String)
    case playlist(id: Int64, name: String)
}

struct LibraryView: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    @State private var path: [LibraryRoute] = []
    @State private var picking = false

    var body: some View {
        NavigationStack(path: $path) {
            List {
                Section {
                    PageTitle(text: L.t("tab.library"))
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                }
                if (app.counts?.tracks ?? 0) == 0 && !app.importing {
                    EmptyState(
                        title: L.t("library.empty"), hint: L.t("library.empty.hint"),
                        action: (L.t("library.add"), { picking = true })
                    )
                    .listRowBackground(Color.clear)
                } else {
                    row(.playlists, "music.note.list", L.t("library.playlists"))
                    row(.albums, "square.stack", L.t("library.albums"), app.counts.map { Int($0.albums) })
                    row(.artists, "music.mic", L.t("library.artists"), app.counts.map { Int($0.artists) })
                    row(.tracks, "music.note", L.t("library.tracks"), app.counts.map { Int($0.tracks) })
                    row(.liked, "heart", L.t("library.liked"))
                    if let u = app.counts?.unknown, u > 0 {
                        row(.unknown, "questionmark.square.dashed", L.t("library.unknown"), Int(u))
                    }
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(p.bgLayer)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { picking = true } label: { Image(systemName: "plus") }
                        .accessibilityLabel(L.t("library.add"))
                        .foregroundStyle(p.icon)
                }
            }
            .navigationDestination(for: LibraryRoute.self) { r in destination(r) }
            .fileImporter(
                isPresented: $picking,
                allowedContentTypes: [.audio, .folder],
                allowsMultipleSelection: true
            ) { result in
                if case let .success(urls) = result { app.importURLs(urls) }
            }
            .overlay(alignment: .top) {
                if app.importing {
                    ProgressView().tint(p.accent.text).padding(M.space2)
                }
            }
        }
    }

    private func row(_ r: LibraryRoute, _ icon: String, _ title: String, _ n: Int? = nil) -> some View {
        NavigationLink(value: r) {
            HStack(spacing: M.space3) {
                Image(systemName: icon).foregroundStyle(p.accent.primaryText).frame(width: 28)
                Text(verbatim: title).font(.sirtone(Ty.body)).foregroundStyle(p.textPrimary)
                Spacer()
                if let n {
                    Text(verbatim: "\(n)").font(.sirtone(Ty.caption, relativeTo: .caption))
                        .monospacedDigit().foregroundStyle(p.textTertiary)
                }
            }
            .frame(minHeight: 48)
        }
        .listRowBackground(p.bgLayer)
    }

    @ViewBuilder private func destination(_ r: LibraryRoute) -> some View {
        switch r {
        case .playlists: PlaylistsView()
        case .albums: AlbumsView()
        case .artists: ArtistsView()
        case .tracks: AllTracksView()
        case .liked: TrackListView(title: L.t("library.liked")) { lib in try lib.likedTracks(limit: 5000) }
        case .unknown: TrackListView(title: L.t("library.unknown")) { lib in try lib.unknownTracks(limit: 5000) }
        case let .album(id, title, cover):
            TrackListView(title: title, cover: cover, numbered: true) { lib in try lib.albumTracks(albumId: id) }
        case let .artist(id, name):
            TrackListView(title: name) { lib in try lib.artistTracks(artistId: id) }
        case let .playlist(id, name):
            TrackListView(title: name) { lib in try lib.playlistTracks(playlistId: id) }
        }
    }
}

/// A list of tracks with «Слушать» and «Перемешать» (S08).
struct TrackListView: View {
    let title: String
    var cover: String?
    var numbered = false
    let load: @Sendable (Library) throws -> [TrackItem]
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p
    @State private var tracks: [TrackItem]?

    init(title: String, cover: String? = nil, numbered: Bool = false,
         load: @escaping @Sendable (Library) throws -> [TrackItem]) {
        self.title = title
        self.cover = cover
        self.numbered = numbered
        self.load = load
    }

    var body: some View {
        List {
            VStack(alignment: .leading, spacing: M.space4) {
                if cover != nil || numbered {
                    Artwork(hash: cover, size: 220, radius: M.radiusLg).frame(maxWidth: .infinity)
                }
                PageTitle(text: title)
                if let tracks {
                    Text(verbatim: L.count("tracks", tracks.count))
                        .font(.sirtone(Ty.caption, relativeTo: .caption)).foregroundStyle(p.textSecondary)
                    if !tracks.isEmpty {
                        HStack(spacing: M.space3) {
                            PrimaryButton(title: L.t("library.listen"), icon: "play.fill") {
                                playback.play(tracks)
                            }
                            SecondaryButton(title: L.t("library.shuffle"), icon: "shuffle") {
                                playback.play(tracks, shuffled: true)
                            }
                        }
                    }
                }
            }
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
            if let tracks {
                if tracks.isEmpty {
                    Text(verbatim: L.t("library.nothing")).foregroundStyle(p.textSecondary)
                        .listRowBackground(Color.clear)
                }
                ForEach(Array(tracks.enumerated()), id: \.element.id) { i, t in
                    TrackRowView(track: t, showArt: !numbered, number: i + 1) { playback.play(tracks, at: i) }
                        .listRowBackground(p.bgLayer)
                }
            } else {
                ForEach(0..<8, id: \.self) { _ in SkeletonRow().listRowBackground(p.bgLayer) }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(p.bgLayer)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: app.revision) {
            guard let lib = app.library else { return }
            let load = self.load
            tracks = await Task.detached { (try? load(lib)) ?? [] }.value
        }
    }
}

/// Skeleton instead of a spinner (DESIGN.md §1).
struct SkeletonRow: View {
    @Environment(\.palette) private var p
    var body: some View {
        HStack(spacing: M.space3) {
            RoundedRectangle(cornerRadius: M.radiusSm).fill(p.artworkPlaceholder).frame(width: 44, height: 44)
            VStack(alignment: .leading, spacing: 6) {
                Capsule().fill(p.artworkPlaceholder).frame(width: 160, height: 12)
                Capsule().fill(p.artworkPlaceholder).frame(width: 100, height: 10)
            }
        }
        .frame(minHeight: M.rowHeight)
        .accessibilityHidden(true)
    }
}

/// «Треки»: every track by title, page by page.
struct AllTracksView: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p
    @State private var tracks: [TrackItem] = []
    @State private var next: TrackCursor?
    @State private var done = false
    /// One page at a time: several rows appear together and would ask for the same page.
    @State private var loading = false

    var body: some View {
        List {
            PageTitle(text: L.t("library.tracks")).listRowBackground(Color.clear).listRowSeparator(.hidden)
            ForEach(Array(tracks.enumerated()), id: \.element.id) { i, t in
                TrackRowView(track: t) { playback.play(tracks, at: i) }
                    .listRowBackground(p.bgLayer)
                    .onAppear { if i >= tracks.count - 20 { Task { await more() } } }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(p.bgLayer)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: app.revision) {
            tracks = []; next = nil; done = false
            await more()
        }
    }

    private func more() async {
        guard !done, !loading, let lib = app.library else { return }
        loading = true
        defer { loading = false }
        let after = next
        guard let page = await Task.detached(operation: { try? lib.tracks(after: after, limit: 200) }).value
        else { done = true; return }
        tracks += page.tracks
        next = page.next
        done = page.next == nil
    }
}

/// «Альбомы»: a two-column grid of covers.
struct AlbumsView: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    @State private var albums: [AlbumItem] = []
    @State private var next: PageCursor?
    @State private var done = false
    /// One page at a time: several rows appear together and would ask for the same page.
    @State private var loading = false

    private let cols = [GridItem(.flexible(), spacing: M.space4), GridItem(.flexible(), spacing: M.space4)]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: M.space4) {
                PageTitle(text: L.t("library.albums"))
                LazyVGrid(columns: cols, spacing: M.space6) {
                    ForEach(Array(albums.enumerated()), id: \.element.id) { i, a in
                        NavigationLink(value: LibraryRoute.album(id: a.id, title: a.title, cover: a.cover)) {
                            VStack(alignment: .leading, spacing: M.space2) {
                                GeometryReader { g in
                                    Artwork(hash: a.cover, size: g.size.width)
                                        .shadow(color: .black.opacity(0.24), radius: 8, y: 4)
                                }
                                .aspectRatio(1, contentMode: .fit)
                                Text(verbatim: a.title).font(.sirtone(Ty.body)).foregroundStyle(p.textPrimary).lineLimit(1)
                                Text(verbatim: a.artist).font(.sirtone(Ty.caption, relativeTo: .caption))
                                    .foregroundStyle(p.textSecondary).lineLimit(1)
                            }
                        }
                        .buttonStyle(.plain)
                        .onAppear { if i >= albums.count - 6 { Task { await more() } } }
                    }
                }
            }
            .padding(.horizontal, M.space4)
        }
        .background(p.bgLayer)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: app.revision) {
            albums = []; next = nil; done = false
            await more()
        }
    }

    private func more() async {
        guard !done, !loading, let lib = app.library else { return }
        loading = true
        defer { loading = false }
        let after = next
        guard let page = await Task.detached(operation: { try? lib.albums(after: after, limit: 60) }).value
        else { done = true; return }
        albums += page.albums
        next = page.next
        done = page.next == nil
    }
}

/// «Исполнители»: round initials (photos come later), by name.
struct ArtistsView: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    @State private var artists: [ArtistItem] = []
    @State private var next: PageCursor?
    @State private var done = false
    /// One page at a time: several rows appear together and would ask for the same page.
    @State private var loading = false

    var body: some View {
        List {
            PageTitle(text: L.t("library.artists")).listRowBackground(Color.clear).listRowSeparator(.hidden)
            ForEach(Array(artists.enumerated()), id: \.element.id) { i, a in
                NavigationLink(value: LibraryRoute.artist(id: a.id, name: a.name)) {
                    HStack(spacing: M.space3) {
                        Text(verbatim: String(a.name.prefix(1)).uppercased())
                            .font(.sirtone(Ty.bodyStrong))
                            .foregroundStyle(p.textSecondary)
                            .frame(width: 44, height: 44)
                            .background(p.artworkPlaceholder, in: Circle())
                        VStack(alignment: .leading, spacing: 2) {
                            Text(verbatim: a.name).font(.sirtone(Ty.body)).foregroundStyle(p.textPrimary).lineLimit(1)
                            Text(verbatim: L.count("tracks", Int(a.tracks)))
                                .font(.sirtone(Ty.caption, relativeTo: .caption)).foregroundStyle(p.textSecondary)
                        }
                    }
                    .frame(minHeight: M.rowHeight)
                }
                .listRowBackground(p.bgLayer)
                .onAppear { if i >= artists.count - 20 { Task { await more() } } }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(p.bgLayer)
        .navigationBarTitleDisplayMode(.inline)
        .task(id: app.revision) {
            artists = []; next = nil; done = false
            await more()
        }
    }

    private func more() async {
        guard !done, !loading, let lib = app.library else { return }
        loading = true
        defer { loading = false }
        let after = next
        guard let page = await Task.detached(operation: { try? lib.artists(after: after, limit: 200) }).value
        else { done = true; return }
        artists += page.artists
        next = page.next
        done = page.next == nil
    }
}

/// «Плейлисты»: manual playlists and «Новый плейлист».
struct PlaylistsView: View {
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    @State private var lists: [PlaylistItem] = []
    @State private var naming = false
    @State private var name = ""

    var body: some View {
        List {
            PageTitle(text: L.t("library.playlists")).listRowBackground(Color.clear).listRowSeparator(.hidden)
            Button { naming = true } label: {
                Label(L.t("library.new_playlist"), systemImage: "plus")
                    .font(.sirtone(Ty.body)).foregroundStyle(p.accent.text).frame(minHeight: 48)
            }
            .listRowBackground(p.bgLayer)
            ForEach(lists, id: \.id) { pl in
                NavigationLink(value: LibraryRoute.playlist(id: pl.id, name: pl.name)) {
                    HStack(spacing: M.space3) {
                        Artwork(hash: pl.covers.first, size: 48, radius: M.radiusSm)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(verbatim: pl.name).font(.sirtone(Ty.body)).foregroundStyle(p.textPrimary)
                            Text(verbatim: L.count("tracks", Int(pl.tracks)))
                                .font(.sirtone(Ty.caption, relativeTo: .caption)).foregroundStyle(p.textSecondary)
                        }
                    }
                }
                .listRowBackground(p.bgLayer)
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(p.bgLayer)
        .navigationBarTitleDisplayMode(.inline)
        .alert(L.t("library.new_playlist"), isPresented: $naming) {
            TextField(L.t("library.playlist_name"), text: $name)
            Button(L.t("library.create")) { create() }
            Button(L.t("library.cancel"), role: .cancel) { name = "" }
        }
        .task(id: app.revision) { await load() }
    }

    private func load() async {
        guard let lib = app.library else { return }
        lists = await Task.detached { (try? lib.playlists()) ?? [] }.value
    }

    private func create() {
        guard let lib = app.library else { return }
        let n = name.trimmingCharacters(in: .whitespaces)
        name = ""
        guard !n.isEmpty else { return }
        Task {
            _ = await Task.detached { try? lib.createPlaylist(name: n) }.value
            await load()
        }
    }
}

/// S03 «Поиск»: field in focus, results from the first letter by group.
struct SearchView: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var playback: Playback
    @Environment(\.palette) private var p
    @State private var query = ""
    @State private var results: SearchResults?
    @AppStorage("recentSearches") private var recentRaw = ""

    private var recent: [String] { recentRaw.split(separator: "\n").map(String.init) }

    var body: some View {
        NavigationStack {
            List {
                if query.isEmpty {
                    if !recent.isEmpty {
                        Section(L.t("search.recent")) {
                            ForEach(recent, id: \.self) { q in
                                Button(q) { query = q }.foregroundStyle(p.textPrimary)
                                    .listRowBackground(p.bgLayer)
                            }
                        }
                    }
                } else if let r = results {
                    if r.tracks.isEmpty && r.artists.isEmpty && r.albums.isEmpty {
                        Text(verbatim: L.t("search.empty")).foregroundStyle(p.textSecondary)
                            .listRowBackground(p.bgLayer)
                    }
                    if !r.artists.isEmpty {
                        Section(L.t("library.artists")) {
                            ForEach(r.artists, id: \.id) { a in
                                NavigationLink(value: LibraryRoute.artist(id: a.id, name: a.name)) {
                                    Text(verbatim: a.name).font(.sirtone(Ty.body))
                                }
                                .listRowBackground(p.bgLayer)
                            }
                        }
                    }
                    if !r.albums.isEmpty {
                        Section(L.t("library.albums")) {
                            ForEach(r.albums, id: \.id) { a in
                                NavigationLink(value: LibraryRoute.album(id: a.id, title: a.name, cover: nil)) {
                                    Text(verbatim: a.name).font(.sirtone(Ty.body))
                                }
                                .listRowBackground(p.bgLayer)
                            }
                        }
                    }
                    if !r.tracks.isEmpty {
                        Section(L.t("library.tracks")) {
                            ForEach(Array(r.tracks.enumerated()), id: \.element.id) { i, t in
                                TrackRowView(track: t) {
                                    remember(query)
                                    playback.play(r.tracks, at: i)
                                }
                                .listRowBackground(p.bgLayer)
                            }
                        }
                    }
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(p.bgLayer)
            .navigationTitle(L.t("tab.search"))
            .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always),
                        prompt: L.t("search.prompt"))
            .navigationDestination(for: LibraryRoute.self) { r in
                switch r {
                case let .album(id, title, cover):
                    TrackListView(title: title, cover: cover, numbered: true) { lib in try lib.albumTracks(albumId: id) }
                case let .artist(id, name):
                    TrackListView(title: name) { lib in try lib.artistTracks(artistId: id) }
                default: EmptyView()
                }
            }
            .task(id: query) {
                let q = query.trimmingCharacters(in: .whitespaces)
                guard !q.isEmpty, let lib = app.library else { results = nil; return }
                results = await Task.detached { try? lib.search(query: q) }.value
            }
        }
    }

    private func remember(_ q: String) {
        let q = q.trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else { return }
        recentRaw = ([q] + recent.filter { $0 != q }).prefix(8).joined(separator: "\n")
    }
}
#endif
