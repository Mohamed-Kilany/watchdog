import { defineConfig } from "vite";

// The bundle is embedded in the host binary and served from the host's own
// origin root (http://<host>:7700/), so absolute asset URLs also resolve
// correctly when the server returns the app shell for a client-side route.
export default defineConfig({
  base: "/",
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    assetsInlineLimit: 0,
  },
});
