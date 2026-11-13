# ASD 오케스트레이터 TUI/GUI 앱 개발 계획

> 작성일: 2026-11-06 · 근거: 5개 하네스 로컬 실측 + 공식 문서 조사 + ASD v2.4.2 코드 검증
> 상태: M1 검증 완료 (`asd-20261113-104918-ln8l`) → **순수 네이티브 Rust 전환 계획 확정 (새 세션에서 M2~M4 재착수 예정)**
> 갱신 (2026-11-13, ASD v2.6.0 기준): §0 표의 스냅샷·명령 조립 위치를 최신 코드에 맞춤. 어댑터의 headless/resume/interactive 빌더 메서드는 v2.6.0에서 삭제되었고 명령 조립은 `src/core/session-tracker.ts`의 `buildSpawnSpec` 계열 단일 경로임. 부록 B 갭 1(zstd)은 해결 확인.
> **아키텍처 전환 확정 (2026-11-13)**: 브라우저/WebView/Tauri 대신 **완전 네이티브 Rust (`eframe`/`egui` 데스크톱 GUI + `ratatui`/`crossterm` 터미널 TUI/CLI)** 로 전환하며, 기존 `src/web/`·`index.html`·`vite.config.ts` 웹 프론트엔드는 삭제한다. UI 테마는 **순수 흑백(모노크롬)** 으로 통일한다.

---

## 0. 핵심 전제 — 이미 구현된 것을 다시 만들지 않는다

ASD v2.4.2(`asd-multiagent`)에는 앱의 코어 로직 대부분이 이미 구현·출시 상태다.
새 앱은 **"새 하네스 연동 레이어"가 아니라 "기존 ASD SDK 위의 쉘"**이다.

| 계획한 기능 | 기존 구현 | 위치 |
|---|---|---|
| 하네스 호출 + 실시간 스트리밍(thinking/tool_use/tool_result/assistant) | `runDriverTurn` + `parseDriverStreamLine` | `src/core/drivers.ts` |
| 세션 기록 불러오기(4분류 파싱) | `readHarnessTranscript` | `src/core/transcripts.ts` |
| 프로젝트 세션 자동 탐색 | `discoverProjectCliSessions` | `src/core/session-tracker.ts` |
| ASD 진행도 보드 | `renderProgressBoard`, `asd status --json` | `src/core/render.ts`, `src/cli/index.ts` |
| 전체 상태 스냅샷 | `collectDashboardSnapshot()` | `src/core/session-services.ts` |
| 웹 UI 원형 | `asd ui` (node:http, :3847) | `src/cli/dashboard.ts` |
| 서브세션 스폰/로그 | `spawnSubagent`, `scanSubagentLogs` | `src/core/spawner.ts` |
| 하네스 명령 조립 | `buildSpawnSpec` / `buildHeadlessHarnessCommand` / `buildResumeHarnessCommand` / `buildInteractiveResumeHarnessCommand` | `src/core/session-tracker.ts` |
| GUI 프로젝트 | ASD-Nexus (`yjsjs0987/ASD-Nexus`) | 별도 리포 |

---

## 1. 하네스별 실측 매트릭스

| 하네스 | 세션 저장 | 스트리밍 1순위 | 스트리밍 2순위(CLI 파이프) | 세션 이어가기 | 증거 |
|---|---|---|---|---|---|
| OpenCode | `~/.local/share/opencode/opencode.db` (SQLite WAL, 단일) | `opencode serve` + SSE `GET /api/event/subscribe` | `opencode run "..." --format json` | `-s <id>` / `-c` / `--fork`, `opencode export <id>` | 데모 실행 ✅ |
| Claude Code | `~/.claude/projects/<cwd인코딩>/<uuid>.jsonl` | Agent SDK `query({ resume, includePartialMessages })` | `claude -p --output-format stream-json --verbose --include-partial-messages` | `--resume <id>` + 스트림 동시 공식 지원, `--fork-session` | 스트림 이벤트 실측 ✅ |
| Codex CLI | `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` (구세션 `.zst`) | `codex app-server` JSON-RPC (양방향, 런타임 승인 가능) | `codex exec --json` | `codex exec resume <id>`, `codex fork` | **데모 완주 ✅** |
| Cursor CLI | `~/.cursor/chats/<workspace-md5>/<uuid>/store.db` (SQLite) + `meta.json` | `@cursor/sdk` (GA) | `cursor-agent -p --output-format stream-json --stream-partial-output` | `--resume <chatId>`, `create-chat` | 헬프+문서+저장소 실측 |
| Antigravity | `~/.gemini/antigravity-cli/conversations/<id>.db` + `brain/<id>/.system_generated/logs/transcript.jsonl`(→`transcript_full.jsonl` 폴백) | Python/Rust SDK | `agy -p --output-format stream-json`, `--input-format stream-json`(상주 멀티턴) | `--conversation <id>`, `-c` | 헬프+문서+디렉토리 실측 |

