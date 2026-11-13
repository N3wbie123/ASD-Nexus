import type { RequestContext } from "./types.ts";
import { sendJson, sendError } from "./types.ts";
import type { GetModelsResponse } from "../../shared/protocol.ts";
import * as path from "node:path";
import * as os from "node:os";

export async function getModelsRoute(
  ctx: RequestContext,
  forceRefreshOverride?: boolean
): Promise<void> {
  try {
    const forceRefresh =
      forceRefreshOverride ?? (ctx.query.get("forceRefresh") === "true");
    const catalog = await ctx.deps.discoverHarnessModels({
      projectRoot: ctx.projectRoot,
      homeDir: ctx.homeDir,
      forceRefresh,
    });
    const home = ctx.homeDir || os.homedir();
    const cachePath = path.join(home, ".asd", "state", "models-cache.json");
    const resp: GetModelsResponse = {
      models: (catalog || {}) as Record<string, string[]>,
      cachePath,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendError(ctx.res, 500, err instanceof Error ? err.message : String(err));
  }
}
