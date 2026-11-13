# ASD-Nexus Testing & Verification Guide

ASD-Nexus consists of a clean-room Node v2.6.0 HTTP/SSE sidecar server (`src/server/**`) and a monochrome native Rust workspace (`native/Cargo.toml`: `asd-nexus-client`, `asd-nexus-cli`, `asd-nexus-desktop`).

## 1. Prerequisites

- Node.js `>=22.13.0` (`node:test`, `node:sqlite`, TypeScript strip-types)
- Rust toolchain (`cargo` / `rustc` with locked `native/Cargo.lock`)
- For Live 5-Harness E2E (`tests/live-5harness-e2e.test.ts`):
  - Installed and authenticated CLIs on `PATH`: `claude`, `codex`, `opencode`, `agy`, `agent`
  - If `claude` is configured with a local proxy (e.g. `ANTHROPIC_BASE_URL=http://127.0.0.1:4000`), ensure that proxy is running

## 2. Node Sidecar Checks

### TypeScript Typecheck

```bash
npm run typecheck
```

### Offline Server & Normalizer Tests Only

Runs 5-harness fixture replay, chunk-boundary equivalence, v2.6.0 SDK boundary cleanliness, HTTP/SSE server APIs, and `--port 0` sidecar process lifecycle:

```bash
node --no-warnings --test tests/normalizer-replay.test.ts tests/sdk-surface.test.ts tests/server-api.test.ts
```

### Live 5-Harness CLI E2E Suite

Runs Turn 1 SSE streaming -> Turn 2 session resume context check -> `/api/sessions` discovery -> `/api/transcript/:harness/:id` normalized restoration against all 5 installed CLIs in isolated temporary git repositories, and records non-secret telemetry to `docs/evidence/e2e-5harness-summary.json`:

```bash
node --no-warnings --test tests/live-5harness-e2e.test.ts
```

Model overrides per harness (optional):
- `ASD_E2E_MODEL_CLAUDE` (default: `haiku`)
- `ASD_E2E_MODEL_CODEX` (default: `gpt-6-luna`)
- `ASD_E2E_MODEL_OPENCODE` (default: `opencode/glm-5.3-flash-free`)
- `ASD_E2E_MODEL_ANTIGRAVITY` (default: `gemini-3.8-flash-low`)
- `ASD_E2E_MODEL_CURSOR` (default: `default`)

Note: `nvidia/z-ai/glm-5.3-flash` returned an upstream 404 during verification, so the defaults intentionally avoid it: `claude-code` uses `haiku` through the local `127.0.0.1:4000` proxy, and `opencode` uses the free `opencode/glm-5.3-flash-free` model. Override with `ASD_E2E_MODEL_*` only when the routed model is confirmed healthy.

### Full Node Suite (Offline + Live 5-Harness E2E)

```bash
npm test
```

## 3. Native Rust Workspace Tests & Release Build

### Locked Workspace Tests (`asd-nexus-client`, `asd-nexus-cli`, `asd-nexus-desktop`)

Includes `ratatui` TUI `TestBackend` monochrome rendering tests, `egui` headless tab grayscale pixel verification, Korean font fallback tests, CLI arg checks, and live `--port 0` sidecar spawn/shutdown verification:

```bash
cargo test --manifest-path native/Cargo.toml --workspace --locked
```

### Release Binaries

```bash
cargo build --release --manifest-path native/Cargo.toml --locked
```

Produces:
- `native/target/release/asd-nexus-cli.exe`
- `native/target/release/asd-nexus-desktop.exe`

## 4. Live Release Binary Smoke & Desktop UI Verification

### Desktop Non-Interactive Verification (`--verify`)

Auto-spawns the Node sidecar with `--port 0`, connects over HTTP, loads Korean fonts (`malgun.ttf`), verifies monochrome visuals, renders all 6 tabs headlessly, prints JSON diagnostics, and terminates the child sidecar cleanly:

```bash
./native/target/release/asd-nexus-desktop.exe --verify --project-root .
```

### Desktop Real Window Screenshot Capture

Captures a 24-bit BMP screenshot (convertible to PNG for inspection/archival) and exits cleanly:

```bash
./native/target/release/asd-nexus-desktop.exe --project-root . --screenshot docs/evidence/desktop-monochrome.bmp --exit-after 4
```

### CLI Scriptable Mode Smoke Check

```bash
./native/target/release/asd-nexus-cli.exe health --json
./native/target/release/asd-nexus-cli.exe board --json
./native/target/release/asd-nexus-cli.exe sessions --json
```

### Full Regression Verification Command

```bash
npm run typecheck && npm test && cargo test --manifest-path native/Cargo.toml --locked && cargo build --release --manifest-path native/Cargo.toml --locked
```
