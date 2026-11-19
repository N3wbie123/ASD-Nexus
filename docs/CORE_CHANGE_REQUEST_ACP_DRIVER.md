# Core 수정 요청서 (CR-ACP-DRIVER): 상주형 ACP 메인 드라이버 & 대화형 권한 승인 연동

> **발신**: `ASD-Nexus` (사이드카 서버 + 네이티브 Rust GUI/TUI)  
> **수신**: `asd-multiagent` (ASD Core v2.7.0+, `agent-swarm-director`)  
> **관련 문서**: `asd-multiagent/docs/ACP_ROADMAP.md` (§4 G1~G4), `docs/CORE_GUI_BOUNDARY_AUDIT.md`  
> **상태**: **완료 (Core `c795d22` / `7e1b42c` / `1dccb52` 반영 및 `ASD-Nexus` 전 구간 통합·E2E 검증 완료)**

---

## 1. 배경 및 Core 수정이 필요한 이유

`ASD-Nexus`는 `tests/sdk-surface.test.ts`를 통해 `src/` 내 직접적인 `child_process` import를 금지하고 모든 프로세스 스폰을 `agent-swarm-director` SDK 경유로만 수행합니다. 현재 Core v2.7.0 루트(`src/index.ts`)에는 `acp-types.ts` 및 `acp-client.ts`가 export 되어 있으나 다음 4가지 확장이 필요합니다.

1. **`session/request_permission` 동기 자동 응답 고정 (`src/core/acp-client.ts:1198-1216`)**
   - `AcpStdioClient.handleLine`이 `session/request_permission` 수신 시 무조건 `evaluateAcpPermissionRequest()`를 동기로 호출해 즉시 응답하므로, GUI/TUI 승인 모달 응답을 비동기로 대기(`await`)할 수 없습니다.
2. **하네스 ACP 실행 인자 내 권한 우회 플래그 고정 (`src/core/acp-client.ts:254-264`)**
   - `probeAcpCapability("antigravity")`가 `["acp", "--dangerously-skip-permissions"]`를 고정 반환하여, 사용자가 자동 승인(`autoApprove`)을 끈 상태에서도 하네스가 권한 요청을 생략합니다.
3. **상주 세션 도중 모델 변경 공개 API 부재 (`src/core/acp-client.ts:989`)**
   - `tryConfigureSessionModel`이 `private`이고 `session/new`·`session/load` 응답의 `availableModels`/`configOptions` 카탈로그를 인스턴스에 보관하지 않아, 이미 열려 있는 상주 ACP 세션에서 모델만 변경할 때 Core의 슬러그/에포트 매칭(`resolveBestAcpModelId`)을 재사용할 수 없습니다.
4. **`runDriverTurn` 단발 CLI 스폰 구조 유지 (`src/core/drivers.ts:1319-1579`, `ACP_ROADMAP.md` G1~G4 미착수)**
   - `runDriverTurn` 내부 실행기가 `executeHarnessProcess`(매 턴 단발 CLI 스폰)로만 연결되어 있어 `autoApprove` 및 `onPermissionRequest` 옵션을 아직 소비하지 않습니다.

---

## 2. 요청 변경 사항 상세

### 요청 A. `src/core/acp-client.ts` & `src/core/acp-types.ts` 확장

1. **비동기 권한 승인 핸들러 콜백 (`onPermissionRequest`) 추가**
   - `AcpStdioClientOptions`에 아래 옵션을 추가:
     ```ts
     onPermissionRequest?: (
       params: AcpRequestPermissionParams
     ) => Promise<AcpRequestPermissionResult>;
     ```
   - `handleLine`에서 `method === "session/request_permission"` 수신 시:
     - 보호 경로(`.git/`, `.asd/`, `.env*` 등) 위반이 명백한 경우 즉시 거부하고, 그 외 `onPermissionRequest`가 지정된 경우 비동기로 결과를 대기(`await`)해 JSON-RPC 응답 프레임을 전송.
     - 대기 중 세션이 취소(`cancelSession`)되거나 클라이언트가 종료(`close`)되면 `{ outcome: { outcome: "cancelled" } }`로 안전 정리.
     - `onPermissionRequest` 미지정 시에는 기존 `evaluateAcpPermissionRequest` 동기 동작 100% 유지(서브에이전트 하위 호환 보장).

2. **`probeAcpCapability` 수동 권한 모드 옵션 추가**
   - 시그니처 확장 (하위 호환 유지):
     ```ts
     export function probeAcpCapability(
       harness: HarnessId,
       env?: Record<string, string | undefined>,
       options?: { interactivePermissions?: boolean }
     ): AcpHarnessReadiness;
     ```
   - `options?.interactivePermissions === true`일 때 `antigravity`의 `--dangerously-skip-permissions` 등 자동 권한 스킵 플래그를 제외하여 하네스가 `session/request_permission`을 정상 발행하도록 처리.

