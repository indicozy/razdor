# razdor-wire

The shared API types of Razdor's analytics and cloud saves. The game and the server (`server/`, later)
both depend on this crate, so a body's shape and its limits are written once. Plan and specs:
`docs/superpowers/plans/2026-11-22-analytics-and-cloud.md`, `docs/superpowers/specs/2026-11-22-*.md`.

## What is in it

- `analytics`: the ping, the install delete, the play events (`EventBatch`, `EventKind`), `PerfReport`,
  `CrashReport` and its save header, `BattleUpload` (its record follows as the body), the verifier's
  `Verdict`.
- `cloud`: `AccountNew` and `AccountCreated`, `LinkIssued` and `LinkRedeem`/`LinkRedeemed`, `SaveMeta`,
  `SaveEntry`, `SaveList`, `SavePut`, `Marks`, `SyncedSettings`, `HubList`, `BenchReport`.
- `ids`: `InstallId`, `AccountKey`, `LinkCode` (typed, checked).
- `paths`: the API paths and the two header names. `limits`: every size, count and rate limit.
- `frame`: the framed body (a JSON header, then raw bytes) for battle records, crash saves and save puts.
- `SCHEMA` (1): every body carries `schema`; a body with 0 or a newer schema is refused.

## Rules

- `validate()` (the `Validate` trait) checks a body against `limits`. It is called on both sides.
- No type denies unknown fields: a newer peer's extra fields are ignored.
- Enums are strict: an unknown variant fails to decode. A server reads an event batch one event at a
  time and drops the unknown kinds, so a new event kind does not void a whole batch.
- No I/O, no async runtime, no macroquad. serde and serde_json only.

## Decisions

- Battle records and save files are not base64 in JSON: they travel as the body of a framed request
  (`frame::encode(&header, &bytes)`). The header carries the length and the sha256; the receiver checks both.
- Limits that the specs give as rates (pings per day, events per hour, the ban steps) are constants here
  and enforced by the server. The per-install and per-IP request rates are not in the specs, so the
  server sets them.

## Tests

`cargo test --profile fasttest -p razdor-wire` (round trips, validation, forward compatibility, framing).
