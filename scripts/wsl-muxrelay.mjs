// Dev only: relay the Windows Apple usbmuxd (127.0.0.1:27015) to the WSL-facing host address, so
// xtool and libimobiledevice in WSL see the iPhone without taking USB from Windows (docs/11 §5).
// Usage: node scripts/wsl-muxrelay.mjs <host address in the WSL network>
import net from "node:net";

const host = process.argv[2];
if (!host) {
  console.error("usage: node scripts/wsl-muxrelay.mjs <host-ip>");
  process.exit(2);
}
net
  .createServer((c) => {
    const u = net.connect(27015, "127.0.0.1");
    c.pipe(u).pipe(c);
    c.on("error", () => u.destroy());
    u.on("error", () => c.destroy());
  })
  .listen(27015, host, () => console.log(`relay on ${host}:27015`));
