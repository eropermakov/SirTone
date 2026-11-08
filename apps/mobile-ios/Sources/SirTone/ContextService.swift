// «Обстановка» on the phone (D-149, D-150, `mobile/full-port.md` §2; references `01d` §F): the
// sensors and what they mean for «Моя волна». Core Motion (walking, running, cycling, in a car —
// a motion must hold `stable_s`), the audio route (CarPlay or a Bluetooth device marked «Это
// машина»), places the person confirmed («Это дом?» / «Это работа?» — asked once, never decided
// silently), the weather from Open-Meteo (coordinates rounded to 0.1° by the core). Each signal
// has its own switch; permissions are asked when the switch is turned on, never at launch.
// Everything stays on the phone; places never travel to the PC.
#if canImport(SirToneCore)
import AVFoundation
import CoreLocation
import CoreMotion
import Foundation
import SirToneCore

@MainActor
final class ContextService: NSObject, ObservableObject, CLLocationManagerDelegate {
    /// What the wave follows now (an activity key) — for the card «Похоже, вы в пути».
    @Published private(set) var activity: String?
    /// «Это дом?» / «Это работа?» waiting for an answer (`home` / `work`).
    @Published private(set) var ask: String?
    /// The weather class now (`clear` … `storm`), for «Почему этот трек».
    @Published private(set) var weather: String?
    /// The Bluetooth output right now (to offer «Это машина»).
    @Published private(set) var bluetoothName: String?

    private let d = UserDefaults.standard
    private let rules = contextRules()
    private let motion = CMMotionActivityManager()
    private let location = CLLocationManager()
    private weak var model: AppModel?
    private var motionNow = "unknown"
    private var candidate: (kind: String, since: Date)?
    private var driveSince: Date?
    private var lastFix: CLLocation?
    private var weatherAt: Date?
    /// The person said «Отменить» to the card: the wave ignores context until it changes.
    private var dismissed: String?

    // The switches (all on by default except places, which ask first).
    var enabled: Bool { d.object(forKey: "ctxOn") as? Bool ?? true }
    var useMotion: Bool { enabled && (d.object(forKey: "ctxMotion") as? Bool ?? true) }
    var useCar: Bool { enabled && (d.object(forKey: "ctxCar") as? Bool ?? true) }
    var useWeather: Bool { enabled && (d.object(forKey: "ctxWeather") as? Bool ?? true) }
    var usePlaces: Bool { enabled && (d.object(forKey: "ctxPlaces") as? Bool ?? true) }
    var offline: Bool { d.bool(forKey: "offline") }

