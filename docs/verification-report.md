# ASD-Nexus Clean-Room Verification Report

**Session:** `asd-20261118-161100-acpl`  
**Task:** `T4.1` — Remediate Critic edge-case findings, run full TS + Rust verification gate, and update verification report  
**Timestamp (UTC):** `2026-11-18T08:25:40Z`  
**Prior Live E2E Evidence:** `docs/evidence/e2e-5harness-summary.json` (generated `2026-11-13T16:12:11.152Z`, 5/5 harnesses passed; live external LLM suite was not re-run in this offline/UI pass)

## 1. Architecture & UX Verified

- **Node v2.6.0 HTTP/SSE Sidecar (`src/server/`)**:
  - Zero `child_process` imports in `src/`, `--port 0` JSON readiness handshake, clean stdin EOF shutdown, `erasableSyntaxOnly: true` compliance, `GET /api/harness-commands`, `GET /api/fs/list`, and custom slash command expansion in `src/server/routes/turns.ts`.
  - **Server-Side Harness Immutability (`turns.ts`)**: Started/existing ASD sessions reject cross-harness turn requests with `HARNESS_IMMUTABLE` (preserving `lockedHarness` and `harness_session_id`), and raw native CLI `sessionId` requests check `db.listSessions?.()` / `findSessionByHarnessSessionId` to reject cross-harness mismatches with `HARNESS_IMMUTABLE` rather than creating duplicate sessions.
  - **Live Same-Harness Model Switching & Default Reset (`turns.ts`)**: Same-harness model updates attach cleanly while preserving `harness_session_id`; explicit `model: "default"` or `""` resets `session_drivers.model` and `sessions.main_model` to `null` and passes `undefined` into `runDriverTurn`.
  - **`autoApprove` Persistence & Interactive Permission Broker (`sessions.ts`, `turns.ts`)**: `POST /api/sessions` and `POST /api/sessions/auto-approve` persist `auto_approve` in SQLite. When `autoApprove` is `false`, `runDriverTurn` tool permission requests emit SSE `permission_request` events and block until `POST /api/turn/permission` resolves (`selected` / `cancelled`). Choosing `allow_always` (including empty-options fallback `optionId === "allow_always"`) updates `turn.autoApprove = true` and persists `auto_approve = 1` in SQLite.
  - **Core Runtime ACP Integration (`sdk.ts`, `app.ts`, `index.ts`, `sessions.ts`, `turns.ts`)**: Wired to ASD Core v2.7.0 `CR-ACP-DRIVER` (`c795d22`, `7e1b42c`, `1dccb52`). `runDriverTurn` runs persistent ACP sessions (`transport: "auto"` with streaming CLI fallback when external bridges are absent), supports live `session/set_model` model switching and `onPermissionRequest` interactive permission modal round-trips, calls cooperative `cancelDriverTurn` (`session/cancel`) on turn cancel, closes pooled ACP sessions via `closeDriverSession` on session demote/unlink/delete, and drains `closeAllDriverSessions` on sidecar shutdown.
- **CLI / TUI (`native/asd-nexus-cli`)**:
  - Borderless OpenCode-inspired TUI with `DarkColor` default + strict grayscale `Monochrome` toggle, `/` command & `@` file popups, `Ctrl+P` palette, new-session-only harness picker + read-only started-session harness badge with live model switching, `/auto-approve` toggle, and interactive `PERMISSION APPROVAL` modal (`y` / `a` / `n` / `Esc` / arrow selection, plus `Ctrl+X` turn abort).
- **Desktop GUI (`native/asd-nexus-desktop`)**:
  - `eframe`/`egui` native OS window (`0%` WebView) with `Chat` | `Dashboard` mode split, `DarkColor` default + strict grayscale `Monochrome` toggle, new-session-only harness dropdown + read-only started-session harness badge with live model dropdown, `autoApprove` toggle & `/auto-approve` command, and interactive Permission Request modal.

## 2. Critic Edge-Case Remediations (T4.1)

