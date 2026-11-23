import { createServer } from "node:http";
import { isIP } from "node:net";
import { MemoryAuditStore } from "./audit-store.mjs";

const MAX_BODY_BYTES = 7 * 1024 * 1024;
const MAX_REPORT_BYTES = 5 * 1024 * 1024;
const MAX_MESSAGE_CHARS = 6000;
// A 5 MiB base64 report must have time to cross an ordinary mobile connection, but a client
// must not be able to occupy a Node request indefinitely by sending it byte by byte.
const REQUEST_TIMEOUT_MS = 60_000;
const HEADERS_TIMEOUT_MS = 15_000;
const KEEP_ALIVE_TIMEOUT_MS = 5_000;
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
export function createFeedbackServer({
  mailer,
  supportTo,
  now = () => Date.now(),
  rateLimit = 5,
  trustLoopbackProxy = false,
  audit = new MemoryAuditStore(),
}) {
  if (!mailer || typeof mailer.send !== "function") throw new TypeError("mailer.send is required");
  if (!isEmail(supportTo)) throw new TypeError("a fixed supportTo email is required");
  if (
    !audit ||
    typeof audit.enforceRateLimit !== "function" ||
    typeof audit.reserve !== "function" ||
    typeof audit.commitDelivery !== "function" ||
    typeof audit.release !== "function"
  ) {
    throw new TypeError("an audit store is required");
  }

  const server = createServer(async (request, response) => {
    try {
      if (request.method !== "POST" || request.url !== "/v1/feedback/reports") {
        throw new FeedbackError(404, "not_found");
      }
      const ip = clientAddress(request, trustLoopbackProxy);
      if (!(await audit.enforceRateLimit(ip, now(), rateLimit)))
        throw new FeedbackError(429, "rate_limited");
      const payload = validatePayload(await readJson(request));
      const reservation = await audit.reserve(payload.reportId, now());
      if (reservation.kind === "delivered") {
        return respond(response, 200, { referenceId: reservation.referenceId, duplicate: true });
      }
      if (reservation.kind === "pending") throw new FeedbackError(409, "report_pending");
      const referenceId = payload.reportId;
      const message = mailFor(payload, supportTo, referenceId, now());
      try {
        await mailer.send(message);
        await audit.commitDelivery(payload.reportId, referenceId, now());
      } catch (error) {
        await audit.release(payload.reportId);
        throw error;
      }
      return respond(response, 202, { referenceId, duplicate: false });
    } catch (error) {
      const status = error instanceof FeedbackError ? error.status : 500;
      const code = error instanceof FeedbackError ? error.code : "delivery_unavailable";
      return respond(response, status, { error: code });
    }
  });
  server.requestTimeout = REQUEST_TIMEOUT_MS;
  server.headersTimeout = HEADERS_TIMEOUT_MS;
  server.keepAliveTimeout = KEEP_ALIVE_TIMEOUT_MS;
  return server;
}

function clientAddress(request, trustLoopbackProxy) {
  const direct = request.socket.remoteAddress ?? "unknown";
  if (!trustLoopbackProxy || !isLoopback(direct)) return direct;
  const forwarded = request.headers["x-forwarded-for"];
  const first = (Array.isArray(forwarded) ? forwarded[0] : forwarded)?.split(",")[0]?.trim();
  return first && isIP(first) !== 0 ? first : direct;
}

function isLoopback(address) {
  return address === "127.0.0.1" || address === "::1" || address === "::ffff:127.0.0.1";
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
  const errorCode = raw.errorCode == null || raw.errorCode === "" ? null : raw.errorCode;
  if (
    errorCode !== null &&
    (typeof errorCode !== "string" || !/^[A-Za-z0-9_.-]{1,96}$/.test(errorCode))
  ) {
    throw new FeedbackError(400, "invalid_error_code");
  }
  const report = raw.reportZipBase64 == null ? null : decodeAndValidateReport(raw.reportZipBase64);
  return { type, message, reportId, platform, version, email, errorCode, report };
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
    // The support inbox receives this attachment rather than extracting it, but requiring both
    // ZIP name records to agree prevents a central-directory-only allow-list from accepting a
    // misleading local header if the report is later inspected with another tool.
    const local = bytes.readUInt32LE(at + 42);
    if (local + 30 > at || bytes.readUInt32LE(local) !== 0x04034b50) {
      throw new FeedbackError(400, "invalid_report");
    }
    const localFlags = bytes.readUInt16LE(local + 6);
    const localMethod = bytes.readUInt16LE(local + 8);
    const localCompressed = bytes.readUInt32LE(local + 18);
    const localUncompressed = bytes.readUInt32LE(local + 22);
    const localNameLength = bytes.readUInt16LE(local + 26);
    const localExtraLength = bytes.readUInt16LE(local + 28);
    const localNameEnd = local + 30 + localNameLength + localExtraLength;
    const localName = bytes.subarray(local + 30, local + 30 + localNameLength).toString("utf8");
    if (
      localNameEnd > at ||
      localFlags !== flags ||
      localMethod !== method ||
      localCompressed !== compressed ||
      localUncompressed !== uncompressed ||
      localName !== name
    ) {
      throw new FeedbackError(400, "invalid_report");
    }
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
      `Основная ошибка: ${payload.errorCode ?? "не указана"}`,
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
