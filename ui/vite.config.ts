import { defineConfig } from "vite";

// The bundle is embedded in the host binary and served from the host's own
// origin. Relative asset URLs keep it usable under a sub-path as well.
export default defineConfig({
  base: "./",
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    assetsInlineLimit: 0,
  },
});
