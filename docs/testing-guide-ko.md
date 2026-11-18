# ASD-Nexus 수동 테스트 가이드 (한국어)

이 문서는 ASD-Nexus를 직접 실행해서 확인하는 방법을 순서대로 정리한 것입니다.  
영문판은 `docs/testing-guide.md`에 있습니다.  
기준 환경: Windows 10/11 + PowerShell 5.1, Node.js 22.13+, Rust stable.  
모든 명령은 ASD-Nexus 프로젝트 루트에서 실행합니다.

```powershell
Set-Location "C:\path\to\ASD-Nexus"
```

> 핵심 명령과 기대 결과는 실제 실행으로 확인했습니다.  
> 취소/토큰 인증 항목은 서버 코드 기준으로 작성했습니다.  
> 상세 자동 검증 기록은 `docs/verification-report.md`, E2E 원본 데이터는 `docs/evidence/e2e-5harness-summary.json`에 있습니다.

---

## 0. 사전 준비

### 0-1. 필수 도구 확인

```powershell
node --version          # v22.13.0 이상
cargo --version
curl.exe --version      # Windows 기본 포함
```

5개 하네스 CLI가 PATH에 있어야 라이브 E2E를 돌릴 수 있습니다.

```powershell
where.exe claude codex opencode agy agent
```

### 0-2. Claude Code 프록시 확인 (라이브 E2E용)

claude-code를 로컬 프록시 경유로 사용하는 경우, 프록시가 실행 중인지 확인합니다.  
프록시를 사용하지 않으면 이 단계는 건너뜁니다. 아래 주소는 예시이므로 환경에 맞게 바꾸세요.

```powershell
curl.exe -s -o NUL -w "%{http_code}`n" "http://127.0.0.1:4000/health"
```

- `200` → 정상
- `000` 또는 연결 오류 → 프록시가 꺼진 상태

프록시 실행 방법은 사용하는 프록시에 따라 다릅니다. 예:

```powershell
node "<프록시-스크립트-경로>"
```

실행 중인 node 프로세스를 확인하려면:

```powershell
Get-CimInstance Win32_Process -Filter "Name='node.exe'" |
  Select-Object ProcessId, CommandLine
