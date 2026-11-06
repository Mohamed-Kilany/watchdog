# Security

## Threat model (M1 baseline)

| Asset | Threat | Mitigation |
|---|---|---|
| ADB key pair (`<data_dir>/keys/adbkey`) | leaked → device shell access | 0600 files; excluded from VCS; documented backup/revoke |
| Bearer token (all ports) | interception, brute force, log leakage | constant-time compare; TLS recommended on LAN; redaction tests; `config show` hides it |
| Media library | path traversal, unauthorized read/delete | store under data dir; ids validated; export via explicit ids; deletion gated in MCP |
| Integration API (7702) | abuse from LAN when `0.0.0.0` | bearer token; CORS off by default; request size limits |
| MCP server (7701) | destructive tool invocation | destructive tools require explicit `confirm` params; rate limits; image payload caps |
| Log files | secret leakage | streaming redaction of registered secrets (see `logging.rs`); pairing codes registered when received |
| Supply chain | compromised dependency | `cargo deny` (advisories + licenses), lockfile committed, minimal deps |
| Wireless debugging | anyone on LAN pairs | pairing code single-use 6-digit; TLS after pairing; recommend USB or trusted VLAN; disable when idle |

## Defaults

- All three ports bind **127.0.0.1**. LAN exposure requires explicit `ui_listen`/
  `mcp_listen`/`api_listen` = `0.0.0.0` (or per-port `--*-listen`), and `watchdog doctor`
  prints a loud warning when a non-loopback bind lacks a token.
- No tokens are ever logged; `watchdog config show`, error bodies and MCP outputs
  redact them (unit-tested).
- Media ids are ULIDs; file paths are never taken from clients.

## Network exposure recipes

**Safe default** (local only): nothing to configure.

**LAN mode**:

```toml
# watchdog.toml
ui_listen = "0.0.0.0"
mcp_listen = "0.0.0.0"
api_listen = "0.0.0.0"
token = "generate-a-long-random-token"
```

Clients pass `Authorization: Bearer …`. Rotate with `watchdog config` (new token in
env/file; restart) — a dedicated `token rotate` command lands with M9-T04.

**TLS on LAN** (M10-T02): optional `--tls-cert/--tls-key` or self-signed generation;
documented with fingerprint verification for impresario clients. Until M10-T02, LAN TLS
means reverse proxy (Caddy/nginx) in front of watchdog.

## Known advisories and accepted risks

- **RUSTSEC-2023-0071** (`rsa` crate — Marvin Attack, timing sidechannel): no patched
  version exists. watchdog uses `rsa` solely for the ADB AUTH handshake, which mandates
  RSA-PKCS#1-v1_5 with SHA-1 by protocol definition. Signing happens once per device
  connection. The attack requires an attacker who can observe very precise timing across
  many signatures; practically it targets network-visible services, not a localhost/LAN
  test tool. The advisory is explicitly ignored in `deny.toml` with this rationale and is
  re-evaluated in M10 (or whenever a patched `rsa` release lands). If you have a stricter
  threat model, run watchdog on isolated VLANs and keep the bearer token enabled.

## Reporting

Security issues: open a private GitHub security advisory on the repository, or email the
maintainer listed in the repo profile. Do not file public issues for vulnerabilities.
