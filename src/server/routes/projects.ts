import type { RequestContext } from "./types.ts";
import { sendJson, sendError } from "./types.ts";
import type {
  ListProjectsResponse,
  SelectProjectResponse,
  ResolveProjectPathsResponse,
} from "../../shared/protocol.ts";

export function listProjectsRoute(ctx: RequestContext): void {
  try {
    const skipDiskCheck = ctx.query.get("skipDiskCheck") === "true";
    const forceRefresh = ctx.query.get("forceRefresh") === "true";
    const raw = ctx.deps.listKnownProjects({
      skipDiskCheck,
      forceRefresh,
      homeDir: ctx.homeDir,
    });
    const resp: ListProjectsResponse = {
      projects: (raw || []).map((p) => ({
        projectRoot: p.projectRoot,
        projectHash: p.projectHash,
        sessionCount: p.sessionCount,
        lastActiveAt: (p as any).lastActiveAt ?? (p as any).updatedAt,
      })),
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendError(ctx.res, 500, err instanceof Error ? err.message : String(err));
  }
}

export function selectProjectRoute(ctx: RequestContext): void {
  const projectRoot = ctx.body?.projectRoot || ctx.projectRoot;
  const initialize = ctx.body?.initialize ?? true;
  try {
    if (initialize) {
      ctx.deps.ensureProjectInitialized(projectRoot, ctx.homeDir);
    }
    const paths = ctx.deps.resolveAsdPaths(projectRoot, ctx.homeDir);
    const resp: SelectProjectResponse = {
      success: true,
      projectRoot,
      projectHash: paths.projectHash,
      paths: {
        dbPath: paths.dbPath,
        globalStateDir: paths.globalStateDir,
        localViewDir: paths.localViewDir,
        planMdPath: paths.planMdPath,
        sessionsDir: paths.sessionsDir,
        worktreesDir: paths.worktreesDir,
        globalRoutingPath: paths.globalRoutingPath,
        localRoutingPath: paths.localRoutingPath,
      },
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendJson(ctx.res, 200, {
      success: false,
      projectRoot,
      projectHash: "",
      paths: {},
      error: err instanceof Error ? err.message : String(err),
    } satisfies SelectProjectResponse);
  }
}

export function resolveProjectPathsRoute(ctx: RequestContext): void {
  try {
    const projectRoot = ctx.body?.projectRoot || ctx.query.get("projectRoot") || ctx.projectRoot;
    const paths = ctx.deps.resolveAsdPaths(projectRoot, ctx.homeDir);
    const resp: ResolveProjectPathsResponse = {
      projectRoot,
      paths: {
        dbPath: paths.dbPath,
        globalStateDir: paths.globalStateDir,
        localViewDir: paths.localViewDir,
        planMdPath: paths.planMdPath,
        sessionsDir: paths.sessionsDir,
        worktreesDir: paths.worktreesDir,
        globalRoutingPath: paths.globalRoutingPath,
        localRoutingPath: paths.localRoutingPath,
      },
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendError(ctx.res, 500, err instanceof Error ? err.message : String(err));
  }
}
