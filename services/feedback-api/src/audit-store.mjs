import { createHash, randomBytes } from "node:crypto";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";

const HOUR_MS = 60 * 60 * 1000;
const DELIVERY_TTL_MS = 30 * 24 * HOUR_MS;

/** Volatile store for tests and local runs. It intentionally holds no request body or email. */
export class MemoryAuditStore {
  deliveries = new Map();
  attempts = new Map();

  async enforceRateLimit(ip, now, limit) {
    const recent = (this.attempts.get(ip) ?? []).filter((time) => time >= now - HOUR_MS);
    if (recent.length >= limit) return false;
    recent.push(now);
    this.attempts.set(ip, recent);
    return true;
  }

  async reserve(reportId, now) {
    const entry = this.deliveries.get(reportId);
    if (entry && entry.expiresAt > now) {
      return entry.pending
        ? { kind: "pending" }
        : { kind: "delivered", referenceId: entry.referenceId };
    }
    this.deliveries.set(reportId, { pending: true, expiresAt: now + 5 * 60 * 1000 });
    return { kind: "new" };
  }

  async commitDelivery(reportId, referenceId, now) {
    this.deliveries.set(reportId, { referenceId, expiresAt: now + DELIVERY_TTL_MS });
  }

  async release(reportId) {
    if (this.deliveries.get(reportId)?.pending) this.deliveries.delete(reportId);
  }
}

/**
 * Small persistent TTL audit store for one API instance. It records only report IDs, delivery
 * timestamps, and HMAC-like salted IP hashes — never feedback text, contact email, ZIP bytes or
 * SMTP credentials. For horizontally scaled production, use an equivalent shared transactional
 * store at the deployment layer.
 */
export class FileAuditStore {
  constructor({ path, salt }) {
    if (!path) throw new TypeError("path is required");
    if (!salt || salt.length < 16)
      throw new TypeError("an audit salt of at least 16 characters is required");
    this.path = path;
    this.salt = salt;
    this.state = null;
    this.chain = Promise.resolve();
  }

  async enforceRateLimit(ip, now, limit) {
    return this.#mutate((state) => {
      const key = this.#ipKey(ip);
      const recent = (state.attempts[key] ?? []).filter((time) => time >= now - HOUR_MS);
      if (recent.length >= limit) {
        state.attempts[key] = recent;
        return false;
      }
      recent.push(now);
      state.attempts[key] = recent;
      return true;
    }, now);
  }

  async reserve(reportId, now) {
    return this.#mutate((state) => {
      const entry = state.deliveries[reportId];
      if (entry?.expiresAt > now) {
        return entry.pending
          ? { kind: "pending" }
          : { kind: "delivered", referenceId: entry.referenceId };
      }
      state.deliveries[reportId] = { pending: true, expiresAt: now + 5 * 60 * 1000 };
      return { kind: "new" };
    }, now);
  }

  async commitDelivery(reportId, referenceId, now) {
    await this.#mutate((state) => {
      state.deliveries[reportId] = { referenceId, expiresAt: now + DELIVERY_TTL_MS };
      return null;
    }, now);
  }

  async release(reportId) {
    await this.#mutate((state) => {
      if (state.deliveries[reportId]?.pending) delete state.deliveries[reportId];
      return null;
    }, Date.now());
  }

  #ipKey(ip) {
    return createHash("sha256").update(this.salt).update("\0").update(ip).digest("hex");
  }

  async #mutate(action, now) {
    const run = async () => {
      if (!this.state) this.state = await readState(this.path);
      prune(this.state, now);
      const value = action(this.state);
      await writeState(this.path, this.state);
      return value;
    };
    const result = this.chain.then(run, run);
    this.chain = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }
}

async function readState(path) {
  try {
    const candidate = JSON.parse(await readFile(path, "utf8"));
    if (candidate?.schemaVersion === 1 && candidate.deliveries && candidate.attempts)
      return candidate;
    throw new Error("feedback audit store has an unsupported schema");
  } catch (error) {
    if (error?.code === "ENOENT") {
      return { schemaVersion: 1, deliveries: {}, attempts: {} };
    }
    // Replacing a malformed store would forget delivered IDs and could send duplicate support
    // emails. Fail closed until an operator restores or deliberately rotates this tiny audit file.
    throw new Error("feedback audit store is unreadable", { cause: error });
  }
}

function prune(state, now) {
  for (const [id, entry] of Object.entries(state.deliveries)) {
    if (!entry || entry.expiresAt <= now) delete state.deliveries[id];
  }
  for (const [key, times] of Object.entries(state.attempts)) {
    const recent = Array.isArray(times)
      ? times.filter((time) => Number.isFinite(time) && time >= now - HOUR_MS)
      : [];
    if (recent.length) state.attempts[key] = recent;
    else delete state.attempts[key];
  }
}

async function writeState(path, state) {
  await mkdir(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = join(dirname(path), `.${randomBytes(12).toString("hex")}.tmp`);
  await writeFile(temporary, JSON.stringify(state), { mode: 0o600 });
  await rename(temporary, path);
}
