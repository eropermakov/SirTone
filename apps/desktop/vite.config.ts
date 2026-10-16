import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri expects a fixed dev port (devUrl in tauri.conf.json).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  build: { target: "chrome120", sourcemap: true },
});