```

> 라이브 E2E에서 claude-code를 기본값 `haiku`로 실행하려면 이 프록시가 필요합니다. 프록시를 쓰지 않으면 5-3에서 다른 모델로 바꾸세요.

### 0-3. 모델 주의사항

`nvidia/z-ai/glm-5.3-flash` 라우트가 upstream 404를 반환하는 경우가 있어, E2E 기본값은 다음처럼 GLM 라우트를 피합니다.

| 하네스 | 기본 모델 |
|---|---|
| claude-code | `haiku` (프록시 경유) |
| codex | `gpt-6-luna` |
| opencode | `opencode/glm-5.3-flash-free` |
| antigravity | `gemini-3.8-flash-low` |
| cursor | `default` |

---

## 1. 빠른 점검 (오프라인, 약 1~2분)

### 1-1. TypeScript 타입 체크

```powershell
npm run typecheck
```

기대: 출력 마지막에 에러 없이 종료(exit code 0).

### 1-2. Node 오프라인 테스트 73개

```powershell
node --no-warnings --test tests/normalizer-replay.test.ts tests/sdk-surface.test.ts tests/server-api.test.ts tests/model-catalog.test.ts
```

기대:

```text
tests 73
pass 73
fail 0
duration_ms 약 1,260
```

### 1-3. Rust 워크스페이스 테스트 80개

```powershell
cargo test --manifest-path native/Cargo.toml --workspace --locked
```

기대: 아래 합계 **80 passed, 0 failed**.

- `asd-nexus-client`: 11
- `asd-nexus-cli`: 30
- `asd-nexus-desktop`: 39

---

## 2. 사이드카 서버 직접 테스트

### 2-1. 서버 실행

**별도 PowerShell 창**을 열고 실행합니다. 이 창은 테스트가 끝날 때까지 닫지 마세요.  
(표준 입력이 닫히면 사이드카가 스스로 종료되므로, 리다이렉션 없이 일반 터미널에서 실행해야 합니다.)

```powershell
Set-Location "C:\path\to\ASD-Nexus"
node --no-warnings src/server/index.ts --port 3847 --project-root .
```

기대: 아래 형태의 JSON 한 줄이 출력되고 서버가 계속 떠 있음.

```json
{"protocol":"asd-nexus-sidecar","protocolVersion":1,"pid":12345,"host":"127.0.0.1","port":3847,"baseUrl":"http://127.0.0.1:3847","projectRoot":".","version":"2.6.0","tokenRequired":false,"startedAt":"..."}
```

이제 **또 다른 PowerShell 창**에서 아래 API를 확인합니다.

```powershell
$base = "http://127.0.0.1:3847"
$root = [uri]::EscapeDataString((Get-Location).Path)
```

### 2-2. Health

```powershell
curl.exe -s "$base/api/health"
```

기대:

```json
{"ok":true,"version":"2.6.0","defaultProjectRoot":".","pid":12345}
```

### 2-3. 프로젝트 / 모델 / 라우팅

```powershell
curl.exe -s "$base/api/projects"
curl.exe -s "$base/api/models"
curl.exe -s "$base/api/routing?projectRoot=$root"
```

기대: 프로젝트 목록, 모델 카탈로그(5개 하네스), `routing.jsonc` 기반 라우팅 설정이 JSON으로 반환됩니다.

`POST /api/models/refresh`는 CLI 프로브가 일부 모델을 놓쳐도 마지막 캐시와 병합해 기존 모델을 유지합니다. 프로브는 최대 15초까지 기다립니다.

### 2-4. 진행 보드 / PLAN

```powershell
curl.exe -s "$base/api/board?projectRoot=$root"
curl.exe -s "$base/api/plan?projectRoot=$root"
```

기대:

- `/api/board`: `sessionId`, `phase`, `tasks` 배열. 태스크가 있으면 `progress`(예: `"7/8"`), 없으면 `progress`가 생략되고 `tasks`가 빈 배열입니다.
- `/api/plan`: `exists: true`, `planMdPath`가 `.asd\PLAN.md`를 가리킴, `content`에 PLAN 본문

> 보드는 현재 포그라운드 세션 기준입니다. 새 턴을 실행해 새 세션이 생기면 보드가 그 세션 기준으로 바뀌어 `tasks`가 비어 보일 수 있습니다. PLAN 전체 진행률은 `asd status` 또는 `asd plan get --json`으로 확인하세요.

### 2-5. 턴 실행 (SSE 스트리밍)

요청 본문을 임시 파일로 만들어 전송합니다. PowerShell 따옴표 이스케이프 문제를 피하려면 이 방법을 권장합니다.

```powershell
$body = Join-Path $env:TEMP "asd-turn.json"
$payload = @{
  projectRoot = (Get-Location).Path
  harness     = "opencode"
  model       = "opencode/glm-5.3-flash-free"
  prompt      = "Remember the calibration code AMBER-42. Reply with only OK."
} | ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($body, $payload)

curl.exe -s -N -X POST "$base/api/turn/stream" `
  -H "Content-Type: application/json" `
  --data-binary "@$body"
```

기대: `data: {...}` 줄이 순서대로 출력되고, 마지막이 `status: done`.

```text
data: {"kind":"status","status":"running","turnId":"turn_...","sessionId":"asd-...","harness":"opencode",...}
data: {"kind":"status","status":"running","sessionId":"ses_...","harness":"opencode",...}
data: {"kind":"text","text":"OK",...}
data: {"kind":"usage",...}
data: {"kind":"status","status":"done",...,"raw":{"asdSessionId":"asd-...","harnessSessionId":"ses_..."}}
```

확인할 것:

- 첫 이벤트의 `sessionId`가 `asd-`로 시작
- `text` 이벤트에 실제 응답 텍스트 포함
- `usage` 이벤트에 토큰/비용 정보 포함
- 마지막 `status`가 `done`이고 `raw.harnessSessionId`가 존재

### 2-6. 턴 2회차 (세션 재개 + 기억 확인)

위 출력에서 `asd-...` 세션 ID를 복사한 뒤, 같은 세션으로 두 번째 턴을 보냅니다.

```powershell
$sid = "asd-..."   # 위에서 복사한 값으로 교체
$body = Join-Path $env:TEMP "asd-turn.json"
$payload = @{
  projectRoot = (Get-Location).Path
  sessionId   = $sid
  harness     = "opencode"
  model       = "opencode/glm-5.3-flash-free"
  prompt      = "What calibration code did I ask you to remember? Reply with only the code."
} | ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($body, $payload)

