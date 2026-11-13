# ASD-Nexus M1 Verification Report

- **ASD Session**: `asd-20261113-104918-ln8l` (`LIGHT` track)
- **Core Runtime**: `agent-swarm-director` v2.6.0 (`file:../../code/asd-runtime` -> `C:\Users\yjsjs0987\Desktop\code\asd-runtime`)
- **Milestone Scope**: M1 (Clean-Room Orchestrator HTTP/SSE Server + Claude Code 1-Harness Web Chat UI)

---

## 1. Completed Tasks & Boundary Compliance

| Task | Scope | Verified Commit |
|------|-------|-----------------|
| `T1.1` | Clean-room workspace, `agent-swarm-director` v2.6.0 link, `src/shared/events.ts`, `src/server/sdk.ts`, `tests/sdk-surface.test.ts` | `8e24e345f650bbdbc1539ebbbf52a603144c1a0a` |
| `T2.1` | `node:http` orchestrator server (`src/server/normalizer.ts`, `src/server/app.ts`, `src/server/index.ts`, `tests/server-sse.test.ts`) | `50a49bf64355a1e20ca8ce6de0279c2477a2cf4a` |
| `T3.1` | Clean-room Vite + React browser UI (`src/web/**/*`) | `a51fb00f0df0094cb3bbb64ff4db4c0ec2ebeee7` |
| `T4.1` | Live Claude CLI E2E test (`tests/live-claude-m1.test.ts`), SSE event log, browser UI screenshot, verification report | Verified below |

### Core SDK Boundary Checklist (`SDK_SURFACE_CLEANUP_PLAN.md` §3)
Verified automatically by `tests/sdk-surface.test.ts`:
- Core runtime linked to `2.6.0`.
- Zero imports/usages of `startDashboardServer`, `agent-swarm-director/adapters/claude`, or removed v2.6.0 aliases/subpaths.
- Zero positional calls to `buildResumeHarnessCommand` and zero calls to `verifyPidIdentity`.
- Zero raw `child_process` CLI string assembly for harnesses in `src/` (uses Core `runDriverTurn`, `buildSpawnSpec`, `buildResumeHarnessCommand`, `discoverProjectCliSessions`, `promoteCliSessionToAsd`, `readHarnessTranscript`, `collectDashboardSnapshot`, `runSubagentControlAction`, `scanSubagentLogs`, `openProjectDb` read-only wrapper `withReadOnlyProjectDb`, and `handlePlan`).
- Zero modifications made to `asd-multiagent` or `asd-runtime`.

---

## 2. Live Claude Code E2E & Browser UI Evidence

### A. Automated Live CLI Integration (`tests/live-claude-m1.test.ts`)
Executed against real `createOrchestratorServer` with an isolated temporary project directory and real local `claude` CLI (zero fixed-sleep `setTimeout` assertions; event-driven HTTP SSE `ReadableStream` reading):
1. **Turn 1 (`POST /api/turn/stream`, `harness: "claude-code"`, `model: "haiku"`)**:
   - Created ASD session (`asd-...`) via `handlePlan` and streamed normalized `OrchestratorEvent` SSE frames (`status: "running"` -> `status(harnessSessionId)` -> `text` -> `usage` -> `status: "done"` with `exitCode: 0`, `timedOut: false`).
   - Confirmed exact reply token (`ACK_7492`) while planting secret token `NEXUS_SECRET_7492`.
2. **Turn 2 (`--resume`, same `sessionId`)**:
   - Preserved the exact `harnessSessionId` from Turn 1 via Core `runDriverTurn` + `buildSpawnSpec` `--resume`.
   - Recalled `NEXUS_SECRET_7492` from Turn 1 context.
3. **Session Discovery & 4-Category Transcript Restoration**:
   - `GET /api/sessions?projectRoot=...&harness=claude-code` returned the promoted ASD session (`isPromotedToAsd: true`).
   - `GET /api/transcript/claude-code/:id` restored Turn 1 and Turn 2 `user` and `assistant` messages via positional `readHarnessTranscript("claude-code", harnessSessionId, { limit: 300 })`.

### B. Live Browser Web Chat UI Capture (`docs/evidence/m1-chat-ui.png` & `docs/evidence/m1-sse-events.jsonl`)
Verified by serving the production `dist/web/` bundle from `createOrchestratorServer` inside Headless Microsoft Edge (`1440x960`) controlled via Chrome DevTools Protocol (`MutationObserver`-driven, zero fixed-sleep assertions):
- Entered `haiku` in `#claude-model-select` and sent interactive Turn 1 (`NEXUS_EVIDENCE_3141`) followed by Turn 2 (`--resume`) asking for the remembered token.
- Captured live SSE wire stream (`12` events in [`docs/evidence/m1-sse-events.jsonl`](file:///C:/Users/yjsjs0987/Desktop/Projects/ASD-Nexus/docs/evidence/m1-sse-events.jsonl)):
  - ASD Session: `asd-20261113-111504-m385`
  - Claude Harness Session (`--resume`): `cf1efcca-a01d-4445-a78a-c264e4dd382a`
  - Turn 1 (`turn-d08ed5b8`): `exitCode: 0`, `durationMs: 5015`, response: `"Ready. I've noted the token NEXUS_EVIDENCE_3141 for Turn 2."`, cost: `$0.00002`
  - Turn 2 (`turn-2f8f9d69`): `exitCode: 0`, `durationMs: 4221`, response: `"NEXUS_EVIDENCE_3141"`, cost: `$0.00004`
- Visual UI screenshot saved at [`docs/evidence/m1-chat-ui.png`](file:///C:/Users/yjsjs0987/Desktop/Projects/ASD-Nexus/docs/evidence/m1-chat-ui.png), showing:
  - Sidebar `Claude Sessions (1)` automatically synchronized after turn completion (`ASD`, `haiku`, `cf1efcca-a`).
  - Header pills displaying `SDK v2.6.0`, active ASD session ID, and `Claude Resume: cf1efcca-a01`.
  - Clean two-turn conversation timeline and token/cost telemetry.

---

## 3. Core / Environment Observation (Report-Only, No Core Edits)

- During initial live probing without `model` specified on `POST /api/turn/stream` (and without `~/.asd/routing.json`), `asd-runtime` (`src/core/drivers.ts`) defaulted to `--model claude-sonnet-5-5`, which the local Anthropic proxy/Claude CLI rejected (`There's an issue with the selected model (claude-sonnet-5-5)`).
- Providing `"haiku"` in the M1 Composer / API payload (matching the local `~/.claude/settings.json` configuration) executes cleanly with `exitCode: 0`.
- Suggested Core consideration (no changes made in `asd-multiagent` or `asd-runtime`): allow `runDriverTurn` / `buildSpawnSpec` to omit `--model` when no explicit user/routing model is configured so each harness CLI uses its own native profile default unless overridden.

---

## 4. Verification Suite Summary

Ran `npm run typecheck && npm run build && node --test tests/sdk-surface.test.ts tests/server-sse.test.ts tests/live-claude-m1.test.ts`:
- `tsc --noEmit`: 0 errors
- `vite build`: `dist/web/` built (`index.html`, CSS, JS bundle)
- `node --test`: **19/19 passed** across 5 suites (0 failed, 0 skipped):
  - `ASD v2.6.0 SDK Surface & Boundary Contract` (11 tests)
  - `Orchestrator HTTP + SSE Server (M1)` (7 tests)
  - `Live Claude Code M1 Integration` (1 real CLI multi-turn E2E test)
