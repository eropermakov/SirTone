import { FileAuditStore } from "./audit-store.mjs";
import { createFeedbackServer } from "./server.mjs";
import { SmtpMailer } from "./smtp.mjs";

const port = Number.parseInt(process.env.PORT ?? "8080", 10);
const host = process.env.HOST ?? "127.0.0.1";
const supportTo = required("FEEDBACK_SUPPORT_TO");
const from = required("FEEDBACK_SMTP_FROM");
const smtpHost = required("FEEDBACK_SMTP_HOST");
const smtpPort = Number.parseInt(process.env.FEEDBACK_SMTP_PORT ?? "465", 10);
const secure = process.env.FEEDBACK_SMTP_SECURE !== "false";
const local = process.env.FEEDBACK_ALLOW_INSECURE_LOCAL_SMTP === "true";
const audit = new FileAuditStore({
  path: required("FEEDBACK_AUDIT_PATH"),
  salt: required("FEEDBACK_AUDIT_SALT"),
});
const mailer = new SmtpMailer({
  host: smtpHost,
  port: smtpPort,
  secure,
  from,
  allowInsecureLocal: local,
});
const server = createFeedbackServer({ mailer, supportTo, audit });

server.listen(port, host, () => {
  // Never emit request bodies, email addresses, reports, SMTP credentials or headers.
  console.info(`SIRTONE feedback API listening on ${host}:${port}`);
});

function required(name) {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is required`);
  return value;
}
