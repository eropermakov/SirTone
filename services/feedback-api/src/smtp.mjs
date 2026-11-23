import { connect as connectNet } from "node:net";
import { connect as connectTls } from "node:tls";

/**
 * Narrow SMTP submission transport. Production uses implicit TLS; plaintext is allowed solely
 * for a localhost test relay. The destination remains fixed by the Feedback API, never client input.
 */
export class SmtpMailer {
  constructor({ host, port = 465, secure = true, from, allowInsecureLocal = false }) {
    if (!host || !from) throw new TypeError("SMTP host and from are required");
    if (!secure && !allowInsecureLocal) throw new TypeError("plaintext SMTP is test-only");
    if (!secure && !["127.0.0.1", "::1", "localhost"].includes(host)) {
      throw new TypeError("plaintext SMTP is only allowed for localhost");
    }
    this.options = { host, port, secure, from };
  }

  async send(message) {
    const socket = await connect(this.options);
    const client = new Client(socket);
    try {
      await client.expect(220);
      await client.command("EHLO sirtone-feedback", 250);
      await client.command(`MAIL FROM:<${this.options.from}>`, 250);
      await client.command(`RCPT TO:<${message.to}>`, 250);
      await client.command("DATA", 354);
      await client.command(`${mime({ ...message, from: this.options.from })}\r\n.`, 250);
      await client.command("QUIT", 221);
    } finally {
      socket.end();
    }
  }
}

function connect({ host, port, secure }) {
  return new Promise((resolve, reject) => {
    const socket = secure
      ? connectTls({ host, port, servername: host })
      : connectNet({ host, port });
    socket.once("error", reject);
    socket.once(secure ? "secureConnect" : "connect", () => {
      socket.off("error", reject);
      resolve(socket);
    });
  });
}

class Client {
  constructor(socket) {
    this.socket = socket;
    this.buffer = "";
    this.waiters = [];
    this.replies = [];
    socket.setEncoding("utf8");
    socket.on("data", (part) => this.receive(part));
    socket.on("error", (error) => this.fail(error));
    socket.on("close", () => this.fail(new Error("SMTP connection closed")));
  }

  receive(part) {
    this.buffer += part;
    let at = this.buffer.indexOf("\r\n");
    while (at >= 0) {
      const line = this.buffer.slice(0, at);
      this.buffer = this.buffer.slice(at + 2);
      if (/^\d{3} /.test(line)) {
        const code = Number(line.slice(0, 3));
        const waiter = this.waiters.shift();
        if (waiter) waiter.resolve(code);
        else this.replies.push(code);
      }
      at = this.buffer.indexOf("\r\n");
    }
  }

  fail(error) {
    while (this.waiters.length) this.waiters.shift().reject(error);
  }

  expect(expected) {
    return new Promise((resolve, reject) => {
      const waiter = {
        resolve: (code) =>
          code === expected
            ? resolve()
            : reject(new Error(`SMTP expected ${expected}, got ${code}`)),
        reject,
      };
      const reply = this.replies.shift();
      if (reply !== undefined) waiter.resolve(reply);
      else this.waiters.push(waiter);
    });
  }

  async command(line, expected) {
    const response = this.expect(expected);
    this.socket.write(`${line}\r\n`);
    await response;
  }
}

function mime(message) {
  const headers = [
    `From: ${"SirTone Feedback"} <${message.from ?? "noreply@example.invalid"}>`,
    `To: ${message.to}`,
    `Subject: ${message.subject.replace(/[\r\n]/g, " ")}`,
    "MIME-Version: 1.0",
  ];
  if (message.replyTo) headers.push(`Reply-To: ${message.replyTo}`);
  if (!message.attachment)
    return `${headers.join("\r\n")}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n${message.text.replace(/\r?\n/g, "\r\n")}`;
  const boundary = "sirtone-feedback-boundary";
  headers.push(`Content-Type: multipart/mixed; boundary="${boundary}"`);
  const attachment = message.attachment.bytes.toString("base64").replace(/.{76}/g, "$&\r\n");
  return [
    headers.join("\r\n"),
    "",
    `--${boundary}`,
    "Content-Type: text/plain; charset=utf-8",
    "",
    message.text.replace(/\r?\n/g, "\r\n"),
    `--${boundary}`,
    `Content-Type: ${message.attachment.contentType}; name="${message.attachment.filename}"`,
    "Content-Transfer-Encoding: base64",
    `Content-Disposition: attachment; filename="${message.attachment.filename}"`,
    "",
    attachment,
    `--${boundary}--`,
  ].join("\r\n");
}
