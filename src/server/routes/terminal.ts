import type { RequestContext } from "./types.ts";
import { sendJson } from "./types.ts";
import type {
  TerminalOpenRequest,
  TerminalOpenResponse,
} from "../../shared/protocol.ts";

export function terminalOpenRoute(ctx: RequestContext): void {
  const body: TerminalOpenRequest = ctx.body || {};
  const projectRoot = body.projectRoot || ctx.projectRoot;
  const harness = body.harness || "claude-code";
  let harnessSessionId = "";
  let model: string | undefined = undefined;

  if (body.sessionId) {
    try {
      ctx.deps.withReadOnlyProjectDb(
        projectRoot,
        (db) => {
          const sess = db.getSession(body.sessionId);
          const driver = db.getActiveDriver(body.sessionId);
          if (driver?.harness_session_id) {
            harnessSessionId = driver.harness_session_id;
          } else if (sess?.harness_session_id) {
            harnessSessionId = sess.harness_session_id;
          }
          if (driver?.model) {
            model = driver.model;
          } else if (sess?.main_model) {
            model = sess.main_model;
          }
        },
        ctx.homeDir
      );
    } catch {
      // Ignore if session not in DB
    }
  }

  try {
    const spec = ctx.deps.buildInteractiveResumeHarnessCommand(
      harness,
      harnessSessionId,
      model
    );
    const resp: TerminalOpenResponse = {
      success: true,
      command: spec.command,
      args: spec.args,
      cliString: spec.cliString,
      cwd: projectRoot,
      launched: false,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      command: "",
      args: [],
      cliString: "",
      cwd: projectRoot,
      launched: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies TerminalOpenResponse);
  }
}
