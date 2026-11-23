#if canImport(SirToneCore)
import SwiftUI
import SirToneDesign
#if canImport(MessageUI)
import MessageUI
#endif

struct FeedbackView: View {
    @Environment(\.palette) private var p
    // A failed request (or an app restart) must not discard the person's wording.
    @AppStorage("feedback.draft.kind") private var kind = "bug"
    @AppStorage("feedback.draft.message") private var message = ""
    @AppStorage("feedback.draft.email") private var email = ""
    @AppStorage("feedback.draft.include-diagnostics") private var includeDiagnostics = false
    @State private var sending = false
    @State private var result = ""
    @State private var export: DiagnosticReport?
    @State private var showMail = false
    @State private var confirmDiagnosticsReset = false

    var body: some View {
        Form {
            Section {
                Picker(L.t("feedback.kind"), selection: $kind) {
                    ForEach(["bug", "suggestion", "question", "other"], id: \.self) { item in
                        Text(verbatim: L.t("feedback.kind.\(item)")).tag(item)
                    }
                }
                TextEditor(text: $message)
                    .frame(minHeight: 120)
                    .accessibilityLabel(L.t("feedback.description"))
                TextField(L.t("feedback.email"), text: $email)
                    .textInputAutocapitalization(.never).keyboardType(.emailAddress).textContentType(.emailAddress)
            } header: { Text(verbatim: L.t("feedback.request")) }

            Section {
                Toggle(L.t("feedback.attach"), isOn: $includeDiagnostics).tint(p.accent.fill)
                Text(verbatim: L.t("feedback.attach_hint"))
                    .font(.sirtone(Ty.caption, relativeTo: .caption)).foregroundStyle(p.textSecondary)
                Button(L.t("feedback.save_report")) { exportReport() }
                    .disabled(sending).foregroundStyle(p.accent.text)
                Button(L.t("feedback.reset_diagnostics"), role: .destructive) {
                    confirmDiagnosticsReset = true
                }
                .disabled(sending)
                if let export {
                    ShareLink(item: export.url) { Text(verbatim: L.t("feedback.share_report")) }
                }
            } header: { Text(verbatim: L.t("feedback.technical")) }

            Section {
                Button(sending ? L.t("feedback.sending") : L.t("feedback.send")) { send() }
                    .disabled(sending || message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .foregroundStyle(p.accent.text)
                if !result.isEmpty { Text(verbatim: result).foregroundStyle(p.textSecondary) }
                if canSendMail, let export {
                    Button(L.t("feedback.mail_fallback")) { showMail = true }.foregroundStyle(p.accent.text)
                }
            }
        }
        .font(.sirtone(Ty.body)).scrollContentBackground(.hidden).background(p.bgLayer)
        .navigationTitle(L.t("feedback.title")).navigationBarTitleDisplayMode(.inline)
        .task { await Diagnostics.shared.start() }
        .confirmationDialog(
            L.t("feedback.reset_diagnostics_confirm"),
            isPresented: $confirmDiagnosticsReset,
            titleVisibility: .visible,
        ) {
            Button(L.t("feedback.reset_diagnostics"), role: .destructive) { resetDiagnostics() }
        }
        #if canImport(MessageUI)
        .sheet(isPresented: $showMail) {
            if let export { MailComposer(report: export, kind: kind, message: message, email: email) }
        }
        #endif
    }

    private var canSendMail: Bool {
        #if canImport(MessageUI)
        MFMailComposeViewController.canSendMail()
        #else
        false
        #endif
    }

    private func exportReport() {
        Task {
            do { export = try await Diagnostics.shared.exportReport(userMessage: message) }
            catch { result = L.t("feedback.export_failed") }
        }
    }

    private func send() {
        sending = true; result = ""
        Task {
            do {
                let report = if includeDiagnostics {
                    try await Diagnostics.shared.exportReport(userMessage: message)
                } else {
                    nil
                }
                // Retain the prepared ZIP on a delivery failure: the person can share it or
                // use the system Mail fallback without regenerating a different report.
                if let report { export = report }
                let reference = try await Diagnostics.shared.submit(kind: kind, message: message, email: email, report: report)
                result = L.t("feedback.sent", reference)
                message = ""; email = ""; includeDiagnostics = false
            } catch { result = error.localizedDescription }
            sending = false
        }
    }

    private func resetDiagnostics() {
        Task {
            await Diagnostics.shared.resetIdentityAndData()
            export = nil
            result = L.t("feedback.reset_diagnostics_done")
        }
    }
}

#if canImport(MessageUI)
private struct MailComposer: UIViewControllerRepresentable {
    let report: DiagnosticReport
    let kind: String
    let message: String
    let email: String
    @Environment(\.dismiss) private var dismiss

    func makeUIViewController(context: Context) -> MFMailComposeViewController {
        let view = MFMailComposeViewController()
        view.mailComposeDelegate = context.coordinator
        view.setToRecipients(["sir.kov.mail@gmail.com"])
        view.setSubject(L.t("feedback.mail_subject", kind.uppercased(), report.reportID))
        view.setMessageBody(L.t("feedback.mail_body", message, email.isEmpty ? L.t("feedback.no_email") : email), isHTML: false)
        if let data = try? Data(contentsOf: report.url) { view.addAttachmentData(data, mimeType: "application/zip", fileName: report.url.lastPathComponent) }
        return view
    }
    func updateUIViewController(_: MFMailComposeViewController, context _: Context) {}
    func makeCoordinator() -> Coordinator { Coordinator(dismiss: dismiss) }
    final class Coordinator: NSObject, MFMailComposeViewControllerDelegate {
        let dismiss: DismissAction
        init(dismiss: DismissAction) { self.dismiss = dismiss }
        func mailComposeController(_: MFMailComposeViewController, didFinishWith _: MFMailComposeResult, error _: Error?) { dismiss() }
    }
}
#endif
#endif
