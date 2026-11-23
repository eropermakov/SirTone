import assert from "node:assert/strict";
import { once } from "node:events";
import { createServer } from "node:net";
import test from "node:test";
import { createFeedbackServer, MemoryMailer } from "../src/server.mjs";
import { SmtpMailer } from "../src/smtp.mjs";

const REPORT_ID = "018f1bb2-3b91-7cc5-8cf0-6c0ce0e5aa01";

async function withServer(run) {
  const mailer = new MemoryMailer();
  const server = createFeedbackServer({
    mailer,
    supportTo: "sir.kov.mail@gmail.com",
    now: () => 1_700_000_000_000,
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const { port } = server.address();
  try {
    await run({ mailer, url: `http://127.0.0.1:${port}/v1/feedback/reports` });
  } finally {
    server.close();
    await once(server, "close");
  }
}

function reportZipBase64() {
  // A prebuilt stored ZIP whose central directory lists the six allowed empty report files.
  // The server validates metadata only and never extracts it.
  const names = [
    "summary.txt",
    "manifest.json",
    "environment.json",
    "events.jsonl",
    "errors.json",
    "README.txt",
  ];
  const chunks = [];
  let offset = 0;
  for (const name of names) {
    const n = Buffer.from(name);
    const header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(n.length, 26);
    chunks.push(header, n);
    offset += header.length + n.length;
  }
  const central = [];
  let local = 0;
  for (const name of names) {
    const n = Buffer.from(name);
    const header = Buffer.alloc(46);
    header.writeUInt32LE(0x02014b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(20, 6);
    header.writeUInt16LE(n.length, 28);
    header.writeUInt32LE(local, 42);
    central.push(header, n);
    local += 30 + n.length;
  }
  const centralBytes = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(names.length, 8);
  end.writeUInt16LE(names.length, 10);
  end.writeUInt32LE(centralBytes.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...chunks, centralBytes, end]).toString("base64");
}

test("accepts an opted-in report and produces a readable fixed-recipient email", async () => {
  await withServer(async ({ mailer, url }) => {
    const response = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        type: "bug",
        message: "Не включается после смены устройства",
        email: "reader@example.test",
        reportId: REPORT_ID,
        platform: "Windows",
        version: "0.2.7",
        reportZipBase64: reportZipBase64(),
      }),
    });
    assert.equal(response.status, 202);
    assert.deepEqual(await response.json(), { referenceId: REPORT_ID, duplicate: false });
    assert.equal(mailer.messages.length, 1);
    assert.equal(mailer.messages[0].to, "sir.kov.mail@gmail.com");
    assert.equal(mailer.messages[0].replyTo, "reader@example.test");
    assert.match(mailer.messages[0].subject, /\[BUG\] \[Windows\] \[0\.2\.7\]/);
    assert.match(mailer.messages[0].text, /Не включается/);
    assert.equal(mailer.messages[0].attachment?.bytes.length > 0, true);
  });
});

test("deduplicates an identical report without another email", async () => {
  await withServer(async ({ mailer, url }) => {
    const body = JSON.stringify({
      type: "question",
      message: "Как обновиться?",
      reportId: REPORT_ID,
      platform: "iOS",
      version: "0.2.5",
    });
    for (const expected of [202, 200]) {
      const response = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body,
      });
      assert.equal(response.status, expected);
    }
    assert.equal(mailer.messages.length, 1);
  });
});

test("rejects malformed reports and does not create email", async () => {
  await withServer(async ({ mailer, url }) => {
    const response = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        type: "bug",
        message: "Тест",
        reportId: REPORT_ID,
        platform: "Windows",
        version: "0.2.7",
        reportZipBase64: Buffer.from("not a zip").toString("base64"),
      }),
    });
    assert.equal(response.status, 400);
    assert.equal(mailer.messages.length, 0);
  });
});

test("rate limits a sixth request from the same client", async () => {
  await withServer(async ({ url }) => {
    for (let i = 0; i < 6; i += 1) {
      const reportId = `018f1bb2-3b91-7cc5-8cf0-6c0ce0e5aa0${i}`;
      const response = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          type: "other",
          message: "Тест",
          reportId,
          platform: "Windows",
          version: "0.2.7",
        }),
      });
      assert.equal(response.status, i < 5 ? 202 : 429);
    }
  });
});

test("SMTP transport delivers a test email to a local simulator", async () => {
  let received = "";
  const relay = createServer((socket) => {
    socket.setEncoding("utf8");
    socket.write("220 local test relay\r\n");
    let dataMode = false;
    socket.on("data", (chunk) => {
      received += chunk;
      if (dataMode && received.includes("\r\n.\r\n")) {
        dataMode = false;
        socket.write("250 queued\r\n");
      } else if (/DATA\r\n$/.test(chunk)) {
        dataMode = true;
        socket.write("354 send data\r\n");
      } else if (/QUIT\r\n$/.test(chunk)) {
        socket.write("221 bye\r\n");
      } else if (!dataMode) {
        socket.write("250 ok\r\n");
      }
    });
  });
  relay.listen(0, "127.0.0.1");
  await once(relay, "listening");
  try {
    const mailer = new SmtpMailer({
      host: "127.0.0.1",
      port: relay.address().port,
      secure: false,
      from: "noreply@example.test",
      allowInsecureLocal: true,
    });
    await mailer.send({
      to: "sir.kov.mail@gmail.com",
      replyTo: null,
      subject: "[SIRTONE] test",
      text: "hello",
      attachment: null,
    });
    assert.match(received, /RCPT TO:<sir\.kov\.mail@gmail\.com>/);
    assert.match(received, /Subject: \[SIRTONE\] test/);
  } finally {
    relay.close();
    await once(relay, "close");
  }
});