curl.exe -s -N -X POST "$base/api/turn/stream" `
  -H "Content-Type: application/json" `
  --data-binary "@$body"
```

기대: `text` 이벤트에 `AMBER-42`가 포함됩니다.  
(하네스가 세션을 제대로 재개했다는 증거)

### 2-7. 세션 목록 / 트랜스크립트 복원

```powershell
curl.exe -s "$base/api/sessions?projectRoot=$root&syncLocalTranscripts=true"
```

기대: `sessions` 배열에 방금 실행한 하네스 세션이 `harnessSessionId`와 함께 보입니다.

트랜스크립트는 하네스 세션 ID로 조회합니다.

```powershell
$hid = "ses_..."   # done 이벤트의 raw.harnessSessionId 또는 sessions 응답에서 복사
curl.exe -s "$base/api/transcript/opencode/$hid?projectRoot=$root"
```

기대: `messages` 배열에 최소 `user`, `assistant` 카테고리가 포함됩니다.

### 2-8. 턴 취소 (선택)

취소는 스트리밍 중인 턴의 `turnId`가 필요합니다. 터미널 A에서 긴 작업을 실행하고, 터미널 B에서 취소합니다.

터미널 A:

```powershell
$base = "http://127.0.0.1:3847"
$body = Join-Path $env:TEMP "asd-turn.json"
$payload = @{
  projectRoot = (Get-Location).Path
  harness     = "opencode"
  model       = "opencode/glm-5.3-flash-free"
  prompt      = "Count from 1 to 200 slowly, one number per line."
} | ConvertTo-Json -Compress
[System.IO.File]::WriteAllText($body, $payload)
curl.exe -s -N -X POST "$base/api/turn/stream" -H "Content-Type: application/json" --data-binary "@$body"
```

터미널 A의 첫 이벤트에서 `turnId`(`turn_...`)를 복사한 뒤, 터미널 B:

```powershell
$base = "http://127.0.0.1:3847"
$cancel = Join-Path $env:TEMP "asd-cancel.json"
[System.IO.File]::WriteAllText($cancel, '{"turnId":"turn_..."}')   # 실제 turnId로 교체
curl.exe -s -X POST "$base/api/turn/cancel" -H "Content-Type: application/json" --data-binary "@$cancel"
```

기대:

- 취소 응답: `{"success":true,"message":"Turn cancelled successfully"}`
- 터미널 A 스트림: `status: interrupted`로 종료

### 2-9. 토큰 인증 (선택)

서버를 `--token`과 함께 실행하면 모든 API에 Bearer 토큰이 필요합니다.

```powershell
node --no-warnings src/server/index.ts --port 3848 --project-root . --token test123
```

```powershell
curl.exe -s -i "http://127.0.0.1:3848/api/health"                                  # 401
curl.exe -s -H "Authorization: Bearer test123" "http://127.0.0.1:3848/api/health"  # 200
```

### 2-10. 서버 종료

사이드카를 실행한 창에서 `Ctrl+C`.  
다른 창에서 확인:

```powershell
Get-Process node -ErrorAction SilentlyContinue
```

테스트 전후 node 프로세스 수가 크게 늘어나 있으면 누수이므로, 남은 사이드카를 종료합니다.

---

## 3. CLI 수동 테스트

먼저 릴리스 바이너리를 빌드합니다(이미 빌드되어 있으면 생략 가능).

```powershell
cargo build --release --manifest-path native/Cargo.toml --locked
```

### 3-1. 스크립트 모드

```powershell
.\native\target\release\asd-nexus-cli.exe health --json --project-root .
.\native\target\release\asd-nexus-cli.exe sessions --json --project-root .
.\native\target\release\asd-nexus-cli.exe board --json --project-root .
```

기대:

- `health --json`: `{"ok":true,"version":"2.6.0",...}`
- `sessions --json`: 세션 배열
- `board --json`: 진행 보드 JSON(포그라운드 세션/태스크에 따라 내용이 달라짐)

CLI는 `--server-url`을 주지 않으면 사이드카를 자동으로 띄우고 종료 시 정리합니다.

### 3-2. 대화형 TUI (OpenCode 스타일 UX)

```powershell
.\native\target\release\asd-nexus-cli.exe --project-root .
```

주요 키 & 슬래시/멘션/쉘 기능:

| 키 / 입력 | 동작 |
|---|---|
| `Enter` / `Shift+Enter` · `Alt+Enter` · `Ctrl+J` | 프롬프트 전송 / 멀티라인 줄바꿈 |
| `Ctrl+P` | 커맨드 팔레트 모달 |
| `/` (입력창) | `[nexus]` 내장 명령(`/cd`, `/sessions`, `/new`, `/models`, `/harness`, `/board`, `/diff`, `/subagents`, `/routing`, `/terminal`, `/theme`, `/sidebar`, `/clear`, `/help`, `/exit`) + 현재 하네스 고유 `/명령어` 팝업 |
| `@` (입력창) | 워크스페이스 상대 경로 파일 자동완성 |
| `!<명령>` (입력창) | 현재 `projectRoot` 기준 로컬 쉘 실행 및 타임라인 카드 기록 |
| `Tab` / `Shift+Tab` | 팝업 활성 시 자동완성, 기본 입력창에서는 하네스 순환 (사이드바 오픈 시 포커스 이동) |
| `Ctrl+X` | 진행 중인 턴 취소 |
| `Ctrl+K` | 하네스/모델 선택 |
| `Ctrl+B` | 진행 보드 + PLAN |
| `Ctrl+L` | 서브에이전트 로그 |
| `Ctrl+D` | 워크트리 diff |
| `Ctrl+R` | 라우팅 편집 |
| `Ctrl+T` | 터미널/재개 명령 |
| `Ctrl+E` | thinking 접기/펼치기 |
| `F1` | 도움말 |
| `Ctrl+Q` / `Ctrl+C` | 종료 |

확인할 것: Home/Active 개방형 레이아웃, `/cd`·`/sessions`·`/sidebar` 전환, 하네스 전환 시 새 세션 자동 생성, 하네스별 `/명령어`, `@파일`·`!쉘`, 마우스 휠 스크롤/클릭, `/theme` 다크 컬러 ↔ 흑백 전환.

### 3-3. 이미 떠 있는 사이드카에 붙기

2장에서 사이드카를 `3847` 포트로 띄워 두었다면:

```powershell
.\native\target\release\asd-nexus-cli.exe health --json --server-url http://127.0.0.1:3847
.\native\target\release\asd-nexus-cli.exe --project-root . --server-url http://127.0.0.1:3847
```

토큰을 설정했다면 `--token test123`도 추가합니다.

---

## 4. 데스크톱 GUI 수동 테스트

### 4-1. 헤드리스 검증 (창 없음)

```powershell
.\native\target\release\asd-nexus-desktop.exe --verify --project-root .
```

기대(exit code 0):

```json
{
  "ok": true,
  "spawnedSidecar": true,
  "font": "font: malgun.ttf",
  "monochromeVisuals": true,
  "planLoaded": true,
  "routingLoaded": true,
  "modelHarnesses": 5,
  "tabsRendered": ["Chat","PLAN & Board","Worktree Diff","Subagents","Routing","Terminal & Resume"]
}
```

추가로 `frames`, `projects`, `sessions`, `boardTasks`, `health` 필드가 출력됩니다.

### 4-2. 실제 창 실행 (Antigravity 스타일)

```powershell
.\native\target\release\asd-nexus-desktop.exe --project-root .
# 흑백 모드로 바로 시작하려면:
.\native\target\release\asd-nexus-desktop.exe --project-root . --theme mono
```

확인할 것:

- 상단 바: `Chat` | `Dashboard` 모드 분리(`Ctrl+1` / `Ctrl+2`), `Ctrl+P` 커맨드 팔레트, `Dark | Mono` 런타임 테마 전환
- 왼쪽 접이식 사이드바: `WORKSPACES` 선택 ComboBox + `Open…` / `↻` / `+ New Chat` + 세션 검색/목록
- `Chat` 모드: 중앙 타임라인(`Thought for …` 아코디언, 도구 카드, 마크다운 응답), 하단 플로팅 컴포저 내부 `[Harness] [Model] [Dir] [Send|Stop]` 툴바, `/`·`@` 자동완성 팝업(선택 시 `Tab`/`Enter` 적용, `Ctrl+Enter` 즉시 전송)
- 하네스 수동 변경 시 이전 바인딩 초기화 후 새 세션 자동 생성
- `Dashboard` 모드: `PLAN & Board`, `Worktree Diff`, `Subagents`, `Routing`, `Terminal & Resume` 서브탭
- 입력창: `Enter` 전송, `Shift+Enter` 줄바꿈, 내용이 길어지면 입력창 내부 스크롤(레이아웃 오버플로우 없음)
- 모델 새로고침 후에도 기존 모델이 사라지지 않음(서버/클라이언트 카탈로그 병합)
- 릴리스 실행 시 별도 콘솔 창이 뜨지 않음(디버그 빌드는 로그 확인을 위해 콘솔 유지)
- 기본 `Dark Color` 및 전환된 `Monochrome`(순수 흑백 `r == g == b`) 양쪽 모두에서 `malgun.ttf` 한글이 깨지지 않음
- 창을 닫은 뒤 `Get-Process node`로 사이드카가 정리됐는지 확인

### 4-3. 스크린샷 캡처 (Dark Color & Monochrome)

```powershell
.\native\target\release\asd-nexus-desktop.exe --project-root . `
  --screenshot "$env:TEMP\asd-desktop-dark.bmp" --exit-after 4
.\native\target\release\asd-nexus-desktop.exe --project-root . --theme mono `
  --screenshot "$env:TEMP\asd-desktop-mono.bmp" --exit-after 4
