import { createServer } from "node:http";

const MAX_BODY_BYTES = 7 * 1024 * 1024;
const MAX_REPORT_BYTES = 5 * 1024 * 1024;
const MAX_MESSAGE_CHARS = 6000;
const ALLOWED_TYPES = new Set(["bug", "suggestion", "question", "other"]);
const ALLOWED_REPORT_FILES = new Set([
  "summary.txt",
  "manifest.json",
  "environment.json",
  "events.jsonl",
  "errors.json",
  "README.txt",
]);

export class FeedbackError extends Error {
  constructor(status, code) {
    super(code);
    this.status = status;
    this.code = code;
  }
}

/** An in-memory implementation used only by automated tests and local development. */
export class MemoryMailer {
  messages = [];

  async send(message) {
    this.messages.push(message);
  }
}

/**
 * A minimal server-side API. It deliberately accepts JSON plus a base64 report rather than
 * parsing multipart uploads; report ZIPs are never extracted by the server.
 */
export function createFeedbackServer({ mailer, supportTo, now = () => Date.now(), rateLimit = 5 }) {
  if (!mailer || typeof mailer.send !== "function") throw new TypeError("mailer.send is required");
  if (!isEmail(supportTo)) throw new TypeError("a fixed supportTo email is required");
  const deliveries = new Map();
  const attempts = new Map();

  return createServer(async (request, response) => {
    try {
      if (request.method !== "POST" || request.url !== "/v1/feedback/reports") {
        throw new FeedbackError(404, "not_found");
      }
      const ip = request.socket.remoteAddress ?? "unknown";
      enforceRateLimit(attempts, ip, now(), rateLimit);
      const payload = validatePayload(await readJson(request));
      const previous = deliveries.get(payload.reportId);
      if (previous) {
        return respond(response, 200, { referenceId: previous.referenceId, duplicate: true });
      }
      const referenceId = payload.reportId;
      const message = mailFor(payload, supportTo, referenceId, now());
      await mailer.send(message);
      deliveries.set(payload.reportId, { referenceId, receivedAt: now() });
      return respond(response, 202, { referenceId, duplicate: false });
    } catch (error) {
      const status = error instanceof FeedbackError ? error.status : 500;
      const code = error instanceof FeedbackError ? error.code : "delivery_unavailable";
      return respond(response, status, { error: code });
    }
  });
}

function enforceRateLimit(attempts, ip, now, limit) {
  const windowStart = now - 60 * 60 * 1000;
  const recent = (attempts.get(ip) ?? []).filter((time) => time >= windowStart);
  if (recent.length >= limit) throw new FeedbackError(429, "rate_limited");
  recent.push(now);
  attempts.set(ip, recent);
}

async function readJson(request) {
  if (!request.headers["content-type"]?.startsWith("application/json")) {
    throw new FeedbackError(415, "json_required");
  }
  const chunks = [];
  let bytes = 0;
  for await (const chunk of request) {
    bytes += chunk.length;
    if (bytes > MAX_BODY_BYTES) throw new FeedbackError(413, "request_too_large");
    chunks.push(chunk);
  }
  try {
    return JSON.parse(Buffer.concat(chunks).toString("utf8"));
  } catch {
    throw new FeedbackError(400, "invalid_json");
  }
}

function validatePayload(raw) {
  if (!raw || typeof raw !== "object" || Array.isArray(raw))
    throw new FeedbackError(400, "invalid_payload");
  const type = raw.type;
  const message = typeof raw.message === "string" ? raw.message.trim() : "";
  const reportId = raw.reportId;
  const platform = raw.platform;
  const version = raw.version;
  if (!ALLOWED_TYPES.has(type)) throw new FeedbackError(400, "invalid_type");
  if (!message || message.length > MAX_MESSAGE_CHARS)
    throw new FeedbackError(400, "invalid_message");
  if (!isUuid(reportId)) throw new FeedbackError(400, "invalid_report_id");
  if (!new Set(["Windows", "iOS"]).has(platform)) throw new FeedbackError(400, "invalid_platform");
  if (typeof version !== "string" || !/^[A-Za-z0-9._-]{1,64}$/.test(version)) {
    throw new FeedbackError(400, "invalid_version");
  }
  const email = raw.email == null || raw.email === "" ? null : raw.email;
  if (email !== null && !isEmail(email)) throw new FeedbackError(400, "invalid_email");
  const report = raw.reportZipBase64 == null ? null : decodeAndValidateReport(raw.reportZipBase64);
  return { type, message, reportId, platform, version, email, report };
}

