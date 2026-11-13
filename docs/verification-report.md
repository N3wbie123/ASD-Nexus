# ASD-Nexus Clean-Room Verification Report

**Session:** `asd-20261113-122445-z8e9`  
**Task:** `T6.1` — 5-harness live CLI E2E verification, Rust release build, and desktop UI capture  
**Timestamp (UTC):** `2026-11-13T13:51:20Z`  
**E2E evidence:** `docs/evidence/e2e-5harness-summary.json` (generated `2026-11-13T13:50:09.420Z`)

## 1. Architecture Verified

- **Node v2.6.0 HTTP/SSE Sidecar (`src/server/`)**: Zero `child_process` imports in `src/`, zero removed WebView/Vite artifacts, clean `--port 0` JSON readiness handshake (`SIDECAR_READY_PROTOCOL = "asd-nexus-sidecar"`), and clean shutdown on stdin EOF.
- **Rust Workspace (`native/Cargo.toml`)**: Zero WebView crates (`wry`/`tauri`); shared synchronous client (`asd-nexus-client`), pure monochrome terminal TUI/CLI (`asd-nexus-cli` using `ratatui`/`crossterm`), and pure monochrome native OS window (`asd-nexus-desktop` using `eframe`/`egui` + `rfd`).

## 2. Node Offline & Server Checks

- **`npm run typecheck`** (`tsc --noEmit`): exit code `0` (0 errors).
- **Offline Test Suites** (`tests/normalizer-replay.test.ts`, `tests/sdk-surface.test.ts`, `tests/server-api.test.ts`): **67 passed, 0 failed, 0 skipped** in `1,174 ms`:
  - Replays 10 NDJSON fixtures (`stream.ndjson` + `error.ndjson`) across all 5 harnesses plus 7-byte chunk-boundary equivalence.
  - Enforces v2.6.0 SDK boundary cleanliness across `src/`.
  - Verifies HTTP auth, project/session/board/plan/routing/worktree/subagent/terminal routes, SQLite active driver attachment + Turn 2 `harnessSessionId` preservation, OpenCode hook-bridge `.jsonl` stub bypass in favor of `opencode.db`, and real child sidecar `--port 0` EOF termination.
  - This offline suite is the `--cmd` used for `asd verify` (well under the ASD 60 s verification timeout; live E2E is not part of it).

## 3. Live 5-Harness CLI E2E Results (`tests/live-5harness-e2e.test.ts`)

Every installed CLI completed **Turn 1 SSE stream -> Turn 2 session resume recall (`AMBER-42`) -> `GET /api/sessions` discovery -> `GET /api/transcript/:harness/:id` normalized restoration** in isolated temporary git repos (`5/5` passed, `0` failed, `0` skipped; combined with the 67 offline tests: **72/72 passed**; suite duration `81.6 s`):

| Harness | Model | Duration (ms) | Turn 1 SSE Kinds | Turn 2 Resume Text | Transcript Msgs | Categories Restored |
|---|---|---:|---|---|---:|---|
| `claude-code` | `haiku` | 22,584 | `status`, `text`, `usage` | `AMBER-42` | 4 | `user`, `assistant` |
| `codex` | `gpt-6-luna` | 11,490 | `status`, `text`, `usage` | `AMBER-42` | 5 | `user`, `assistant` |
| `opencode` | `opencode/glm-5.3-flash-free` | 17,560 | `status`, `text`, `usage` | `AMBER-42` | 4 | `user`, `assistant` |
| `antigravity` | `gemini-3.8-flash-low` | 14,173 | `status`, `text` | `AMBER-42` | 5 | `user`, `assistant`, `tool_result` |
| `cursor` | `default` | 15,429 | `status`, `thinking`, `text`, `usage` | `AMBER-42` | 17 | `user`, `thinking`, `assistant` |

### Model Routing During Verification

`nvidia/z-ai/glm-5.3-flash` returned an upstream 404 (`integrate.api.nvidia.com`) throughout this verification window, which broke every GLM-routed default. The E2E defaults therefore avoid that route:

- `claude-code` -> `haiku` (mapped by the local `127.0.0.1:4000` proxy to Nemotron; the proxy must be running).
- `opencode` -> `opencode/glm-5.3-flash-free` (free model, confirmed healthy).
- `codex` -> `gpt-6-luna`, `antigravity` -> `gemini-3.8-flash-low`, `cursor` -> `default`.

These defaults live in `tests/live-5harness-e2e.test.ts` and are overridable with `ASD_E2E_MODEL_*` (`docs/testing-guide.md`).

### Root-Cause Fixes Applied During Verification

1. **`src/server/routes/turns.ts` & `src/server/app.ts`** (commit `ba8ce27`): Wired `openProjectDb`, `isValidAsdSessionId`, and `attachDriver` into `executeTurn`, preserving existing `harness_session_id` across Turn 2 resumes on the active driver and routing raw stdout through `createHarnessStreamNormalizer` to emit `usage` events and latch `harnessSessionId`.
2. **`src/server/routes/transcripts.ts`** (commit `dd6557e`): Prefer `opencode.db` over mirrored Claude hook-bridge `.jsonl` stubs (`~/.claude/transcripts/ses_*.jsonl`) when `transcriptPath` does not end with `.db` (retaining fallback when DB lacks entries).

## 4. Native Rust Locked Test & Release Build Results

- **`cargo test --manifest-path native/Cargo.toml --workspace --locked`**: **66 passed, 0 failed, 0 ignored**:
  - `asd-nexus-client`: `11` passed (`10` unit + `1` `tests/live_sidecar.rs`)
  - `asd-nexus-cli`: `24` passed (`6` unit + `4` `tests/cli_args.rs` + `14` `tests/tui_render.rs`)
  - `asd-nexus-desktop`: `31` passed (`19` unit + `3` `tests/cli_lifecycle.rs` + `9` `tests/gui_headless.rs`)
- **`cargo build --release --manifest-path native/Cargo.toml --locked`**: exited `0`, built `native/target/release/asd-nexus-cli.exe` and `native/target/release/asd-nexus-desktop.exe`.

## 5. Release Binary & Desktop Monochrome UI Verification

- **`./native/target/release/asd-nexus-desktop.exe --verify --project-root .`**:
  - Exit code `0`, `"ok": true`, `"spawnedSidecar": true`, `"connection": "Connected(\"http://127.0.0.1:<ephemeral>\")"`, `"font": "font: malgun.ttf"`, `"monochromeVisuals": true`, `"frames": 132`, `"projects": 22`, `"sessions": 22`, `"boardTasks": 8`, `"modelHarnesses": 5`, `"planLoaded": true`, `"routingLoaded": true`, all 6 tabs rendered (`Chat`, `PLAN & Board`, `Worktree Diff`, `Subagents`, `Routing`, `Terminal & Resume`).
- **`docs/evidence/desktop-monochrome.png`** (`1440x900`):
  - Pixel audit (`x+=4, y+=4`): `0` non-grayscale pixels (`R == G == B` everywhere), `217` distinct luminance levels.
  - Visual inspection confirmed crisp pure monochrome layout, left project/session rail, and `malgun.ttf` Korean UI text rendering.
- **`./native/target/release/asd-nexus-cli.exe` smoke checks**:
  - `health --json` (`exit 0`, `{ "ok": true, "version": "2.6.0" }`) and `board --json` (`exit 0`, `77` lines) succeeded with auto-managed sidecar startup and shutdown.
- **Process Hygiene**: `node` process count observed `4 -> 3` around the desktop `--verify` run (no leaked sidecar child remained), ending at `3` node processes.
