import type { RequestContext } from "./types.ts";
import { sendJson, sendError } from "./types.ts";
import type {
  GetRoutingResponse,
  SaveRoutingResponse,
} from "../../shared/protocol.ts";

export function getRoutingRoute(ctx: RequestContext): void {
  try {
    const projectRoot = ctx.query.get("projectRoot") || ctx.projectRoot;
    const routing = ctx.deps.loadRoutingConfig(projectRoot, ctx.homeDir);
    const resp: GetRoutingResponse = {
      projectRoot,
      routing: (routing || {}) as any,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendError(ctx.res, 500, err instanceof Error ? err.message : String(err));
  }
}

export function saveRoutingRoute(ctx: RequestContext): void {
  try {
    const projectRoot = ctx.body?.projectRoot || ctx.projectRoot;
    const action = ctx.body?.action === "reset" ? "reset" : "save";
    const routingConfig = ctx.body?.routingConfig;
    ctx.deps.saveProjectRoutingConfig({
      projectRoot,
      action,
      routingConfig,
      homeDir: ctx.homeDir,
    });
    const resp: SaveRoutingResponse = { success: true };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    } satisfies SaveRoutingResponse);
  }
}
