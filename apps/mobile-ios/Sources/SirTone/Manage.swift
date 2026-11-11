// Managing the music on the phone (D-165, owner's list 2026-11-11 p. 5): «Добавить в плейлист»,
// «Удалить с телефона» from any track, album, artist and the player; the questions are shown by
// one host (the root, or «Сейчас играет» while it covers the screen).
#if canImport(SirToneCore)
import SirToneCore
import SirToneDesign
import SwiftUI

/// Tracks waiting for «В какой плейлист?».
struct AddRequest: Identifiable {
    let id = UUID()
    let tracks: [Int64]
}

/// Tracks waiting for «Удалить с телефона?», with what it frees.
struct DeleteRequest: Identifiable {
    let id = UUID()
    let tracks: [Int64]
    let title: String
    let size: Deletion
}

extension AppModel {
    /// «Добавить в плейлист…»: the sheet asks which one.
    func askAdd(_ tracks: [Int64]) {
        guard !tracks.isEmpty else { return }
        adding = AddRequest(tracks: tracks)
    }

    func add(_ tracks: [Int64], to playlist: PlaylistItem) {
        guard let lib = library else { return }
        Task {
            _ = await Task.detached { try? lib.addToPlaylist(playlistId: playlist.id, trackIds: tracks) }.value
            playback.notice = L.t("playlist.added", playlist.name)
            await refresh()
        }
    }

    func addToNew(_ tracks: [Int64], name: String) {
        guard let lib = library else { return }
        let n = name.trimmingCharacters(in: .whitespaces)
        guard !n.isEmpty else { return }
        Task {
            _ = await Task.detached { () -> UInt32? in
                guard let id = try? lib.createPlaylist(name: n) else { return nil }
                return try? lib.addToPlaylist(playlistId: id, trackIds: tracks)
            }.value
            playback.notice = L.t("playlist.added", n)
            await refresh()
        }
    }

    /// «Удалить с телефона…»: counts what it frees first, then asks.
    func askDelete(_ tracks: [Int64], title: String) {
        guard let lib = library, !tracks.isEmpty else { return }
        Task {
            guard let size = await Task.detached(operation: { try? lib.deleteSize(trackIds: tracks) }).value,
                  size.tracks > 0 else { return }
            deleting = DeleteRequest(tracks: tracks, title: title, size: size)
        }
    }

    /// The person said «Удалить»: the player lets go of them, the files go to the trash.
    func delete(_ r: DeleteRequest) {
        guard let lib = library else { return }
        playback.forget(Set(r.tracks))
        Task {
            let done = await Task.detached { try? lib.deleteTracks(trackIds: r.tracks) }.value
            playback.notice = done.map { L.t("delete.done", L.count("tracks", Int($0.tracks))) }
                ?? L.t("delete.failed")
            await refresh()
        }
    }

    func removeFromPlaylist(_ track: Int64, playlist: Int64) {
        guard let lib = library else { return }
        Task {
            _ = await Task.detached { try? lib.removeFromPlaylist(playlistId: playlist, trackId: track) }.value
            await refresh()
        }
    }

    func renamePlaylist(_ id: Int64, to name: String) {
        guard let lib = library else { return }
        let n = name.trimmingCharacters(in: .whitespaces)
        guard !n.isEmpty else { return }
        Task {
            _ = await Task.detached { try? lib.renamePlaylist(playlistId: id, name: n) }.value
            await refresh()
        }
    }

    func deletePlaylist(_ id: Int64) {
        guard let lib = library else { return }
        Task {
            _ = await Task.detached { try? lib.deletePlaylist(playlistId: id) }.value
            await refresh()
        }
    }
}

/// Shows the add and delete questions; `active` — this host is the one on screen.
struct ManageHost: ViewModifier {
    let active: Bool
    @EnvironmentObject private var app: AppModel

    func body(content: Content) -> some View {
        content
            .sheet(item: Binding(
                get: { active ? app.adding : nil },
                set: { if $0 == nil { app.adding = nil } }
            )) { r in
                AddToPlaylistSheet(tracks: r.tracks).presentationDetents([.medium, .large])
            }
            .confirmationDialog(
                app.deleting.map { L.t("delete.title", $0.title) } ?? "",
                isPresented: Binding(
                    get: { active && app.deleting != nil },
                    set: { if !$0 { app.deleting = nil } }
                ),
                titleVisibility: .visible,
                presenting: app.deleting
            ) { r in
                Button(L.t("delete.confirm"), role: .destructive) { app.delete(r) }
                Button(L.t("library.cancel"), role: .cancel) {}
            } message: { r in
                Text(verbatim: L.t("delete.text", L.count("tracks", Int(r.size.tracks)),
                                   ByteCountFormatter.string(fromByteCount: Int64(r.size.bytes), countStyle: .file)))
            }
    }
}