```

BMP를 PNG로 변환하려면:

```powershell
Add-Type -AssemblyName System.Drawing
$img = [System.Drawing.Image]::FromFile("$env:TEMP\asd-desktop-dark.bmp")
$img.Save("$env:TEMP\asd-desktop-dark.png", [System.Drawing.Imaging.ImageFormat]::Png)
$img.Dispose()
$mImg = [System.Drawing.Image]::FromFile("$env:TEMP\asd-desktop-mono.bmp")
$mImg.Save("$env:TEMP\asd-desktop-mono.png", [System.Drawing.Imaging.ImageFormat]::Png)
$mImg.Dispose()
```

기대: 1440x900 스크린샷, Monochrome 캡처는 모든 픽셀이 `R == G == B`(비회색 0개).

---

## 5. 라이브 5하네스 E2E

### 5-1. 전체 실행 (약 82초)

```powershell
node --no-warnings --test tests/live-5harness-e2e.test.ts
```

기대:

```text
tests 5
pass 5
fail 0
```

각 하네스는 다음을 실제 CLI로 수행합니다.

1. Turn 1 SSE 스트리밍 (AMBER-42 기억 요청)
2. Turn 2 세션 재개 후 AMBER-42 회상
3. `/api/sessions`에서 세션 발견
4. `/api/transcript/:harness/:id`에서 정규화된 트랜스크립트 복원

결과는 `docs/evidence/e2e-5harness-summary.json`에 자동 기록됩니다.

### 5-2. 한 하네스만 실행

```powershell
node --no-warnings --test --test-name-pattern="opencode" tests/live-5harness-e2e.test.ts
```

`"claude-code"`, `"codex"`, `"antigravity"`, `"cursor"`로 바꿔 실행할 수 있습니다.  
(1개만 실행하면 evidence JSON은 덮어쓰지 않습니다.)

### 5-3. 모델 오버라이드

```powershell
$env:ASD_E2E_MODEL_CLAUDE = "haiku"
$env:ASD_E2E_MODEL_CODEX = "gpt-6-luna"
$env:ASD_E2E_MODEL_OPENCODE = "opencode/glm-5.3-flash-free"
$env:ASD_E2E_MODEL_ANTIGRAVITY = "gemini-3.8-flash-low"
$env:ASD_E2E_MODEL_CURSOR = "default"
node --no-warnings --test tests/live-5harness-e2e.test.ts
```

환경 변수는 현재 PowerShell 세션에만 적용됩니다. 새 창에서는 다시 설정해야 합니다.

---

## 6. 전체 릴리스 게이트

```powershell
npm run typecheck
npm test
cargo test --manifest-path native/Cargo.toml --workspace --locked
cargo build --release --manifest-path native/Cargo.toml --locked
```

기대:

| 단계 | 기대 결과 |
|---|---|
| `npm run typecheck` | exit 0 |
| `npm test` | Node 78/78 (오프라인 73 + 라이브 5) |
| `cargo test ... --locked` | Rust 80/80 |
| `cargo build --release ... --locked` | exit 0, `asd-nexus-cli.exe` / `asd-nexus-desktop.exe` 생성 |

`npm test`는 라이브 E2E를 포함하므로 프록시와 5개 CLI가 준비되어 있어야 합니다.

---

## 7. 문제 해결

| 증상 | 원인 | 해결 |
|---|---|---|
| 사이드카가 ready JSON만 출력하고 바로 종료 | 표준 입력이 닫힌 상태(리다이렉션/파이프) | 일반 PowerShell 창에서 직접 실행 |
| claude-code E2E가 `ECONNREFUSED` | `127.0.0.1:4000` 프록시 꺼짐 | 프록시 시작 또는 `claude` 한 번 실행 |
| `nvidia/z-ai/glm-5.3-flash` 404 | upstream provider 장애 | `haiku` / `opencode/glm-5.3-flash-free` 사용 |
| opencode 테스트 후 temp 폴더가 남음 | Windows에서 opencode LSP가 잠시 잠금 | 잠시 후 수동 삭제하거나 재실행 |
| 포트 충돌 | 3847 사용 중 | 다른 포트 사용 또는 `--port 0` |
| API가 401 | `--token` 설정됨 | `-H "Authorization: Bearer <token>"` 추가 |
| `asd verify`가 60초 타임아웃 | 전체 게이트는 60초 초과 | `--cmd`에 오프라인 테스트만 지정 |
| 데스크톱이 sidecar spawn 실패 | node 경로/스크립트 경로 문제 | `--node`, `--server-script` 옵션 사용 |

---

## 8. 최종 체크리스트

- [ ] `npm run typecheck` exit 0
- [ ] Node 오프라인 73/73
- [ ] 라이브 5하네스 5/5 (claude-code `haiku`, opencode free 모델)
- [ ] Rust 80/80
- [ ] `cargo build --release --locked` 성공
- [ ] `asd-nexus-desktop.exe --verify` → `"ok": true`
- [ ] 데스크톱 창에서 `Chat`/`Dashboard` 분리 + 한글 + Dark Color / 순수 흑백 전환 확인
- [ ] 입력창 `Enter` 전송 / `Shift+Enter` 줄바꿈 / `/`·`@` 팝업 자동완성 / 긴 입력 스크롤 확인
- [ ] 모델 새로고침 후 모델 수가 줄지 않는지 확인
- [ ] 릴리스 실행 시 콘솔 창이 뜨지 않는지 확인
- [ ] `asd-nexus-cli.exe health --json` → `"ok": true`
- [ ] 테스트 후 남은 node 사이드카 없음

이 체크리스트가 모두 통과하면 `docs/verification-report.md`의 최종 검증과 동일한 상태입니다.
