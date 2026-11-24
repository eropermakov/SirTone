// Privacy-safe support diagnostics (D-180). This actor is called only from lifecycle/UI work;
// it must never be reached from an audio render callback.
#if canImport(SirToneCore)
import Foundation

struct DiagnosticReport: Sendable {
    let reportID: String
    let url: URL
}

enum FeedbackDeliveryError: LocalizedError {
    case unavailable, offline, rejected, invalidResponse

    var errorDescription: String? {
        switch self {
        case .unavailable: return L.t("feedback.unavailable")
        case .offline: return L.t("feedback.offline")
        case .rejected: return L.t("feedback.rejected")
        case .invalidResponse: return L.t("feedback.invalid_response")
        }
    }
}

private struct DiagnosticEvent: Codable {
    let schemaVersion: Int
    let timestampUtc: String
    let level: String
    let eventID: String
    let sessionID: String
    let platform: String
    let appVersion: String
    let buildID: String
    let releaseChannel: String
    let component: String
    let operation: String
    let errorCode: String?
    let reportID: String?
    let message: String
    let technicalContext: [String: String]
    let recoverable: Bool

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version", timestampUtc = "timestamp_utc", level
        case eventID = "event_id", sessionID = "session_id", platform
        case appVersion = "app_version", buildID = "build_id", releaseChannel = "release_channel"
        case component, operation, errorCode = "error_code", reportID = "report_id", message
        case technicalContext = "technical_context", recoverable
    }
}

