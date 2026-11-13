import type { RequestContext } from "./types.ts";
import { sendJson } from "./types.ts";
import type { GetHealthResponse } from "../../shared/protocol.ts";

export function getHealthRoute(ctx: RequestContext): void {
  const resp: GetHealthResponse = {
    ok: true,
    version: ctx.deps.version,
    defaultProjectRoot: ctx.options.defaultProjectRoot || process.cwd(),
    pid: process.pid,
  };
  sendJson(ctx.res, 200, resp);
}
