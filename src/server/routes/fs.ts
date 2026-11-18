import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import type { RequestContext } from "./types.ts";
import { sendJson, sendError } from "./types.ts";
import type {
  FsListEntry,
  FsListMode,
  FsListResponse,
} from "../../shared/protocol.ts";

const IGNORED_DIRS = new Set([
  ".git",
  "node_modules",
  "target",
  ".asd",
  ".next",
  "dist",
  "build",
  "coverage",
  "__pycache__",
  ".venv",
  "venv",
  ".turbo",
  ".cache",
]);

function normalizeSlash(p: string): string {
  return p.replaceAll("\\", "/");
}

function listWorkspaceFiles(
  projectRoot: string,
  prefix: string,
  limit: number
): { entries: FsListEntry[]; truncated: boolean } {
  const rootDir = path.resolve(projectRoot);
  if (!fs.existsSync(rootDir)) {
    return { entries: [], truncated: false };
  }

  const normalizedPrefix = normalizeSlash(prefix.trim()).replace(/^\.?\//, "").toLowerCase();
  const entries: FsListEntry[] = [];
  let truncated = false;
  const maxScanFiles = Math.max(limit * 20, 5000);
  let scannedCount = 0;

  function walk(currentDir: string, relDir: string): void {
    if (entries.length >= limit || scannedCount >= maxScanFiles) {
      truncated = true;
      return;
    }

    let dirents: fs.Dirent[];
    try {
      dirents = fs.readdirSync(currentDir, { withFileTypes: true });
    } catch {
      return;
    }

    // Sort directories and files alphabetically for deterministic autocomplete
    dirents.sort((a, b) => a.name.localeCompare(b.name));

    for (const d of dirents) {
      if (entries.length >= limit || scannedCount >= maxScanFiles) {
        truncated = true;
        return;
      }

      const relPath = relDir ? `${relDir}/${d.name}` : d.name;

      if (d.isDirectory()) {
        if (IGNORED_DIRS.has(d.name)) {
          continue;
        }
        walk(path.join(currentDir, d.name), relPath);
      } else if (d.isFile()) {
        scannedCount++;
        if (
          !normalizedPrefix ||
          relPath.toLowerCase().includes(normalizedPrefix) ||
          d.name.toLowerCase().includes(normalizedPrefix)
        ) {
          entries.push({
            path: relPath,
            name: d.name,
            isDir: false,
          });
        }
      }
    }
  }

  walk(rootDir, "");
  return { entries, truncated };
}

function listDirectoriesForCd(
  projectRoot: string,
  rawPrefix: string,
  limit: number,
  homeDir?: string
): { baseDir: string; entries: FsListEntry[]; truncated: boolean } {
  const rootDir = path.resolve(projectRoot);
  const home = path.resolve(homeDir || os.homedir());
  const trimmed = rawPrefix.trim();

  let expandedPrefix = trimmed;
  if (expandedPrefix === "~") {
    expandedPrefix = home + path.sep;
  } else if (expandedPrefix.startsWith("~/") || expandedPrefix.startsWith("~\\")) {
    expandedPrefix = path.join(home, expandedPrefix.slice(2));
  }

  let searchDir = rootDir;
  let nameFilter = "";

  if (!expandedPrefix) {
    searchDir = rootDir;
    nameFilter = "";
  } else {
    const endsWithSep = expandedPrefix.endsWith("/") || expandedPrefix.endsWith("\\");
    const candidatePath = path.isAbsolute(expandedPrefix)
      ? path.resolve(expandedPrefix)
      : path.resolve(rootDir, expandedPrefix);

    if (
      endsWithSep ||
      expandedPrefix === "." ||
      expandedPrefix === ".." ||
      expandedPrefix.endsWith("/.") ||
      expandedPrefix.endsWith("/..") ||
      expandedPrefix.endsWith("\\.") ||
      expandedPrefix.endsWith("\\..")
    ) {
      if (fs.existsSync(candidatePath) && isDirectorySafe(candidatePath)) {
        searchDir = candidatePath;
        nameFilter = "";
      } else {
        searchDir = path.dirname(candidatePath);
        nameFilter = path.basename(candidatePath).toLowerCase();
      }
    } else if (fs.existsSync(candidatePath) && isDirectorySafe(candidatePath)) {
      // Exact directory exists: still check if parent has sibling matches or list inside candidatePath
      // If user typed a partial folder name that happens to exist, listing parent with filter + candidate itself is helpful,
      // or if parent exists we filter in parent unless candidatePath is a root drive.
      const parentDir = path.dirname(candidatePath);
      if (parentDir !== candidatePath && fs.existsSync(parentDir) && isDirectorySafe(parentDir)) {
        searchDir = parentDir;
        nameFilter = path.basename(candidatePath).toLowerCase();
      } else {
        searchDir = candidatePath;
        nameFilter = "";
      }
    } else {
      const parentDir = path.dirname(candidatePath);
      if (fs.existsSync(parentDir) && isDirectorySafe(parentDir)) {
        searchDir = parentDir;
        nameFilter = path.basename(candidatePath).toLowerCase();
      } else {
        return { baseDir: normalizeSlash(rootDir), entries: [], truncated: false };
      }
    }
  }

  let dirents: fs.Dirent[];
  try {
    dirents = fs.readdirSync(searchDir, { withFileTypes: true });
  } catch {
    return { baseDir: normalizeSlash(searchDir), entries: [], truncated: false };
  }

  dirents.sort((a, b) => a.name.localeCompare(b.name));

  const entries: FsListEntry[] = [];
  let truncated = false;

  // Include ".." entry when navigating at top of searchDir with empty or "." filter
  const parentOfSearch = path.dirname(searchDir);
  if (
    parentOfSearch !== searchDir &&
    (!nameFilter || "..".startsWith(nameFilter))
  ) {
    entries.push({
      path: normalizeSlash(parentOfSearch),
      name: "..",
      isDir: true,
    });
  }

  for (const d of dirents) {
    if (!d.isDirectory()) continue;
    if (d.name === ".git" || d.name === "node_modules") continue;
    if (nameFilter && !d.name.toLowerCase().startsWith(nameFilter) && !d.name.toLowerCase().includes(nameFilter)) {
      continue;
    }

    if (entries.length >= limit) {
      truncated = true;
      break;
    }

    const fullChildPath = path.join(searchDir, d.name);
    entries.push({
      path: normalizeSlash(fullChildPath),
      name: d.name,
      isDir: true,
    });
  }

  return {
    baseDir: normalizeSlash(searchDir),
    entries,
    truncated,
  };
}

function isDirectorySafe(p: string): boolean {
  try {
    return fs.statSync(p).isDirectory();
  } catch {
    return false;
  }
}

export function fsListRoute(ctx: RequestContext): void {
  try {
    const projectRoot = path.resolve(ctx.query.get("projectRoot") || ctx.projectRoot);
    const rawMode = ctx.query.get("mode") || "files";
    const mode: FsListMode = rawMode === "dirs" ? "dirs" : "files";
    const prefix = ctx.query.get("prefix") || "";
    const rawLimit = Number.parseInt(ctx.query.get("limit") || "150", 10);
    const limit = Number.isFinite(rawLimit) && rawLimit > 0 ? Math.min(rawLimit, 1000) : 150;

    if (mode === "dirs") {
      const { baseDir, entries, truncated } = listDirectoriesForCd(
        projectRoot,
        prefix,
        limit,
        ctx.homeDir
      );
      const resp: FsListResponse = {
        projectRoot: normalizeSlash(projectRoot),
        mode: "dirs",
        baseDir,
        entries,
        truncated,
      };
      sendJson(ctx.res, 200, resp);
      return;
    }

    const { entries, truncated } = listWorkspaceFiles(projectRoot, prefix, limit);
    const resp: FsListResponse = {
      projectRoot: normalizeSlash(projectRoot),
      mode: "files",
      baseDir: normalizeSlash(projectRoot),
      entries,
      truncated,
    };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendError(ctx.res, 500, err instanceof Error ? err.message : String(err));
  }
}
