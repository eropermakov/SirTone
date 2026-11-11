// The output device line at the top of «Сейчас играет» (D-170, owner's list 2026-11-11 p. 10):
// the device's name on the left (always), its charge and the time left on the right — only when
// the device itself offers the BLE Battery Service (0x180F). AirPods and ordinary Bluetooth
// headphones (A2DP/HFP) do not give their charge to apps; iOS shows it itself. The Bluetooth
// question comes the first time a Bluetooth output is shown, not at launch.
#if canImport(SirToneCore)
import AVFoundation
import CoreBluetooth
import SirToneDesign
import SwiftUI

@MainActor
final class OutputDevice: NSObject, ObservableObject, CBCentralManagerDelegate, CBPeripheralDelegate {
    /// The output's name (headphones, a speaker, the car); `nil` — the phone's own speaker.
    @Published private(set) var name: String?
    @Published private(set) var bluetooth = false
    /// 0…100 when the device tells; `nil` — it does not.
    @Published private(set) var level: Int?
    /// About how long the charge lasts at the rate it went down while playing.
    @Published private(set) var minutesLeft: Int?

    private var central: CBCentralManager?
    private var peripheral: CBPeripheral?
    /// Readings of the charge (time, level) for the time left.
    private var readings: [(Date, Int)] = []
    private var observer: NSObjectProtocol?

    /// The BLE Battery Service and its Battery Level (made where used: CBUUID is not Sendable).
    nonisolated static let batteryId = "180F"
    nonisolated static let levelId = "2A19"

    override init() {
        super.init()
        refresh()
        observer = NotificationCenter.default.addObserver(
            forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main
        ) { [weak self] _ in MainActor.assumeIsolated { self?.refresh() } }
    }

    /// The output now; a Bluetooth one is looked for among the devices offering a battery.
    func refresh() {
        let out = AVAudioSession.sharedInstance().currentRoute.outputs.first
        let bt: Set<AVAudioSession.Port> = [.bluetoothA2DP, .bluetoothHFP, .bluetoothLE]
        name = out.flatMap { $0.portType == .builtInSpeaker || $0.portType == .builtInReceiver ? nil : $0.portName }
        bluetooth = out.map { bt.contains($0.portType) } ?? false
        if name != readingsName { readings = []; level = nil; minutesLeft = nil }
        readingsName = name
        guard bluetooth else { return }
        // Created here, not at launch: creating it is what asks for Bluetooth (D-170).
        if central == nil {
            central = CBCentralManager(delegate: self, queue: .main)
        } else {
            find()
        }
    }

    private var readingsName: String?

    private func find() {
        guard let central, central.state == .poweredOn, let name else { return }
        let known = central.retrieveConnectedPeripherals(withServices: [CBUUID(string: Self.batteryId)])
        // Matched to the audio output by name: the system pairs them, apps cannot.
        guard let p = known.first(where: { $0.name == name }) else { return }
        peripheral = p
        p.delegate = self
        central.connect(p)
    }

    nonisolated func centralManagerDidUpdateState(_ central: CBCentralManager) {
        MainActor.assumeIsolated { find() }
    }

    // The callbacks come on the main queue (the manager's queue); the device calls stay here.
    nonisolated func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
        peripheral.discoverServices([CBUUID(string: Self.batteryId)])
    }

    nonisolated func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
        for s in peripheral.services ?? [] where s.uuid == CBUUID(string: Self.batteryId) {
            peripheral.discoverCharacteristics([CBUUID(string: Self.levelId)], for: s)
        }
    }

    nonisolated func peripheral(_ peripheral: CBPeripheral,
                                didDiscoverCharacteristicsFor service: CBService, error: Error?) {
        for c in service.characteristics ?? [] where c.uuid == CBUUID(string: Self.levelId) {
            peripheral.readValue(for: c)
            if c.properties.contains(.notify) { peripheral.setNotifyValue(true, for: c) }
        }
    }

    nonisolated func peripheral(_ peripheral: CBPeripheral,
                                didUpdateValueFor characteristic: CBCharacteristic, error: Error?) {
        guard let v = characteristic.value?.first else { return }
        MainActor.assumeIsolated { take(Int(v)) }
    }

    /// A new reading; the time left from the fall since the first reading of this device.
    private func take(_ v: Int) {
        level = min(100, max(0, v))
        let now = Date()
        if readings.last?.1 != v { readings.append((now, v)) }
        if let first = readings.first, let last = readings.last, first.1 > last.1 {
            let perMin = Double(first.1 - last.1) / max(1, last.0.timeIntervalSince(first.0) / 60)
            minutesLeft = perMin > 0 ? Int(Double(last.1) / perMin) : nil
        }
    }
}

/// The line itself: name on the left, battery on the right (the system's shape, neutral colours).
struct OutputDeviceLine: View {
    @ObservedObject var device: OutputDevice

    var body: some View {
        if let name = device.name {
            HStack(spacing: M.space2) {
                Image(systemName: device.bluetooth ? "headphones" : "hifispeaker")
                    .font(.system(size: 14))
                Text(verbatim: name).font(.sirtone(Ty.caption, relativeTo: .caption)).lineLimit(1)
                Spacer(minLength: M.space2)
                if let level = device.level {
                    if let m = device.minutesLeft {
                        Text(verbatim: L.t("bt.left", Clock.text(UInt64(m) * 60_000)))
                            .font(.sirtone(Ty.caption, relativeTo: .caption)).monospacedDigit()
                    }
                    Text(verbatim: "\(level)%").font(.sirtone(Ty.caption, relativeTo: .caption))
                        .monospacedDigit()
                    Image(systemName: level > 87 ? "battery.100" : level > 62 ? "battery.75"
                          : level > 37 ? "battery.50" : level > 12 ? "battery.25" : "battery.0")
                        .font(.system(size: 16))
                }
            }
            .opacity(0.85)
            .accessibilityElement(children: .combine)
        }
    }
}
#endif