function decodeAndValidateReport(base64) {
  if (
    typeof base64 !== "string" ||
    !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(base64)
  ) {
    throw new FeedbackError(400, "invalid_report");
  }
  const report = Buffer.from(base64, "base64");
  if (!report.length || report.length > MAX_REPORT_BYTES)
    throw new FeedbackError(413, "report_too_large");
  validateReportZip(report);
  return report;
}

/** Validate central-directory metadata only. The attachment is never extracted on this server. */
export function validateReportZip(bytes) {
  if (bytes.length < 22 || bytes.readUInt32LE(0) !== 0x04034b50) {
    throw new FeedbackError(400, "invalid_report");
  }
  const searchStart = Math.max(0, bytes.length - 65_557);
  let eocd = -1;
  for (let i = bytes.length - 22; i >= searchStart; i -= 1) {
    if (bytes.readUInt32LE(i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0 || eocd + 22 > bytes.length) throw new FeedbackError(400, "invalid_report");
  const entries = bytes.readUInt16LE(eocd + 10);
  const centralSize = bytes.readUInt32LE(eocd + 12);
  let at = bytes.readUInt32LE(eocd + 16);
  if (entries !== ALLOWED_REPORT_FILES.size || at + centralSize > eocd) {
    throw new FeedbackError(400, "invalid_report");
  }
  const names = new Set();
  let totalUncompressed = 0;
  for (let i = 0; i < entries; i += 1) {
    if (at + 46 > eocd || bytes.readUInt32LE(at) !== 0x02014b50) {
      throw new FeedbackError(400, "invalid_report");
    }
    const flags = bytes.readUInt16LE(at + 8);
    const method = bytes.readUInt16LE(at + 10);
    const compressed = bytes.readUInt32LE(at + 20);
    const uncompressed = bytes.readUInt32LE(at + 24);
    const nameLength = bytes.readUInt16LE(at + 28);
    const extraLength = bytes.readUInt16LE(at + 30);
    const commentLength = bytes.readUInt16LE(at + 32);
    const end = at + 46 + nameLength + extraLength + commentLength;
    if (end > eocd || flags !== 0 || method !== 0 || compressed !== uncompressed) {
      throw new FeedbackError(400, "invalid_report");
    }
    const name = bytes.subarray(at + 46, at + 46 + nameLength).toString("utf8");
    if (!ALLOWED_REPORT_FILES.has(name) || names.has(name))
      throw new FeedbackError(400, "invalid_report");
    names.add(name);
    totalUncompressed += uncompressed;
    if (totalUncompressed > MAX_REPORT_BYTES) throw new FeedbackError(413, "report_too_large");
    at = end;
  }
  if (at !== bytes.readUInt32LE(eocd + 16) + centralSize)
    throw new FeedbackError(400, "invalid_report");
}

function mailFor(payload, supportTo, referenceId, now) {
  const type = payload.type.toUpperCase();
  return {
    to: supportTo,
    from: null,
    replyTo: payload.email,
    subject: `[SIRTONE] [${type}] [${payload.platform}] [${payload.version}] [${referenceId}]`,
    text: [
      "Новое обращение SIRTONE",
      `Номер обращения: ${referenceId}`,
      `Тип: ${payload.type}`,
      `Платформа: ${payload.platform}`,
      `Версия: ${payload.version}`,
      `Дата UTC: ${new Date(now).toISOString()}`,
      `Контакт: ${payload.email ?? "не указан"}`,
      "",
      "Сообщение пользователя:",
      payload.message,
      "",
      `Технический отчёт: ${payload.report ? "приложен" : "не приложен"}`,
    ].join("\n"),
    attachment: payload.report
      ? {
          filename: `sirtone-report-${referenceId}.zip`,
          contentType: "application/zip",
          bytes: payload.report,
        }
      : null,
  };
}

function isUuid(value) {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value)
  );
}

function isEmail(value) {
  return (
    typeof value === "string" && value.length <= 254 && /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value)
  );
}

function respond(response, status, body) {
  response.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "cache-control": "no-store",
  });
  response.end(JSON.stringify(body));
}
