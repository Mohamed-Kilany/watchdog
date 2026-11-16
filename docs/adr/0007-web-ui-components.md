# ADR-0007: Web UI stack and component model

- Status: Accepted
- Date: 2026-11-16

## Context

M4-T12 requires host-side control components: sliders with numeric entry, auto/manual
toggles, lock buttons with live badges, regions drawn on the viewfinder, reset-to-auto and
per-control "actual vs requested" indicators, tested with Playwright against a fake device.
M6-T01 adds the constraints for the surrounding app: a static build, **no CDN and no
external fonts**, embedded in the host binary, working offline, dark/light themes,
keyboard accessible and responsive down to tablet width.

The components are consumed by M6 but are also the first place where the host UI is
implemented, so the stack choice must not lock the project into a runtime that is hard to
embed or test.

## Decision

1. **Standards-based Web Components.** Each control is a custom element with Shadow DOM
   and CSS custom properties. There is no UI framework and no runtime dependency; the
   browser's own element lifecycle, events and accessibility primitives are enough for
   this component set.
2. **TypeScript + Vite.** Strict TypeScript, ES2022 output, a static `ui/dist` build with
   relative asset URLs (`base: "./"`). The build has no CDN imports, no remote fonts and
   no runtime network dependency; the font stack is the platform UI font.
3. **Components and contracts.** `wd-slider` (range + numeric entry + actual marker),
   `wd-auto-toggle`, `wd-lock-button`, `wd-region-overlay`, `wd-actual`, `wd-select`,
   `wd-reset` and `wd-control-panel`. Components own presentation and emit composed
   custom events (`wd-commit`, `wd-auto-change`, `wd-lock-change`, `wd-region-change`,
   `wd-reset`); the panel translates them to `wd-control-change` / `wd-control-reset`.
   They never call the device directly, so M6 can wire them to the real UI API.
4. **Fake device first.** `ui/src/fake-device.ts` mirrors the host `state` map and the
   Android service's implications (manual exposure turns AE off, focus distance turns AF
   off, locks freeze the current actual). The demo and Playwright suite use it; no
   emulator or real phone is required for M4-T12.
5. **Accessibility and layout.** Native range/number/select controls, explicit ARIA
   roles/names for switches and lock buttons, a focusable region overlay with arrow-key
   move, Shift+arrow resize, Enter apply and Escape cancel. Dark/light themes use a
   `data-theme` attribute and inherited custom properties. The layout collapses to one
   column at 1000 px and remains usable at 768 px.
6. **Bundle budget.** Enforced in CI by `ui/scripts/check-bundle-size.mjs` on the gzip
   size of `ui/dist`:

   | Part | Budget |
   |---|---|
   | JavaScript | 64 KiB |
   | CSS | 16 KiB |
   | HTML | 8 KiB |
   | Any single file | 48 KiB |
   | Total | 96 KiB |

   Source maps must not ship. The budget is intentionally below the host embedding
   budget and is documented in `docs/ui.md`.

M6-T01 will add `rust-embed`, ETag/compression and the real app shell around this
component layer; those server-side concerns are deliberately outside this ADR.

## Consequences

- No framework runtime or third-party UI code ships; the bundle stays small and the
  embedding path is simple.
- Attribute/property plumbing is manual. Components must keep their setters idempotent
  and avoid re-rendering the panel on every live state update.
- Shadow DOM isolates component styles, but tests and consumers must use open shadow
  roots; Playwright's locators pierce them.
- The CI `web` job needs Node 22 and a Chromium download; it is independent of the Rust,
  Android and device jobs.
- Adding pages in M6 will consume the budget quickly. The budget is a hard gate and must
  be revisited with measured sizes when the full app shell lands, not silently raised.
