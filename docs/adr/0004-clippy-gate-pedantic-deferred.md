# ADR-0004: CI lint gate — clippy::all denied; pedantic deferred

- Status: Accepted
- Date: 2026-11-06

## Context

TASK.md §6 requires `cargo clippy -D warnings` in CI. `clippy::pedantic` enabled wholesale
produces a large number of style-level findings that would gate early milestones without
product value, and `warnings` from rustc (e.g. missing docs) would also become hard errors.

## Decision

- `[workspace.lints.clippy] all = deny` — correctness, suspicious, perf and style lints in
  clippy's default set are denied in CI (`cargo clippy --workspace --all-targets --locked
  -- -D warnings`).
- `pedantic` is **not** enabled yet; it is enabled per crate during the M10 hardening
  milestone, crate by crate, with the fallout fixed in the same PRs.
- `rust.missing_docs` is not denied (review-enforced instead).
- `unsafe_code` stays `warn`; any `unsafe` requires an ADR.

## Consequences

- CI keeps a real, honest gate from day one; no nightly clippy churn.
- Some style lints (e.g. `must_use`) are allowed selectively in the lints table.
- Deferred pedantic work is tracked in M10 (see docs/PROGRESS.md).