### 하네스별 핵심 노트

- **OpenCode**: TUI 자체가 서버 클라이언트. `run --attach http://localhost:4096`으로 웜 서버 재사용(MCP 콜드부트 회피). 공식 SDK `@opencode/sdk`(임베디드)/`@opencode/client`.
- **Claude**: 세션 JSONL에 thinking/tool_use/tool_result 전부 저장. 단 **포맷 불안정 공식 경고** → `readHarnessTranscript` 파서 유지보수 필요. Agent SDK로 권한 승인 UI(`canUseTool`)까지 제어 가능.
- **Codex**: app-server는 `initialize` 핸드셰이크 → `thread/start|resume|fork`, `turn/start|steer|interrupt`, `item/agentMessage/delta` 알림, 승인 RPC(`execCommandApproval`) 제공. `codex app-server generate-ts`로 TS 바인딩 자동 생성. 구 `codex mcp` 서버는 제거됨(app-server가 후계). 실측 함정: stdin 파이프 환경에서 stdin을 닫아야 함.
- **Cursor**: `--stream-partial-output`에서 `model_call_id` 없으면 진짜 델타, 있으면 버퍼 플러시(중복 스킵). IDE와 CLI 세션 저장소 분리.
- **Antigravity**: `agy acp`(ACP stdio JSON-RPC 서버) 존재. App(IDE) 쪽 아티팩트는 `~/.gemini/antigravity/brain/<id>/task.md` 등 Markdown.

### 공통 결론

5종 전부 **자식 프로세스 stdout JSONL 파싱만으로 실시간 스트리밍 가능**. OpenCode·Codex·Claude는 추가로 상주 서버/프로토콜 경로 제공. 세션 렌더링(과거 기록)은 전부 저장 파일/DB 파싱으로 해결.

---

## 2. 아키텍처 — Node SDK 사이드카 서버 + 순수 네이티브 Rust 클라이언트 (GUI + TUI/CLI)

```
┌──────────────────────┐   HTTP/SSE    ┌──────────────────────────────────┐   자식 프로세스
│ Rust TUI/CLI 앱      │ ────────────▶ │ Node 오케스트레이터 서버 (사이드카) │ ──▶ claude -p --output-format stream-json
│ (ratatui+crossterm)  │ ◀──────────── │  - src/server/* (HTTP + SSE)     │ ──▶ codex exec --json
└──────────────────────┘   로컬 바인딩 │  - agent-swarm-director v2.6.0   │ ──▶ opencode run --format json
┌──────────────────────┐               │  - openProjectDb (읽기) + MCP/CLI │ ──▶ agy -p --output-format stream-json
│ Rust 데스크톱 GUI 앱 │ ────────────▶ │  - 5하네스 이벤트 정규화          │ ──▶ cursor-agent -p --output-format stream-json
│ (eframe/egui + rfd)  │ ◀──────────── └──────────────────────────────────┘
└──────────────────────┘  (WebView 0%)
```

- **경계 원칙 유지**: Rust 쪽에 코어 로직(DB SQL, CLI 인자 문자열 조립, 트랜스크립트 파싱, worktree/routing 로직)을 **절대 재구현하지 않는다**. 대신 Rust 클라이언트가 로컬 Node v2.6.0 오케스트레이터 서버(`src/server/`, `--port 0` 자동 포트/프로세스 수명 관리)를 자동 실행하고 REST + SSE(`OrchestratorEvent`)로만 통신한다.
- **오케스트레이터 서버(`src/server/`)**: M1에서 구현한 `/api/health`, `/api/sessions`, `/api/sessions/promote`, `/api/transcript/:harness/:id`, `/api/board`, `/api/subagents`, `/api/subagent-action`, `/api/turn/stream` 기반 위에 `asd-gui` 전체 기능에 대응하는 프로젝트·모델/라우팅·세션 제어·태스크/Diff/승인·5하네스 정규화 API를 확장한다.
- **기존 웹 UI 삭제 확정**: `src/web/`, `index.html`, `vite.config.ts` 및 Vite/React 프론트 의존성은 다음 작업에서 제거하고, `ASD-Nexus`는 **서버(`src/server/` + `src/shared/`) + 순수 Rust 워크스페이스(`native/`)** 구조로 단순화한다.

