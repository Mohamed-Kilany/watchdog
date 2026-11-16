#!/usr/bin/env node
/**
 * Enforce the gzip bundle budget for the static web UI.
 *
 * Budgets are deliberately small: the UI is embedded in the host binary and
 * must work offline. See docs/ui.md and docs/adr/0007-web-ui-components.md.
 */
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const dist = fileURLToPath(new URL("../dist/", import.meta.url));
const budgets = {
  js: 64 * 1024,
  css: 16 * 1024,
  html: 8 * 1024,
  file: 48 * 1024,
  total: 96 * 1024,
};

if (!existsSync(dist)) {
  console.error(`bundle budget: ${dist} does not exist; run "npm run build" first`);
  process.exitCode = 1;
} else {
  const files = walk(dist);
  const groups = { js: 0, css: 0, html: 0, other: 0 };
  const rows = [];
  let failed = false;

  for (const file of files) {
    const name = relative(dist, file).replaceAll("\\", "/");
    if (extname(name) === ".map") {
      console.error(`bundle budget: source map ${name} must not ship`);
      failed = true;
      continue;
    }
    const size = gzipSync(readFileSync(file)).length;
    const kind = extname(name) === ".js"
      ? "js"
      : extname(name) === ".css"
        ? "css"
        : extname(name) === ".html"
          ? "html"
          : "other";
    groups[kind] += size;
    rows.push({ name, kind, size });
    if (size > budgets.file) {
      console.error(
        `bundle budget: ${name} is ${kib(size)} KiB gzip, over the ${kib(budgets.file)} KiB per-file budget`,
      );
      failed = true;
    }
  }

  const total = Object.values(groups).reduce((sum, value) => sum + value, 0);
  console.log("web bundle budget (gzip):");
  for (const row of rows.sort((a, b) => b.size - a.size)) {
    console.log(`  ${row.name.padEnd(34)} ${kib(row.size).padStart(8)} KiB  ${row.kind}`);
  }
  for (const kind of ["js", "css", "html"]) {
    console.log(
      `  ${kind.padEnd(34)} ${kib(groups[kind]).padStart(8)} KiB  (budget ${kib(budgets[kind])} KiB)`,
    );
    if (groups[kind] > budgets[kind]) failed = true;
  }
  console.log(
    `  ${"total".padEnd(34)} ${kib(total).padStart(8)} KiB  (budget ${kib(budgets.total)} KiB)`,
  );
  if (groups.other > 0) {
    console.log(`  ${"other assets".padEnd(34)} ${kib(groups.other).padStart(8)} KiB`);
  }
  if (total > budgets.total) failed = true;

  if (failed) {
    console.error("bundle budget: FAILED");
    process.exitCode = 1;
  } else {
    console.log("bundle budget: ok");
  }
}

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? walk(path) : [path];
  });
}

function kib(bytes) {
  return (bytes / 1024).toFixed(1);
}
