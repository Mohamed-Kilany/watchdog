# MCP server

> Status: skeleton (M1). Full tool reference with JSON schemas lands with M9; the
> normative descriptions below are stable. Images only for AI consumption — video is
> analyzed by the engine; the AI receives the **report plus selected frames**.

## Transports

- Streamable HTTP on `127.0.0.1:7701/mcp` (configurable `mcp_listen`/`mcp_port`)
- `watchdog mcp --stdio` for clients that spawn the binary

Localhost-only by default; bearer token honored when LAN is enabled (see
[security.md](security.md)).

## Client configuration examples

**Claude Desktop / generic MCP clients (stdio):**

```json
{
  "mcpServers": {
    "watchdog": {
      "command": "watchdog",
      "args": ["mcp", "--stdio"]
    }
  }
}
```

**HTTP (any streamable-HTTP client):** endpoint `http://127.0.0.1:7701/mcp`, header
`Authorization: Bearer <token>` (only when LAN auth is enabled).

## Planned tools (M9, minimum set)

| Tool | Purpose | Destructive gate |
|---|---|---|
| `list_devices` | USB + discovered LAN devices | no |
| `connect_device` / `provision_device` | connect, zero-touch provision | provision is mutating but not destructive |
| `get_device_state` | full control state (actual values) | no |
| `get_camera_controls` | capability descriptor: ranges, steps, supported flags | no |
| `set_camera_controls` | batch set; per-control ack/error | no |
| `apply_preset` | apply a named preset atomically | no |
| `lock_white_balance` / `lock_focus` / `lock_exposure` | first-class locks with state badges | no |
| `capture_image` | returns image content (size-limited; `max_width` downscale) | no |
| `capture_sequence` | N frames, interval or as-fast-as-possible | no |
| `get_live_frame` | one downscaled JPEG of the current viewfinder | no |
| `start_recording` / `stop_recording` | video lifecycle | no |
| `list_media` / `get_media_image` | library index + image fetch | no |
| `analyze_image` | run image analyzers; returns report JSON | no |
| `diff_images` | two-image comparison report | no |
| `analyze_video` | engine analysis; returns report + key frames (never the video) | no |
| `run_schedule_now` | trigger a job | no |
| `delete_media` *(destructive)* | removes media | requires `confirm: true` + explicit ids |
| `restore_device` *(destructive)* | reverts provisioning settings | requires `confirm: true` |

Image payload caps and per-tool rate limits apply; oversized requests are rejected with
`PAYLOAD_TOO_LARGE` and a "what to do next" message.

## Resources

- `watchdog://media/index` — current media index (URIs for items)
- `watchdog://reports/latest` — latest analysis report
- `watchdog://reports/{id}` — a specific report

## LLM-targeted descriptions

Each tool description is written for an LLM (when to use, what it returns, failure
explanations, e.g. "ISO rejected because the device caps at 12800 — call
`get_camera_controls` for exact `iso.max`"). See source `crates/mcp` during M9.