### 이벤트 정규화 계약

```ts
// src/shared/events.ts 실제 계약 (OrchestratorEvent)
type OrchestratorEvent = {
  kind: "thinking" | "tool_use" | "tool_result" | "text" | "status" | "usage" | "error";
  sessionId?: string;
  turnId?: string;
  status?: "running" | "done" | "interrupted";
  text?: string;
  thinking?: string;
  toolUse?: { name: string; id?: string; input?: string };
  toolResult?: { name?: string; toolUseId?: string; output: string; isError?: boolean };
  usage?: { inputTokens?: number; outputTokens?: number; totalTokens?: number; costUsd?: number };
  error?: string;
  timestamp?: string;
  raw?: unknown;
};
```

렌더링 규칙(하네스 무관, **순수 흑백 모노크롬 테마**): 파란색/컬러 액센트를 제거하고 무채색(`#0a0a0a`~`#ffffff` 대비) 기준으로 통일. thinking=접이식 실시간 append, tool_use=아이콘+이름+인자+스피너, tool_result=접이식(`isError` 시 대비 테두리/배지 강조), text=마크다운 렌더, usage=턴 푸터.

### 승인(권한) 처리

- v1: 자동승인 플래그(`--auto`, `--dangerously-skip-permissions`, `--force`) — ASD 기존 방식.
- v2: Claude=Agent SDK `canUseTool`, Codex=`app-server` 승인 RPC, OpenCode=permissions API, Cursor=`--auto-review`.

### 확정 스택 (2026-11-13)

| 레이어 | 기술 스택 | 이유 / 비고 |
|---|---|---|
| Core | Node 22+ TS, `agent-swarm-director` (`v2.6.0` `file:../../code/asd-runtime`) | SDK 그대로 import, 코어 재구현 금지 |
| Sidecar Server | `src/server/*` (`node:http`, SSE, 의존성 최소) | Rust GUI/TUI 공통 백엔드 브리지 (`--port 0`) |
| Shared Rust Client | `native/asd-nexus-client` (`serde`, `serde_json`, HTTP+SSE 스트림 파서) | 사이드카 spawn/종료 관리 + 5하네스 REST/SSE 클라이언트 공유 |
| Native Desktop GUI | `native/asd-nexus-desktop` (`eframe` / `egui` + `rfd` OS 폴더 선택기 + 시스템 한글 폰트) | 브라우저/WebView 0% 순수 네이티브 창 (MSVC+Windows SDK+Cargo 확인됨) |
| Native TUI / CLI | `native/asd-nexus-cli` (`ratatui` + `crossterm`) | 터미널 TUI 채팅·보드·세션 탐색 + CLI 서브커맨드 제공 |
| Terminal 연동 | OS 터미널(Windows Terminal / PowerShell) 즉시 실행 + `buildInteractiveResumeHarnessCommand` 복사 + 로그 뷰 | ConPTY 내장은 향후 확장 옵션으로 분리 |

---

## 3. 세션 자동 로드 + 서브세션/진행도 모니터링

### 3.1 메인 세션 오픈 시 자동 로드 (3단계)

1. **세션 후보 수집**: `openProjectDb(projectRoot)` → ASD 세션 목록 + `discoverProjectCliSessions(projectRoot)` → 5하네스 CLI 세션 일괄(승격 여부 포함).
2. **트랜스크립트 복원**: `readHarnessTranscript(harness, harnessSessionId, { limit, homeDir, transcriptPath })` *(v2.6.0 위치 인자 시그니처 준수)* → 4분류 렌더.
3. **이어달리기**: `session_drivers.harness_session_id`로 다음 턴부터 `runDriverTurn`이 resume 플래그로 이어감.

### 3.2 실시간 갱신 전략

- 파일형(Claude jsonl / Codex rollout / agy transcript): `chokidar` append 감지 → 증분 파싱. **Codex 구세션 `.zst`는 로드 시 zstd 복원 필요(발견된 갭).**
- DB형(OpenCode `opencode.db` 2.29GB / Cursor `store.db`): 세션ID 스코프 쿼리만 + 1~2초 폴링. OpenCode는 서버 SSE가 폴링 대체.
- Claude jsonl 포맷 불안정 → 파서 버전별 회귀테스트.

### 3.3 단축키 모니터링 패널 (Ctrl+B 오버레이)