/// Single local writer for the iOS side of the portable JSONL/ZIP format.
actor Diagnostics {
    static let shared = Diagnostics()

    private let maxBytes = 10 * 1024 * 1024
    private let keepDays = 7
    private var sessionID = UUID().uuidString.lowercased()
    private var started = false

    private var root: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("SirTone", isDirectory: true)
            .appendingPathComponent("diagnostics", isDirectory: true)
    }
    private var marker: URL { root.appendingPathComponent("active-session.json") }
    private var installationKey: String { "sirtone.diagnostics.installation-id" }

    func start() {
        guard !started else { return }
        started = true
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        _ = installationID
        let unclean = FileManager.default.fileExists(atPath: marker.path)
        sessionID = UUID().uuidString.lowercased()
        let markerData = (try? JSONSerialization.data(withJSONObject: [
            "schema_version": 1, "session_id": sessionID, "started_at_utc": ISO8601DateFormatter().string(from: Date())
        ])) ?? Data()
        try? markerData.write(to: marker, options: .atomic)
        record(level: "INFO", eventID: "APP_STARTED", component: "lifecycle", operation: "startup")
        if unclean {
            record(level: "WARN", eventID: "PREVIOUS_SESSION_UNCLEAN", component: "lifecycle", operation: "startup")
        }
    }

    /// Call when iOS backgrounds normally. The next foregrounding starts a fresh session.
    func finish() {
        guard started else { return }
        record(level: "INFO", eventID: "APP_STOPPED", component: "lifecycle", operation: "background")
        try? FileManager.default.removeItem(at: marker)
        started = false
    }

    /// Only stable event names and allow-listed technical atoms are accepted here.
    func record(
        level: String, eventID: String, component: String, operation: String,
        errorCode: String? = nil, reportID: String? = nil, context: [String: String] = [:], recoverable: Bool = true
    ) {
        let event = DiagnosticEvent(
            schemaVersion: 1, timestampUtc: ISO8601DateFormatter().string(from: Date()), level: atom(level),
            eventID: atom(eventID), sessionID: sessionID, platform: "ios", appVersion: appVersion,
            buildID: buildID, releaseChannel: "development", component: atom(component), operation: atom(operation),
            errorCode: errorCode.map(atom), reportID: reportID.map(atom), message: atom(eventID),
            technicalContext: allowed(context), recoverable: recoverable)
        guard let line = try? JSONEncoder().encode(event) else { return }
        let file = root.appendingPathComponent("events-\(dayString()).jsonl")
        if !FileManager.default.fileExists(atPath: file.path) { FileManager.default.createFile(atPath: file.path, contents: nil) }
        if let handle = try? FileHandle(forWritingTo: file) {
            defer { try? handle.close() }
            try? handle.seekToEnd()
            try? handle.write(contentsOf: line + Data([10]))
        }
        enforceRetention()
    }

    func resetIdentityAndData() {
        try? FileManager.default.removeItem(at: root)
        UserDefaults.standard.removeObject(forKey: installationKey)
        started = false
    }

    func exportReport(userMessage: String? = nil) throws -> DiagnosticReport {
        start()
        let reportID = UUID().uuidString.lowercased()
        let (events, truncated) = cappedEvents(recentEvents())
        let reportDir = root.deletingLastPathComponent().appendingPathComponent("reports", isDirectory: true)
        try FileManager.default.createDirectory(at: reportDir, withIntermediateDirectories: true)
        let url = reportDir.appendingPathComponent("sirtone-report-\(reportID).zip")
        let errors = events.filter { $0.level == "ERROR" }
        let eventLines = events.compactMap { try? String(data: JSONEncoder().encode($0), encoding: .utf8) }.joined(separator: "\n")
        let summary = """
        SIRTONE Diagnostic Report
        Report ID: \(reportID)
        Platform: iOS
        Version: \(appVersion)
        Build ID: \(buildID)
        Operating system: \(osVersion)
        Generated: \(ISO8601DateFormatter().string(from: Date()))
        Primary error: \(errors.first?.eventID ?? "none")
        Related errors: \(errors.count)
        Crash event: no native crash evidence in this export
        Diagnostics truncated: \(truncated ? "yes" : "no")
        User description: \(safeUserMessage(userMessage))
        """
        let manifest: [String: Any] = [
            "schema_version": 1, "report_id": reportID, "installation_id": installationID,
            "session_id": sessionID, "platform": "ios", "app_version": appVersion, "build_id": buildID,
            "release_channel": releaseChannel, "truncated": truncated
        ]
        let environment: [String: Any] = ["os": "iOS", "os_version": osVersion, "architecture": "arm64"]
        let entries: [(String, Data)] = [
            ("summary.txt", Data(summary.utf8)),
            ("manifest.json", try json(manifest)),
            ("environment.json", try json(environment)),
            ("events.jsonl", Data(eventLines.utf8)),
            ("errors.json", try JSONEncoder().encode(errors)),
            ("README.txt", Data("SIRTONE diagnostic report format v1. No music, library database, paths, listening history, preferences, passwords or tokens are included.\n".utf8))
        ]
        try StoredZip.write(entries: entries, to: url)
        record(level: "INFO", eventID: "REPORT_EXPORTED", component: "diagnostics", operation: "export", reportID: reportID)
        return DiagnosticReport(reportID: reportID, url: url)
    }

    /// Sends a previously prepared voluntary report. Keeping report preparation separate means a
    /// failed HTTPS request leaves the exact ZIP available to the UI for Share Sheet or Mail.
    func submit(kind: String, message: String, email: String?, report: DiagnosticReport?) async throws -> String {
        guard ["bug", "suggestion", "question", "other"].contains(kind), !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw FeedbackDeliveryError.rejected
        }
        guard let raw = Bundle.main.object(forInfoDictionaryKey: "SirToneFeedbackAPIURL") as? String,
              let endpoint = URL(string: raw), endpoint.scheme == "https" else { throw FeedbackDeliveryError.unavailable }
        let reportID = report?.reportID ?? UUID().uuidString.lowercased()
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.timeoutInterval = 30
        var payload: [String: Any] = [
            "type": kind, "message": message, "reportId": reportID, "platform": "iOS", "version": appVersion
        ]
        if let email, !email.isEmpty { payload["email"] = email }
        if let report { payload["reportZipBase64"] = try Data(contentsOf: report.url).base64EncodedString() }
        request.httpBody = try JSONSerialization.data(withJSONObject: payload)
        let data: Data
        let response: URLResponse
        do { (data, response) = try await URLSession.shared.data(for: request) }
        catch { throw FeedbackDeliveryError.offline }
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else { throw FeedbackDeliveryError.rejected }
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let reference = object["referenceId"] as? String, UUID(uuidString: reference) != nil else { throw FeedbackDeliveryError.invalidResponse }
        record(level: "INFO", eventID: "FEEDBACK_SENT", component: "feedback", operation: "submit", reportID: reportID)
        return reference
    }

    private var installationID: String {
        if let id = UserDefaults.standard.string(forKey: installationKey), UUID(uuidString: id) != nil { return id }
        let id = UUID().uuidString.lowercased()
        UserDefaults.standard.set(id, forKey: installationKey)
        return id
    }
    private var appVersion: String { Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "unknown" }
    private var releaseChannel: String {
        guard let value = Bundle.main.object(forInfoDictionaryKey: "SirToneReleaseChannel") as? String,
              !value.isEmpty else { return "development" }
        return value
    }

    private var buildID: String {
        let configured = Bundle.main.object(forInfoDictionaryKey: "SirToneBuildID") as? String
        if let configured, !configured.isEmpty { return configured }
        let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "unknown"
        return "\(appVersion)-\(releaseChannel)-\(build)"
    }
    private var osVersion: String { ProcessInfo.processInfo.operatingSystemVersionString }
    private func dayString() -> String { ISO8601DateFormatter().string(from: Date()).prefix(10).description }
    private func atom(_ value: String) -> String { String(value.prefix(120).filter { $0.isLetter || $0.isNumber || "_-.".contains($0) }) }
    private func allowed(_ context: [String: String]) -> [String: String] {
        let keys: Set<String> = ["attempt", "audio_device_kind", "error_kind", "network_state", "operation_state", "retry_after_ms"]
        return context.reduce(into: [:]) { if keys.contains($1.key) { $0[$1.key] = atom($1.value) } }
    }
    private func safeUserMessage(_ message: String?) -> String { message?.prefix(6000).description ?? "not provided" }
    private func json(_ object: [String: Any]) throws -> Data { try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]) }
    private func recentEvents() -> [DiagnosticEvent] {
        let cutoff = Date().addingTimeInterval(-86_400)
        let files = (try? FileManager.default.contentsOfDirectory(at: root, includingPropertiesForKeys: [.contentModificationDateKey])) ?? []
        return files.filter { $0.lastPathComponent.hasPrefix("events-") }.flatMap { url in
            guard let text = try? String(contentsOf: url) else { return [] }
            return text.split(separator: "\n").compactMap { try? JSONDecoder().decode(DiagnosticEvent.self, from: Data($0.utf8)) }
        }.filter { ISO8601DateFormatter().date(from: $0.timestampUtc).map { $0 >= cutoff } ?? false }
    }
    private func cappedEvents(_ events: [DiagnosticEvent]) -> ([DiagnosticEvent], Bool) {
        // `errors.json` can repeat the selected error events, so keep JSONL at 2 MB and retain
        // headroom under the portable 5 MB stored-ZIP ceiling.
        let limit = 2 * 1024 * 1024
        var total = 0; var kept: [DiagnosticEvent] = []
        for event in events.reversed() {
            let size = ((try? JSONEncoder().encode(event))?.count ?? 0) + 1
            if total + size <= limit { kept.append(event); total += size }
        }
        return (Array(kept.reversed()), kept.count != events.count)
    }
    private func enforceRetention() {
        let fm = FileManager.default
        var files = ((try? fm.contentsOfDirectory(at: root, includingPropertiesForKeys: [.contentModificationDateKey, .fileSizeKey])) ?? [])
            .filter { $0.lastPathComponent.hasPrefix("events-") }
        let cutoff = Date().addingTimeInterval(TimeInterval(-keepDays * 86_400))
        for file in files where ((try? file.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast) < cutoff { try? fm.removeItem(at: file) }
        files = files.sorted { $0.lastPathComponent < $1.lastPathComponent }
        var total = files.reduce(0) { $0 + ((try? $1.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0) }
        for file in files where total > maxBytes {
            let size = (try? file.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            try? fm.removeItem(at: file); total -= size
        }
    }
}

private enum StoredZip {
    static func write(entries: [(String, Data)], to url: URL) throws {
        var out = Data(); var directory = Data(); var offset: UInt32 = 0
        for (name, data) in entries {
            let nameData = Data(name.utf8); let crc = crc32(data); let size = UInt32(data.count)
            out.u32(0x04034b50); out.u16(20); out.u16(0); out.u16(0); out.u16(0); out.u16(0); out.u32(crc); out.u32(size); out.u32(size); out.u16(UInt16(nameData.count)); out.u16(0); out.append(nameData); out.append(data)
            directory.u32(0x02014b50); directory.u16(20); directory.u16(20); directory.u16(0); directory.u16(0); directory.u16(0); directory.u16(0); directory.u32(crc); directory.u32(size); directory.u32(size); directory.u16(UInt16(nameData.count)); directory.u16(0); directory.u16(0); directory.u16(0); directory.u16(0); directory.u32(0); directory.u32(offset); directory.append(nameData)
            offset = UInt32(out.count)
        }
        let centralOffset = UInt32(out.count); out.append(directory)
        out.u32(0x06054b50); out.u16(0); out.u16(0); out.u16(UInt16(entries.count)); out.u16(UInt16(entries.count)); out.u32(UInt32(directory.count)); out.u32(centralOffset); out.u16(0)
        try out.write(to: url, options: .atomic)
    }
    static func crc32(_ data: Data) -> UInt32 { data.reduce(UInt32.max) { crc, byte in (0..<8).reduce(crc ^ UInt32(byte)) { ($0 >> 1) ^ (($0 & 1 == 1) ? 0xedb88320 : 0) } } ^ UInt32.max }
}

private extension Data {
    mutating func u16(_ value: UInt16) { append(UInt8(value & 255)); append(UInt8(value >> 8)) }
    mutating func u32(_ value: UInt32) { u16(UInt16(value & 65535)); u16(UInt16(value >> 16)) }
}
#endif
