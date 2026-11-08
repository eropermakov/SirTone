import { copyFileSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";

// The phone's visualiser page (stage 8, D-148): one classic script (IIFE) with Butterchurn and
// the presets inlined — WKWebView loads it from the app bundle over file://, where module scripts
// and lazy chunks are unreliable. Output goes straight into the iPhone app's resources.
const out = fileURLToPath(new URL("../mobile-ios/Sources/SirTone/Resources/viz", import.meta.url));

export default defineConfig({
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  build: {
    target: "safari17",
    outDir: out,
    emptyOutDir: true,
    sourcemap: false,
    lib: {
      entry: fileURLToPath(new URL("./src/mobile-viz/main.ts", import.meta.url)),
      name: "SirToneViz",
      formats: ["iife"],
      fileName: () => "mobile-viz.js",
    },
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
  plugins: [
    {
      name: "mobile-viz-html",
      closeBundle() {
        copyFileSync(
          fileURLToPath(new URL("./mobile-viz.html", import.meta.url)),
          `${out}/index.html`,
        );
      },
    },
  ],
});
