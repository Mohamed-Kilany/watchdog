# impresario ↔ watchdog integration

> Status: skeleton (M1). This page + `docs/api.md` are the contract; the reference client
> lands with M9-T07. The impresario DSL mapping below is a **proposal only** — no changes
> are made inside impresario as part of this work.

## What impresario needs from watchdog

1. Start a capture with a **trigger timestamp** and **correlation id** (e.g. the moment
   the UI test tapped "Play").
2. Wait for completion (WebSocket event or poll).
3. Fetch the verdict JSON (pass/fail per rule + overall).
4. Fetch evidence frames for failing rules.

## Contract

### 1. Start analysis run

```http
POST /api/v1/analyses
Idempotency-Key: 9f0f3d2a-…
Content-Type: application/json

{
  "device_id": "<watchdog device id>",
  "source": { "media_id": "<id from a capture you started>" },
  "trigger_ms": 1730900000123,
  "correlation_id": "it-4821",
  "profile": "ott-default",
  "rules": { "max_black_ms": 500, "max_buffering_events": 0 }
}
```

`202 Accepted` → `{ "analysis_id": "a-…", "correlation_id": "it-4821", "status": "queued" }`.
Alternatively pass `"capture": { "type": "record", "duration_ms": 30000 }` to have watchdog
record first, then analyze.

### 2. Wait for completion

- WebSocket `/api/v1/events`: wait for `{"event":"analysis.finished","data":{"analysis_id":"a-…"}}`
  — recommended; or
- Poll `GET /api/v1/analyses/{id}` until `status: "finished"` (timeout impresario-side, e.g. 120 s).

### 3. Fetch verdict

```http
GET /api/v1/analyses/a-…?fields=verdict,rules
```

```json
{
  "schema": "watchdog.report.v1",
  "correlation_id": "it-4821",
  "trigger_ms": 1730900000123,
  "verdict": "fail",
  "rules": [
    { "rule": "max_buffering_events", "actual": 2, "limit": 0, "pass": false },
    { "rule": "max_black_ms", "actual": 0, "limit": 500, "pass": true }
  ],
  "startup_ms": 1834,
  "summary": "2 buffering events (longest 1.8 s); first at 00:12; no black screens."
}
```

### 4. Evidence frames

`GET /api/v1/analyses/a-…/evidence/{finding_index}` → JPEG. Attach to the test report;
impresario stores them next to its artifacts.

## Worked example (curl)

```bash
# 1) ensure device + recording
curl -s -X POST $WD/api/v1/devices/$DEV/capture/record -d '{"duration_ms":30000}'
# ... test performs the tap; note the trigger time as trigger_ms
# 2) analyze with correlation
curl -s -X POST $WD/api/v1/analyses \
  -H 'Idempotency-Key: it-4821' \
  -d '{"device_id":"'$DEV'","source":{"media_id":"'$MEDIA'"},"trigger_ms":'$TRIGGER_MS',"correlation_id":"it-4821","profile":"ott-default"}'
# 3) wait + fetch verdict
curl -s $WD/api/v1/analyses/$AID | jq '.verdict, .rules[] | select(.pass == false)'
```

A minimal Rust reference client lands in `clients/impresario-ref` (M9-T07).

## Proposed impresario DSL mapping (proposal only)

```
check watchdog("ott-default", on: "device-1", record_secs: 30, trigger: "play-tap") {
    assert $.verdict == "pass"
    assert $.startup_ms < 5000
}
```

Semantics: the `check` records around a named step (`trigger`), attaches correlation id
`impresario:<test>:<step>`, and asserts on the JSON verdict. This is a **proposal** for an
impresario RFC; nothing here modifies impresario. Implementation discussion belongs in the
impresario repo/design doc.