1. **Desktop Unpromoted CLI Session Hijack & Continuation (`native/asd-nexus-desktop/src/app.rs`)**: Guarded `Reply::Sessions` foreground auto-bind with `self.selected_session.is_none()`, and updated `send_prompt` to pass `selected_session` (`harness_session_id`) when `active_session_id.is_none()` outside draft mode.
2. **TUI Unpromoted CLI Session `foreground_id` Leak (`native/asd-nexus-cli/src/app.rs`)**: Prevented `active_session_id()` from falling back to `foreground_id` when `self.new_session_draft || self.current.is_some()`.
3. **TUI Pre-Turn `create_session` Failure Abort (`native/asd-nexus-cli/src/exec.rs`)**: Aborts `Action::SendPrompt` cleanly with `app.timeline.end_turn(Some("failed to initialize session"))` if pre-turn `create_session` fails when `!app.auto_approve`.
4. **TUI `/new` While Streaming (`native/asd-nexus-cli/src/app.rs`, `exec.rs`)**: Both `/new` and `/new <title>` cancel any active stream (`Action::CancelTurn` + handle preservation) and clear `self.stream = None; self.timeline.streaming = false;`.
5. **Model Stickiness & Explicit `(default)` Reset (`src/server/routes/turns.ts`, Desktop `app.rs`, TUI `app.rs`/`exec.rs`)**: Always sync selected model on session switch (including `None`), pass `Some("default")` when resetting to `(default)` on a started session, and reset driver/session model to `null` in `turns.ts` while passing `undefined` to `runDriverTurn`.
6. **TUI `Ctrl+X` During Permission Modal (`native/asd-nexus-cli/src/app.rs`)**: Allowed `Ctrl+X` while `pending_permissions` is non-empty to clear pending permissions and emit `Action::CancelTurn`.
7. **Desktop `backend.rs` `in_flight` Balance (`native/asd-nexus-desktop/src/backend.rs`)**: Removed uncounted `Reply::CreateSession` emission from `Request::StartTurn` (active session ID is captured from the initial SSE `status` event).
8. **Empty-Options `allow_always` Fallback & Cross-Harness Raw `sessionId` Guard (`src/server/routes/turns.ts`)**: Handled `optionId === "allow_always"` fallback when `options: []` and enforced `HARNESS_IMMUTABLE` across raw native CLI `sessionId` lookups via `db.listSessions?.()`.

## 3. Node Offline & Server Checks

- **`npm run typecheck`** (`tsc --noEmit`): exit code `0` (`0` errors).
- **Offline & Live ACP Subprocess Test Suites** (`node --no-warnings --test tests/normalizer-replay.test.ts tests/sdk-surface.test.ts tests/server-api.test.ts tests/model-catalog.test.ts`): **82 passed, 0 failed, 0 skipped** across **25 suites**:
  - Replays 10 NDJSON fixtures across all 5 harnesses plus 7-byte chunk-boundary equivalence.
  - Verifies model catalog merge behavior across partial refreshes.
  - Enforces v2.6.0+ SDK boundary cleanliness across `src/` (`0` `child_process` imports) and verifies `cancelDriverTurn` / `closeDriverSession` / `closeDriverSessionByDriverId` / `closeAllDriverSessions` exports.
  - Verifies HTTP auth, project/session/board/plan/routing/worktree/subagent/terminal routes, `/api/harness-commands`, `/api/fs/list`, SQLite active driver attachment + Turn 2 `harnessSessionId` preservation, `HARNESS_IMMUTABLE` (ASD ID & raw CLI `sessionId`), explicit `"default"` model reset, `autoApprove` persistence, interactive permission brokering, real child sidecar `--port 0` EOF termination, and **live `sdk.runDriverTurn` persistent ACP subprocess PID reuse + `session/set_model` + `session/request_permission` SSE modal approval + `DELETE /api/sessions/:id` process teardown**.

## 4. Native Rust Locked Test & Release Build Results

- **`cargo test --manifest-path native/Cargo.toml --workspace --locked`**: **86 passed, 0 failed, 0 ignored**:
  - `asd-nexus-client`: `14` passed (`13` unit + `1` `tests/live_sidecar.rs`)
  - `asd-nexus-cli`: `32` passed (`8` unit + `4` `tests/cli_args.rs` + `20` `tests/tui_render.rs`, including regression tests for unpromoted session `active_session_id() == None`, `/new` stream cancellation, `Ctrl+X` during permission modal, and `pending_permissions` clearing on stream end/interrupt)
  - `asd-nexus-desktop`: `40` passed (`23` unit + `3` `tests/cli_lifecycle.rs` + `14` `tests/gui_headless.rs`, including regression tests for `Reply::Sessions` non-hijack on unpromoted CLI sessions, stale model clearing, `auto_approve: false` draft turn submission, and permission modal flow)
- **`cargo build --release --manifest-path native/Cargo.toml --locked`**: exit code `0` (`24.03s`), built `native/target/release/asd-nexus-cli.exe` and `native/target/release/asd-nexus-desktop.exe`.