extension View {
    func manageHost(active: Bool) -> some View { modifier(ManageHost(active: active)) }
}

/// «В какой плейлист?»: the manual playlists and «Новый плейлист».
struct AddToPlaylistSheet: View {
    let tracks: [Int64]
    @EnvironmentObject private var app: AppModel
    @Environment(\.palette) private var p
    @Environment(\.dismiss) private var dismiss
    @State private var lists: [PlaylistItem] = []
    @State private var naming = false
    @State private var name = ""

    var body: some View {
        NavigationStack {
            List {
                Button { naming = true } label: {
                    Label(L.t("library.new_playlist"), systemImage: "plus")
                        .font(.sirtone(Ty.body)).foregroundStyle(p.accent.text).frame(minHeight: 44)
                }
                .listRowBackground(p.bgFlyout)
                ForEach(lists, id: \.id) { pl in
                    Button {
                        app.add(tracks, to: pl)
                        dismiss()
                    } label: {
                        HStack(spacing: M.space3) {
                            Artwork(hash: pl.covers.first, size: 44, radius: M.radiusSm)
                            Text(verbatim: pl.name).font(.sirtone(Ty.body)).foregroundStyle(p.textPrimary)
                            Spacer()
                            Text(verbatim: L.count("tracks", Int(pl.tracks)))
                                .font(.sirtone(Ty.caption, relativeTo: .caption)).foregroundStyle(p.textSecondary)
                        }
                    }
                    .buttonStyle(.plain)
                    .listRowBackground(p.bgFlyout)
                }
            }
            .scrollContentBackground(.hidden)
            .background(p.bgFlyout)
            .navigationTitle(L.t("menu.add_to_playlist"))
            .navigationBarTitleDisplayMode(.inline)
            .alert(L.t("library.new_playlist"), isPresented: $naming) {
                TextField(L.t("library.playlist_name"), text: $name)
                Button(L.t("library.create")) {
                    app.addToNew(tracks, name: name)
                    name = ""
                    dismiss()
                }
                Button(L.t("library.cancel"), role: .cancel) { name = "" }
            }
            .task {
                guard let lib = app.library else { return }
                lists = await Task.detached { (try? lib.playlists()) ?? [] }.value
            }
        }
    }
}

/// The «⋯» of an album, an artist or a playlist page (S08).
struct CollectionMenu: View {
    enum Kind { case album, artist, playlist(Int64) }
    let kind: Kind
    let title: String
    let tracks: [TrackItem]
    @EnvironmentObject private var app: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var renaming = false
    @State private var name = ""
    @State private var confirmDelete = false

    var body: some View {
        Menu {
            Button { app.askAdd(tracks.map(\.id)) } label: {
                Label(L.t("menu.add_to_playlist"), systemImage: "text.badge.plus")
            }
            .disabled(tracks.isEmpty)
            switch kind {
            case .playlist:
                Button { name = title; renaming = true } label: {
                    Label(L.t("playlist.rename"), systemImage: "pencil")
                }
                Button(role: .destructive) { confirmDelete = true } label: {
                    Label(L.t("playlist.delete"), systemImage: "trash")
                }
            case .album, .artist:
                Button(role: .destructive) { app.askDelete(tracks.map(\.id), title: title) } label: {
                    Label(L.t("menu.delete"), systemImage: "trash")
                }
                .disabled(tracks.isEmpty)
            }
        } label: {
            Image(systemName: "ellipsis.circle").frame(width: 44, height: 44)
        }
        .accessibilityLabel(L.t("menu.more"))
        .alert(L.t("playlist.rename"), isPresented: $renaming) {
            TextField(L.t("library.playlist_name"), text: $name)
            Button(L.t("playlist.save")) {
                if case let .playlist(id) = kind { app.renamePlaylist(id, to: name) }
            }
            Button(L.t("library.cancel"), role: .cancel) {}
        }
        .confirmationDialog(L.t("playlist.delete.title", title), isPresented: $confirmDelete,
                            titleVisibility: .visible) {
            Button(L.t("playlist.delete"), role: .destructive) {
                if case let .playlist(id) = kind {
                    app.deletePlaylist(id)
                    dismiss()
                }
            }
            Button(L.t("library.cancel"), role: .cancel) {}
        } message: {
            Text(verbatim: L.t("playlist.delete.text"))
        }
    }
}
#endif