    override init() {
        super.init()
        location.delegate = self
        location.desiredAccuracy = kCLLocationAccuracyHundredMeters
        NotificationCenter.default.addObserver(
            forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.routeChanged() }
        }
    }

    func attach(model: AppModel) { self.model = model }

    /// On every return to the app and when a switch changes.
    func refresh() {
        guard enabled, model?.musicOnly == false else {
            stopMotion()
            apply()
            return
        }
        if useMotion { startMotion() } else { stopMotion() }
        routeChanged()
        if (useWeather || usePlaces), CLLocationManager.locationServicesEnabled() {
            switch location.authorizationStatus {
            case .notDetermined: location.requestWhenInUseAuthorization()
            case .authorizedWhenInUse, .authorizedAlways: location.requestLocation()
            default: break
            }
        }
        apply()
    }

    // MARK: Motion (Core Motion, `01d` M55: confidence ≥ medium, held `stable_s`)

    private func startMotion() {
        guard CMMotionActivityManager.isActivityAvailable() else { return }
        motion.startActivityUpdates(to: .main) { [weak self] a in
            guard let a, a.confidence != .low else { return }
            let kind = a.running ? "running" : a.cycling ? "cycling" : a.automotive ? "automotive"
                : a.walking ? "walking" : a.stationary ? "stationary" : "unknown"
            MainActor.assumeIsolated { self?.saw(kind) }
        }
    }

    private func stopMotion() {
        motion.stopActivityUpdates()
        motionNow = "unknown"
        candidate = nil
        driveSince = nil
    }

    private func saw(_ kind: String) {
        if candidate?.kind != kind { candidate = (kind, Date()) }
        guard let c = candidate, c.kind != motionNow,
              Date().timeIntervalSince(c.since) >= Double(rules.stableS) else { return }
        motionNow = c.kind
        driveSince = c.kind == "automotive" ? (driveSince ?? c.since) : nil
        apply()
    }

    /// «Долго еду»: more than `long_drive_min` in a car.
    var longDrive: Bool {
        driveSince.map { Date().timeIntervalSince($0) >= Double(rules.longDriveMin) * 60 } ?? false
    }

    // MARK: Car (the audio route)

    private var carNames: [String] {
        get { d.stringArray(forKey: "carNames") ?? [] }
        set { d.set(newValue, forKey: "carNames") }
    }

    private func routeChanged() {
        let outs = AVAudioSession.sharedInstance().currentRoute.outputs
        bluetoothName = outs.first { [.bluetoothA2DP, .bluetoothHFP, .bluetoothLE].contains($0.portType) }?.portName
        apply()
    }

    private var inCar: Bool {
        guard useCar else { return false }
        let outs = AVAudioSession.sharedInstance().currentRoute.outputs
        return outs.contains { $0.portType == .carAudio || carNames.contains($0.portName) }
    }

    /// «Это машина» for the current Bluetooth output (Spotify cannot, `01d` M54).
    func markCar(_ on: Bool) {
        guard let name = bluetoothName else { return }
        carNames = on ? Array(Set(carNames + [name])) : carNames.filter { $0 != name }
        apply()
    }

    var currentIsCar: Bool { bluetoothName.map { carNames.contains($0) } ?? false }

    // MARK: Places (D-150: asked, never decided silently)

    private struct Place: Codable { let kind: String; let lat: Double; let lon: Double }

    private var places: [Place] {
        get { (try? JSONDecoder().decode([Place].self, from: d.data(forKey: "places") ?? Data())) ?? [] }
        set { d.set(try? JSONEncoder().encode(newValue), forKey: "places") }
    }

    func hasPlace(_ kind: String) -> Bool { places.contains { $0.kind == kind } }

    func forgetPlace(_ kind: String) {
        places = places.filter { $0.kind != kind }
        apply()
    }

    private func place(at loc: CLLocation) -> String? {
        places.first {
            CLLocation(latitude: $0.lat, longitude: $0.lon).distance(from: loc) <= Double(rules.placeRadiusM)
        }?.kind
    }

    /// The answer to «Это дом?» / «Это работа?».
    func answer(_ yes: Bool) {
        guard let kind = ask, let loc = lastFix else { ask = nil; return }
        if yes {
            places = places.filter { $0.kind != kind }
                + [Place(kind: kind, lat: loc.coordinate.latitude, lon: loc.coordinate.longitude)]
        } else {
            d.set(Date(), forKey: "askedNo.\(kind)")
        }
        ask = nil
        apply()
    }

    /// Asks when the phone has stayed in one unnamed spot `ask_after_min` at home or work hours.
    private func considerAsking(_ loc: CLLocation) {
        guard usePlaces, ask == nil, place(at: loc) == nil else { return }
        let h = UInt8(Calendar.current.component(.hour, from: Date()))
        let weekday = !Calendar.current.isDateInWeekend(Date())
        let night = h >= rules.nightFromH || h < rules.nightToH
        let work = weekday && h >= rules.workFromH && h < rules.workToH
        guard let kind = night && !hasPlace("home") ? "home" : work && !hasPlace("work") ? "work" : nil
        else { return }
        // Not again for a week after «Нет».
        if let no = d.object(forKey: "askedNo.\(kind)") as? Date, Date().timeIntervalSince(no) < 7 * 86_400 {
            return
        }
        let spot = d.object(forKey: "spot") as? [Double]
        let since = d.object(forKey: "spotSince") as? Date
        if let s = spot, s.count == 2,
           CLLocation(latitude: s[0], longitude: s[1]).distance(from: loc) <= Double(rules.placeRadiusM),
           let since, Date().timeIntervalSince(since) >= Double(rules.askAfterMin) * 60 {
            ask = kind
        } else if spot == nil || since == nil
                    || CLLocation(latitude: spot![0], longitude: spot![1]).distance(from: loc)
                        > Double(rules.placeRadiusM) {
            d.set([loc.coordinate.latitude, loc.coordinate.longitude], forKey: "spot")
            d.set(Date(), forKey: "spotSince")
        }
    }

    // MARK: Location and weather

    nonisolated func locationManagerDidChangeAuthorization(_ m: CLLocationManager) {
        MainActor.assumeIsolated { self.authorizationChanged() }
    }

    private func authorizationChanged() {
        if [.authorizedWhenInUse, .authorizedAlways].contains(location.authorizationStatus) {
            location.requestLocation()
        }
    }

    nonisolated func locationManager(_ m: CLLocationManager, didUpdateLocations locs: [CLLocation]) {
        guard let loc = locs.last else { return }
        MainActor.assumeIsolated { self.got(loc) }
    }

    nonisolated func locationManager(_ m: CLLocationManager, didFailWithError error: Error) {}

    private func got(_ loc: CLLocation) {
        lastFix = loc
        considerAsking(loc)
        fetchWeather(loc)
        apply()
    }

    private func fetchWeather(_ loc: CLLocation) {
        guard useWeather, !offline, model?.musicOnly == false, let wave = model?.wave else { return }
        if let at = weatherAt, Date().timeIntervalSince(at) < Double(rules.weatherEveryMin) * 60 { return }
        weatherAt = Date()
        let lat = loc.coordinate.latitude, lon = loc.coordinate.longitude
        // Open-Meteo, the only weather host (`08` §4); the core rounds the coordinates.
        guard let url = URL(string: "https://api.open-meteo.com/v1/" + weatherQuery(lat: lat, lon: lon))
        else { return }
        Task {
            guard let (data, _) = try? await URLSession.shared.data(from: url) else { return }
            let cls = await Task.detached { wave.setWeather(json: data, lat: lat, lon: lon) }.value
            weather = cls
        }
    }

    // MARK: Decision

    /// The activity now, to the wave (only «Моя волна» without a chosen activity listens).
    func apply() {
        guard let model, let wave = model.wave else { return }
        var act: String?
        if enabled, model.musicOnly == false {
            let h = UInt8(Calendar.current.component(.hour, from: Date()))
            let here = usePlaces ? lastFix.flatMap(place(at:)) : nil
            act = autoActivity(motion: useMotion ? motionNow : "unknown", car: inCar,
                               place: here, localHour: h)
        }
        if act != dismissed { dismissed = nil }
        if act == dismissed { act = nil }
        let changed = activity != act
        activity = act
        let w = wave, decided = act
        Task {
            let moved = await Task.detached { w.setAutoActivity(activity: decided) }.value
            if moved, changed { model.playback.retune() }
        }
    }

    /// «Отменить» on the card: this context is ignored until it changes.
    func dismiss() {
        dismissed = activity
        apply()
    }

    /// A new wave (another profile) gets the context again.
    func reapply() {
        if let loc = lastFix { weatherAt = nil; fetchWeather(loc) }
        apply()
    }
}
#endif