| 섹션 | 데이터 소스 (전부 기존 구현) |
|---|---|
| 진행 보드 | `asd status --json` → `progressBoard` |
| 서브세션 목록 | `collectDashboardSnapshot()` tasks + `session_drivers` |
| 서브세션 라이브 로그 | `scanSubagentLogs()` (`output.log` 실시간 append) + `sub_session_id` tail |
| HEAVY 모드 | `.asd/wt/<turn>-<task>` 확인 + `getWorktreeDiff()` |
| 컨트롤 | `/api/subagent-action` (kill/retry/fallback/resume) |

키맵 제안: Ctrl+B 보드, Ctrl+L 로그, Ctrl+D diff. 갱신: 패널 열림 중 1초 폴링 + 로그 tail.

---

## 4. 하네스 전환 이어가기 — 제외 확정

세션 포맷이 하네스마다 완전히 달라 무손실 이관은 비용 대비 가치 없음. **`/handoff`(asd-handoff) 스킬의 "대화 요약 → 어느 하네스에서든 재개"를 표준 방법으로 채택**. 변환 레이어/신규 DB 미생성.

---

## 5. `asd-gui` 이식 기능 범위 & 개정 구현 시퀀스 (v2.6.0 + 순수 네이티브 Rust)

### 5.1 `asd-gui` 기능 이식 범위 (Core v2.6.0 SDK 기반 클린룸 구현)
*주의: 구 `asd-gui`(v2.4.0)의 삭제된 SDK 심볼/포지셔널 인자 코드는 복사하지 않고, 기능 명세만 v2.6.0 공개 SDK 위에 새로 구현한다.*

| 영역 | 기능 항목 | 사용 v2.6.0 Core SDK / 서버 API |
|---|---|---|
| **프로젝트 전환** | 등록 프로젝트 목록, 폴더 탐색, OS 폴더 선택(`rfd`), 프로젝트 변경 즉시 세션 조회/시작 | `listKnownProjects`, `resolveAsdPaths`, `GET /api/projects/*` |
| **5하네스 실행** | `claude-code`·`opencode`·`codex`·`antigravity`·`cursor` 드라이버 선택/전환, 실시간 SSE, 취소 | `runDriverTurn(streamJson:true)`, `buildSpawnSpec`, `POST /api/turn/stream` |
| **모델·라우팅** | 하네스별 사용 모델 목록 조회, 프로젝트/전역 라우팅 설정 조회·저장 | `discoverHarnessModels`, `loadRoutingConfig`, `saveProjectRoutingConfig` |
| **세션 관리** | 생성, CLI↔ASD 승격/하강, 삭제, 메모, 전경 전환, phase 변경, 4분류 트랜스크립트 복원 | `handlePlan`, `promoteCliSessionToAsd`, `unlinkHarnessSessionFromAsdSession`, `saveSessionMemo`, `readHarnessTranscript` |
| **PLAN·워크플로우** | PLAN 칸반 보드, `PLAN.md` 편집/충돌체크, Turn 실행, Debate 실행 | `collectDashboardSnapshot`, `syncPlanFromMd`, `handleDispatch`, `handleDebate` |
| **Diff·승인·서브에이전트** | Worktree Diff 조회, 태스크 승인/반려, 서브에이전트 실시간 로그 tail 및 kill/retry/fallback/resume | `getWorktreeDiff`, `approvePendingTask`, `rejectPendingTask`, `scanSubagentLogs`, `runSubagentControlAction` |
| **터미널 연동** | OS 터미널(WT/PowerShell) 즉시 열기, resume 명령 복사, 라이브 로그 뷰 | `buildInteractiveResumeHarnessCommand` |

### 5.2 구현 시퀀스 (새 세션 실행 기준)

