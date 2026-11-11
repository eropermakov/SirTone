// Сиртон for iPhone (stage 8, `docs/mobile/README.md` §4): three tabs like Apple Music — «Волна»,
// «Медиатека», «Поиск» — with the mini-player above them and «Сейчас играет» growing from it.
// The look comes from DESIGN.md tokens (`SirToneDesign`); the Rust core does the work.
import SwiftUI
import SirToneDesign
#if canImport(SirToneCore)
import SirToneCore
#endif

@main
struct SirToneApp: App {
    #if canImport(SirToneCore)
    @StateObject private var app = AppModel()
    #endif

    init() { Typeface.register() }

    var body: some Scene {
        WindowGroup {
            #if canImport(SirToneCore)
            RootView()
                .environmentObject(app)
                .environmentObject(app.playback)
                .onOpenURL { url in app.pendingImport.append(url) }
            #else
            Text(verbatim: "core not linked").padding()
            #endif
        }
    }
}

#if canImport(SirToneCore)
struct RootView: View {
    @EnvironmentObject private var app: AppModel
    @EnvironmentObject private var playback: Playback
    @Environment(\.colorScheme) private var system
    @Environment(\.colorSchemeContrast) private var contrast
    @Environment(\.scenePhase) private var phase
    @State private var expanded = false
    @AppStorage("onboarded") private var onboarded = false

    var body: some View {
        let p = app.palette(system: system, contrast: contrast)
        tabs
        .tint(p.accent.text)
        .environment(\.palette, p)
        .preferredColorScheme(app.preferredScheme)
        .modifier(Toast(text: $playback.notice))
        .manageHost(active: !expanded)
        .fullScreenCover(isPresented: $expanded) {
            NowPlayingView().environment(\.palette, p)
        }
        .fullScreenCover(isPresented: Binding(get: { !onboarded }, set: { onboarded = !$0 })) {
            OnboardingView().environment(\.palette, p)
        }
        .qualityDialog(isPresented: Binding(
            get: { !app.pendingImport.isEmpty },
            set: { if !$0 { app.pendingImport = [] } }
        )) { q in
            let urls = app.pendingImport
            app.pendingImport = []
            app.importURLs(urls, quality: q)
        }
        .onAppear { app.open() }
        .onChange(of: phase) { _, now in
            app.playback.setOnScreen(now == .active)
            if now == .active { app.becameActive() }
        }
    }

    /// On iOS 26+ as Apple Music (D-169): system glass tabs that shrink on scroll, «Поиск» as
    /// its own round button, the mini-player above the tabs; before that the tabs as they were.
    @ViewBuilder private var tabs: some View {
        if #available(iOS 26.1, *) {
            TabView {
                Tab(L.t("tab.wave"), systemImage: "water.waves") { WaveView() }
                Tab(L.t("tab.library"), systemImage: "square.stack") { LibraryView() }
                Tab(L.t("tab.search"), systemImage: "magnifyingglass", role: .search) { SearchView() }
            }
            .tabBarMinimizeBehavior(.onScrollDown)
            .tabViewBottomAccessory(isEnabled: playback.current != nil) {
                MiniPlayer(expanded: $expanded, inAccessory: true)
            }
        } else {
            TabView {
                tab(WaveView(), L.t("tab.wave"), "water.waves")
                tab(LibraryView(), L.t("tab.library"), "square.stack")
                tab(SearchView(), L.t("tab.search"), "magnifyingglass")
            }
        }
    }

    private func tab(_ content: some View, _ title: String, _ icon: String) -> some View {
        content
            .safeAreaInset(edge: .bottom, spacing: 0) { MiniPlayer(expanded: $expanded) }
            .tabItem { Label(title, systemImage: icon) }
    }
}
#endif
