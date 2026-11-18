/**
 * Pure model-catalog merge helpers (src/server/model-merge.ts).
 *
 * A refresh can legitimately return fewer models than the previous catalog
 * (for example when a harness CLI probe times out). The picker must never lose
 * models it already knows about, so catalogs are unioned with fresh entries
 * first and duplicates removed.
 */

export type ModelCatalogMap = Record<string, string[]>;

export function mergeModelCatalogs(
  previous: ModelCatalogMap | null | undefined,
  fresh: ModelCatalogMap | null | undefined
): ModelCatalogMap {
  const merged: ModelCatalogMap = {};
  const keys = new Set([
    ...Object.keys(fresh || {}),
    ...Object.keys(previous || {}),
  ]);

  for (const key of keys) {
    const seen = new Set<string>();
    const list: string[] = [];
    for (const model of [...(fresh?.[key] || []), ...(previous?.[key] || [])]) {
      if (typeof model !== "string") continue;
      const trimmed = model.trim();
      if (!trimmed || seen.has(trimmed)) continue;
      seen.add(trimmed);
      list.push(trimmed);
    }
    merged[key] = list;
  }

  return merged;
}
