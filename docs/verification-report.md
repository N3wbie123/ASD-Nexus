# ASD-Nexus Clean-Room Verification Report

**Session:** `asd-20261114-173800-ux01`  
**Task:** `T3.1` — Full TS + Rust verification gate, release build, dual-theme desktop Dark/Mono captures, and CLI smoke tests  
**Timestamp (UTC):** `2026-11-14T09:35:50Z`  
**Prior Live E2E Evidence:** `docs/evidence/e2e-5harness-summary.json` (generated `2026-11-13T16:12:11.152Z`, 5/5 harnesses passed; live external LLM suite was not re-run in this offline/UI pass)

## 1. Architecture & UX Verified

- **Node v2.6.0 HTTP/SSE Sidecar (`src/server/`)**: Zero `child_process` imports in `src/`, `--port 0` JSON readiness handshake, clean stdin EOF shutdown, plus `GET /api/harness-commands` (`src/server/routes/commands.ts` for all 5 harnesses' builtin + global/project `.md` commands/skills), `GET /api/fs/list` (`src/server/routes/fs.ts` for `@` files & `/cd` dirs), and custom slash command expansion in `src/server/routes/turns.ts`.
- **CLI / TUI (`native/asd-nexus-cli`)**: OpenCode-inspired borderless TUI with Home & Active timeline views, collapsible `/sidebar` drawer, `DarkColor` default + strict grayscale `Monochrome` `/theme` toggle, `/` command popup (`[nexus]` + `[<harness>]` commands), `Ctrl+P` palette, `@` workspace file autocomplete, `!` local shell execution, mouse wheel/click support, and automatic new session creation on manual harness switch.
- **Desktop GUI (`native/asd-nexus-desktop`)**: `eframe`/`egui` native OS window (`0%` WebView) with Antigravity-inspired `Chat` | `Dashboard` (`Ctrl+1`/`Ctrl+2`) mode split, `DarkColor` default + strict grayscale `Monochrome` toggle (`Dark | Mono` button and `--theme mono` CLI flag), collapsible Workspaces & Sessions rail, centered chat timeline (`Thought for …` accordions, tool cards, markdown responses), floating rounded composer with `/` & `@` popups and `Ctrl+P` palette, and separated 5-subtab `Dashboard` (`PLAN & Board`, `Worktree Diff`, `Subagents`, `Routing`, `Terminal & Resume`).

## 2. Node Offline & Server Checks

- **`npm run typecheck`** (`tsc --noEmit`): exit code `0` (0 errors).
- **Offline Test Suites** (`tests/normalizer-replay.test.ts`, `tests/sdk-surface.test.ts`, `tests/server-api.test.ts`, `tests/model-catalog.test.ts`): **73 passed, 0 failed, 0 skipped** across **23 suites** in `1,261 ms` (re-verified at `1,299 ms` standalone):
  - Replays 10 NDJSON fixtures (`stream.ndjson` + `error.ndjson`) across all 5 harnesses plus 7-byte chunk-boundary equivalence.
  - Verifies model catalog merge behavior across partial refreshes.
  - Enforces v2.6.0 SDK boundary cleanliness across `src/` (`0` `child_process` imports).
  - Verifies HTTP auth, project/session/board/plan/routing/worktree/subagent/terminal routes, `/api/harness-commands`, `/api/fs/list`, SQLite active driver attachment + Turn 2 `harnessSessionId` preservation, and real child sidecar `--port 0` EOF termination.

## 3. Native Rust Locked Test & Release Build Results

- **`cargo test --manifest-path native/Cargo.toml --workspace --locked`**: **80 passed, 0 failed, 0 ignored**:
  - `asd-nexus-client`: `11` passed (`10` unit + `1` `tests/live_sidecar.rs` including `harness_commands` & `fs_list` roundtrips)
  - `asd-nexus-cli`: `30` passed (`8` unit + `4` `tests/cli_args.rs` + `18` `tests/tui_render.rs`)
  - `asd-nexus-desktop`: `39` passed (`23` unit + `3` `tests/cli_lifecycle.rs` + `13` `tests/gui_headless.rs` verifying `Chat`/`Dashboard` split, `/` & `@` popups, palette, `DarkColor` default, strict grayscale `Monochrome`, and auto-new-session on harness switch)
- **`cargo build --release --manifest-path native/Cargo.toml --locked`**: exited `0`, built `native/target/release/asd-nexus-cli.exe` and `native/target/release/asd-nexus-desktop.exe`.

## 4. Verification Fixes Applied During T3.1 Inspection

1. **`native/asd-nexus-desktop/src/app.rs`**: Shortened sidebar header button text (`Open Folder…` -> `Open…` with tooltip) to prevent horizontal overlap with the `WORKSPACES` label at default/narrow widths.
2. **`native/asd-nexus-desktop/src/app.rs` & `tests/gui_headless.rs`**: Prevented plain `Enter` in the focused composer from sending uncompleted `@token` or `/cmd` strings while `@`/`/` autocomplete popups are active (`Tab`/`Enter` applies/executes the highlighted popup item; `Ctrl+Enter` forces immediate send).
3. **`native/asd-nexus-desktop/src/{cli.rs,main.rs}`**: Added `--theme <dark|mono>` flag so both `DarkColor` and strict grayscale `Monochrome` screenshots can be automated cleanly.

## 5. Release Binary, Desktop Dark/Mono Captures & CLI Smoke Checks

- **`./native/target/release/asd-nexus-desktop.exe --verify --project-root .`**:
  - Exit code `0`, `"ok": true`, `"spawnedSidecar": true`, `"connection": "Connected(\"http://127.0.0.1:54214\")"`, `"font": "font: malgun.ttf"`, `"monochromeVisuals": true`, `"frames": 120`, `"projects": 23`, `"sessions": 35`, `"boardTasks": 4`, `"modelHarnesses": 5`, `"planLoaded": true`, `"routingLoaded": true`, `"status": "theme: Dark"`, all 6 tabs rendered (`Chat`, `PLAN & Board`, `Worktree Diff`, `Subagents`, `Routing`, `Terminal & Resume`).
- **`docs/evidence/desktop-dark.png`** (`1440x900`) & **`docs/evidence/desktop-monochrome.png`** (`1440x900` via `--theme mono`):
  - Pixel audit on `desktop-monochrome.png` (`x+=4, y+=4`): `0` non-grayscale pixels (`R == G == B` everywhere), `237` distinct luminance levels.
  - Visual inspection of both PNGs confirmed crisp `Chat` | `Dashboard` top bar, clean `WORKSPACES` / `Open…` sidebar layout, Antigravity floating composer card, and `malgun.ttf` Korean text rendering.
- **`./native/target/release/asd-nexus-cli.exe` smoke checks**:
  - `health --json --project-root .` (`exit 0`, `{ "ok": true, "version": "2.6.0", "defaultProjectRoot": "." }`), `sessions --json --limit 5 --project-root .` (`exit 0`, `55` lines), and `board --json --project-root .` (`exit 0`, `39` lines) all succeeded with auto-managed sidecar startup and shutdown.
- **Process Hygiene**: `node` process count observed `2 -> 2` before/after `--verify`, CLI smoke checks, and both desktop `--screenshot` runs (`0` leaked child processes).
