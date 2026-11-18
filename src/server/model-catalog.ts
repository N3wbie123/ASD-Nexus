/**
 * Cached model discovery (src/server/model-catalog.ts).
 *
 * The SDK owns harness probing; this boundary adds two protections required by
 * the desktop UI:
 *   1. a wider CLI timeout, so a cold `opencode models` run does not get killed
 *      just before it prints its catalog;
 *   2. a union with the last disk-cached catalog, so a partial refresh never
 *      removes models that were already visible.
 *
 * Only `createDefaultDeps` wires this in. Tests that inject a mock
 * `discoverHarnessModels` keep their exact behavior.
 */

import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { discoverHarnessModels } from "./sdk.ts";
import type { HarnessModelCatalog } from "./sdk.ts";
import { mergeModelCatalogs, type ModelCatalogMap } from "./model-merge.ts";

const CLI_PROBE_TIMEOUT_MS = 15_000;

export function getModelCachePath(homeDir?: string): string {
  return path.join(
    path.resolve(homeDir || os.homedir()),
    ".asd",
    "state",
    "models-cache.json"
  );
}

function readCachedCatalog(cachePath: string): ModelCatalogMap | null {
  try {
    const raw = JSON.parse(fs.readFileSync(cachePath, "utf8")) as {
      catalog?: ModelCatalogMap;
    };
    if (!raw?.catalog || typeof raw.catalog !== "object") return null;
    return raw.catalog;
  } catch {
    return null;
  }
}

function writeCachedCatalog(cachePath: string, catalog: ModelCatalogMap): void {
  try {
    fs.mkdirSync(path.dirname(cachePath), { recursive: true });
    const tmp = `${cachePath}.${process.pid}.tmp`;
    fs.writeFileSync(
      tmp,
      JSON.stringify({ updatedAt: Date.now(), catalog }, null, 2) + "\n",
      "utf8"
    );
    fs.renameSync(tmp, cachePath);
  } catch {
    // Best-effort cache write; the API response still carries the merged catalog.
  }
}

function toHarnessCatalog(map: ModelCatalogMap): HarnessModelCatalog {
  return {
    antigravity: map.antigravity ?? [],
    "claude-code": map["claude-code"] ?? [],
    codex: map.codex ?? [],
    opencode: map.opencode ?? [],
    cursor: map.cursor ?? [],
  };
}

function catalogsEqual(a: ModelCatalogMap, b: ModelCatalogMap): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export async function discoverHarnessModelsWithCache(options?: {
  projectRoot?: string;
  homeDir?: string;
  forceRefresh?: boolean;
}): Promise<HarnessModelCatalog> {
  const cachePath = getModelCachePath(options?.homeDir);
  const previous = readCachedCatalog(cachePath);

  let fresh: ModelCatalogMap;
  try {
    fresh = await discoverHarnessModels({
      projectRoot: options?.projectRoot,
      homeDir: options?.homeDir,
      forceRefresh: options?.forceRefresh,
      cliTimeoutMs: CLI_PROBE_TIMEOUT_MS,
    });
  } catch (err) {
    if (previous) {
      console.error(
        `[models] discovery failed; serving cached catalog: ${
          err instanceof Error ? err.message : String(err)
        }`
      );
      return toHarnessCatalog(previous);
    }
    throw err;
  }

  const merged = mergeModelCatalogs(previous, fresh);
  if (!catalogsEqual(merged, fresh)) {
    writeCachedCatalog(cachePath, merged);
  }
  return toHarnessCatalog(merged);
}