3. **상주 세션 내 실시간 모델 변경 메서드 공개 (`setSessionModel`)**
   - `startOrLoadSession` 수행 시 획득한 `availableModels`와 `configOptions`를 `AcpStdioClient` 내부 필드에 캐시.
   - 공개 메서드 추가:
     ```ts
     async setSessionModel(
       sessionId: string,
       requestedModelId: string,
       thinkingEffort?: string | null
     ): Promise<void>;
     ```
   - 내부적으로 캐시된 카탈로그와 `resolveBestAcpModelId`를 사용해 `session/set_model` (실패 시 `session/set_config_option`) 호출.

4. **프로세스 생존 확인 getter 추가**
   - `get isConnected(): boolean` (`!this.closed && this.child !== null && this.child.exitCode === null`) 공개.

---

### 요청 B. `src/core/drivers.ts` — 상주형 ACP 드라이버 세션 풀 (`ACP_ROADMAP.md` G1~G4)

1. **`DriverSessionPool` 및 수명주기 제어 API 추가 (루트 SDK export 포함)**
   - `(projectRoot, asdSessionId, driverId)` 키 단위로 연결된 `AcpStdioClient` + `acpSessionId`를 상주 유지.
   - 외부(사이드카 서버 종료, 세션 삭제/언링크, 취소 등)에서 호출할 정리 함수 공개:
     ```ts
     export function cancelDriverTurn(sessionId: string): boolean;
     export function closeDriverSession(sessionId: string): Promise<void>;
     export function closeAllDriverSessions(): Promise<void>;
     ```
   - 취소 시 프로세스 트리 강제 종료(`killProcessTree`) 대신 먼저 `client.cancelSession(acpSessionId)`(`session/cancel`)을 전송해 세션 상태를 보존하고, 응답 불능 시에만 프로세스 정리.

2. **`runDriverTurn` 입력 옵션 확장 (`ACP_ROADMAP.md` G1~G3)**
   ```ts
   export interface RunDriverTurnInput {
     // ...기존 필드 유지...
     transport?: "auto" | "acp" | "cli"; // 기본값: commandRunner 주입 시 "cli", 그 외 "auto"
     autoApprove?: boolean;              // 미지정 시 sessions.auto_approve !== 0 참조
     onPermissionRequest?: (
       params: AcpRequestPermissionParams
     ) => Promise<AcpRequestPermissionResult>;
   }
   ```
   - **`transport: "auto"` 동작**:
     1. `input.commandRunner`가 없는 경우 `probeAcpCapability(harnessId, env, { interactivePermissions: !effectiveAutoApprove })` 확인.
     2. `supported === true`이면 상주 `DriverSessionPool`에서 기존 `AcpStdioClient` 재사용(없거나 종료됐으면 신규 `connect()` + `startOrLoadSession({ resumeSessionId: driver.harness_session_id })`).
     3. 이전 턴과 `driver.model`이 달라졌다면 `await client.setSessionModel(...)` 수행 후 `client.promptTurn(...)` 실행.
     4. `AcpNormalizedEvent`(`text`, `thinking`, `tool_use`, `tool_result`, `permission`, `state`)를 `DriverStreamEvent`로 무손실 변환하여 `input.onEvent` 호출 (`ACP_ROADMAP.md` G2).
     5. 획득한 `sessionId`는 기존과 동일하게 `setDriverHarnessSessionId` 및 `meta.json`에 즉시 기록.
     6. `supported === false` (예: `claude-agent-acp` / `codex-acp` 미설치) 또는 초기 ACP 핸드셰이크 실패 시 기존 `executeHarnessProcess` 스트리밍 CLI 경로로 자동 폴백.

---

## 3. 코어 제약 준수 및 배포 동기화 체크리스트

- `dependencies: {}` (외부 패키지 추가 없음), `erasableSyntaxOnly: true`, JSDoc 내 중첩 주석 패턴 금지 준수.
- 추가된 공개 심볼/타입을 `src/index.ts` 및 `EXTERNAL_SDK_REFERENCE.md`에 반영.
- `ASD-Nexus`는 `"agent-swarm-director": "file:../../code/asd-runtime"`을 참조하므로, Core 반영 완료 후 **`asd sync` (또는 `C:\Users\yjsjs0987\Desktop\code\asd-runtime` 동기화)** 까지 수행 필요.