- **[완료] M1**: 오케스트레이터 서버 기반(`src/server/*`, `src/shared/events.ts`, v2.6.0 `asd-runtime` 연동) + Claude 1하네스 웹 프로토타입 및 E2E 검증 완료 (`asd-20261113-104918-ln8l`)
- **[예정] M2 — 웹 프론트 정리 & 5하네스·프로젝트·워크플로우 서버 API 완성**:
  1. `src/web/`, `index.html`, `vite.config.ts` 및 Vite/React 프론트 의존성 삭제 (`npm` 서버/테스트 전용 정리)
  2. `src/server/`에 5하네스 SSE 정규화(Codex `turn.completed`/OpenCode `step_finish`/Cursor/Antigravity usage·세션 ID·중복 처리) 및 `asd-gui` 이식 API(`/api/projects/*`, `/api/models`, `/api/routing`, `/api/sessions/*`, `/api/tasks/*`, `--port 0` 사이드카 ready 출력) 구현
  3. 5하네스 스키마 리플레이 테스트 + 선택/전체 라이브 5하네스 E2E 테스트 + [testing-guide.md](file:///C:/Users/yjsjs0987/Desktop/Projects/ASD-Nexus/docs/testing-guide.md) 갱신
- **[예정] M3 — Rust 워크스페이스 (`native/`) + 공유 HTTP/SSE 클라이언트 + 터미널 TUI/CLI (`ratatui`)**:
  1. `native/Cargo.toml` 워크스페이스 및 `asd-nexus-client` (Node 사이드카 자동 실행·ready 대기·종료, 5하네스 REST/SSE 스트림 클라이언트) 구현
  2. `asd-nexus-cli` (`ratatui` + `crossterm` 순수 흑백 TUI + CLI 진입점: 프로젝트 선택, 세션 탐색/승격, 5하네스 실시간 스트리밍 채팅, 보드/서브에이전트 모니터링 오버레이) 구현 및 스모크 테스트
- **[예정] M4 — 순수 Rust 데스크톱 네이티브 GUI (`eframe`/`egui` + `rfd`, 흑백 모노크롬)**:
  1. `asd-nexus-desktop`: WebView 없는 OS 네이티브 창, 시스템 한글 폰트 로딩, 순수 흑백 모노크롬 테마, `rfd` 네이티브 폴더 선택 대화상자로 프로젝트 변경 시 즉시 세션 조회·시작
  2. 좌측 프로젝트/세션 레일 · 중앙 5하네스 4분류 스트리밍 채팅/모델 피커 · PLAN 칸반/PLAN.md 에디터/Worktree Diff/서브에이전트 인스펙터/라우팅 패널/OS 터미널 열기 구현
  3. `cargo build --release` 바이너리 빌드 및 실제 UI 스크린샷·5하네스 E2E 통합 검증

*(검증 환경 메모: PC에 `rustc`/`cargo` `stable-x86_64-pc-windows-msvc` + VS 2022 `MSVC 14.44 link.exe` + `Windows Kits 10` 완비 확인. `claude` 로컬 프록시 사용 시 `http://127.0.0.1:4000` 기동 및 `~/.asd/routing.json` 미설정 시 사용 가능 모델 지정 필요.)*

---

## 부록 A. 실측 증거 발췌

```
$ codex exec --json "Reply with exactly: OK" --skip-git-repo-check --sandbox read-only
{"type":"thread.started","thread_id":"01a1b12a-..."}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"OK"}}
{"type":"turn.completed","usage":{"input_tokens":18720,...}}

$ opencode run "Reply with exactly: OK" --format json
{"type":"step_start",...,"sessionID":"ses_..."}
{"type":"text","part":{"type":"text","text":"OK",...}}
{"type":"step_finish","part":{...,"tokens":{...},"cost":0}}

$ claude -p "..." --output-format stream-json --verbose
{"type":"system","subtype":"init",...,"session_id":"...","tools":[...]}
{"type":"system","subtype":"api_retry",...}   ← 모델 프록시 오류로 턴 미완주, 프로토콜은 정상
```

## 부록 B. 알려진 갭/리스크

1. Codex 구세션 `.jsonl.zst` — 히스토리 전체 로드 시 zstd 복원 필요 (**해결 확인**: `core/transcripts.ts`의 `decompressZstdBuffer`가 `node:zlib` `zstdDecompressSync` + `zstd -dc` CLI 폴백으로 `.jsonl.zst`를 자동 복원함).
2. Claude 세션 JSONL 공식 "포맷 불안정" — 파서 회귀테스트 필요.
3. OpenCode `opencode.db` 2.29GB — 전체 로드 금지, 세션 스코프 쿼리만.
4. agy 어댑터의 stream-json 플래그 미사용 — 라이브 실측으로 확정, 부록 C 참조.

---

## 부록 C. ASD-Nexus 스트리밍 미작동 원인 (2026-11-06 라이브 실측 확정)

### 원인 매트릭스

| 하네스 | 현재 실행 명령(어댑터) | 실측 결과 | 판정 |
|---|---|---|---|
| Claude Code | 어댑터 `--output-format json` → 코어가 stream-json으로 승격 (`drivers.ts:1066-1073`, claude-code 전용) | 블록 단위 스트리밍 정상 | ✅ 정상 |
| Codex | `codex exec --json` (codex.ts:25) | JSONL 이벤트 실시간 (데모 완주) | ✅ 정상 |
| Antigravity | `agy -p --output-format json` (antigravity.ts:57 하드코딩) | 최종 1회 단일 JSON만 출력. 반면 `--output-format stream-json`은 `step_update`/`text_delta` 토큰 델타(~25ms 간격) 완벽 실측 | ❌ 플래그 + 파서 수정 필요 |
| Cursor | `agent -p --output-format json` (cursor.ts:71 하드코딩) | 최종 1회. 코어의 stream-json 승격이 claude-code 전용이라 cursor는 미적용 | ❌ 플래그 + 파서 수정 필요 |
| OpenCode | `run --format json` (opencode.ts:38) | **part 완료 단위** — 15문장 전체가 단일 `text` 이벤트로 도착(토큰 델타 없음). MCP 콜드부트 ~28s 후에야 첫 이벤트 | ⚠️ 구조적 한계 → serve+SSE 필요 |

### 핵심 사실

1. **원인은 GUI(asd-gui)가 아니라 코어 어댑터/파서다.** GUI의 스트리밍 파이프라인(`createGuiDriverCommandRunner` → WS `driver_stream_event` → `useWsDriverStreamEvent`)은 정상이며, 이벤트가 도착하면 전파된다. 문제는 "애초에 들어오는 이벤트가 없는 것".
2. agy 파서(`parseDriverStreamLine` 868-921행)가 기대하는 `PLANNER_RESPONSE`/`TOOL_RESPONSE`는 transcript.jsonl 포맷이고, CLI `--output-format stream-json`의 실측 스키마(`{"event":"init"|"step_update"|"result", "text_delta":"..."}`)와 불일치. 플래그만 바꿔도 파싱이 안 됨.
3. `EXTERNAL_SDK_REFERENCE.md`의 "Claude/Cursor stream-json 자동 활성화" 주석은 코드와 불일치(실제로는 claude-code만 승격).
4. OpenCode는 `run --format json`이 part 완료 시점에만 이벤트를 내보내는 CLI 구현 특성 → 토큰 단위 실시간은 `opencode serve` + SSE `/api/event/subscribe`(`session.message.part.updated` 델타)로만 가능.

### 수정 목록 (국소 수정 권고 — 백지화 불필요)

1. `src/adapters/antigravity.ts`: `--output-format json` → `stream-json` (라이브 검증됨)
2. `src/adapters/cursor.ts`: `--output-format json` → `stream-json` (+ `--stream-partial-output` 옵션 검토)
3. `src/core/drivers.ts`: claude-code 전용 stream-json 승격 분기 제거, 어댑터가 처음부터 올바른 포맷을 사용하도록 일원화
4. `src/core/drivers.ts::parseDriverStreamLine`: agy 실측 스키마(`event: init|step_update|result`, `text_delta`) 추가, cursor `tool_call`(started/completed) 추가
5. OpenCode: v1은 블록 단위 수용 + `--attach` 웜서버로 콜드부트 완화 → v2에서 `opencode serve` SSE 연동으로 토큰 스트리밍
6. `EXTERNAL_SDK_REFERENCE.md` 주석을 실제 동작과 일치시킴

### 실행 상태 (2026-11-06)

- [x] 1-2. agy·Cursor 스트리밍 플래그 — `buildHeadless/ResumeHarnessCommand`(session-tracker)에 `streaming` 옵션 추가, 하네스별 `stream-json` 분기. 비스트리밍 기본값(json)은 서브에이전트·debate 등 배치 경로 보호용으로 유지.
- [x] 3. drivers.ts claude 전용 splice 제거 — `runDriverTurn`이 `streaming: useStreamJson`을 빌더에 직접 전달(포맷 결정이 빌더로 일원화).
- [x] 4. 파서 — Cursor 실측 스키마(톱레벨 thinking 델타, `tool_call.<family>ToolCall` started/completed, user 프롬프트 에코 스킵) + agy stream-json(`step_update` 델타/`result`/`init`) 추가. 라이브 캡처 바이트 기반 단위 검증 통과(카테고리 전건 정확).
- [x] 6. `EXTERNAL_SDK_REFERENCE.md` 주석 갱신.
- [ ] 5. OpenCode serve+SSE — 미착수(별도 과제, 앱 계획 M2).
- 검증: Core 355/356(유일 실패 = 기존 유령 테스트 installer-sync), asd-gui 14/14. 미커밋.
