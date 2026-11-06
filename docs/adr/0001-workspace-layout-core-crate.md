# ADR-0001: Workspace layout — spec section 4 plus `watchdog-core`

- Status: Accepted
- Date: 2026-11-06

## Context

TASK.md §4 proposes the crate layout. Several concerns are needed by almost every
crate before any protocol or device code exists: the layered config system (M1-T02), the
data-directory layout (M1-T03), the logging setup with secret redaction (M1-T04) and the
shared error taxonomy (M1-T05).

## Options considered

1. Put these in the `watchdog` host binary crate; other crates re-implement or depend on
   the binary (impossible for libraries) or duplicate code (rejected).
2. Put them in `watchdog-api` (rejected: `api` is an integration surface, not a base).
3. Add a small `watchdog-core` crate holding exactly these cross-cutting modules.

## Decision

Option 3. `crates/core` (`watchdog-core`) contains `config`, `paths`, `logging`, `error`.
Nothing else is allowed to accrete into it; feature code lives in the crate that owns it.

## Consequences

- All crates can use the same error taxonomy and logging without cycles.
- One more crate to version, but it changes rarely and is small (~1.3 kLoC).
- If a future milestone needs heavy config consumers (e.g. typed settings per subsystem),
  they belong in their own crates, not in `core`.
