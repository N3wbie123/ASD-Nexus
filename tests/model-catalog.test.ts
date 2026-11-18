import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { mergeModelCatalogs } from "../src/server/model-merge.ts";

describe("mergeModelCatalogs", () => {
  it("keeps fresh models first and preserves models missing from a partial refresh", () => {
    const merged = mergeModelCatalogs(
      { opencode: ["a", "b"], "claude-code": ["sonnet"] },
      { opencode: ["b", "c"], "claude-code": [] }
    );

    assert.deepEqual(merged.opencode, ["b", "c", "a"]);
    assert.deepEqual(merged["claude-code"], ["sonnet"]);
  });

  it("trims and de-duplicates model ids", () => {
    const merged = mergeModelCatalogs(
      { opencode: [" x ", "x", "y"] },
      { opencode: ["x", " y ", "z"] }
    );

    assert.deepEqual(merged.opencode, ["x", "y", "z"]);
  });

  it("handles empty catalogs", () => {
    assert.deepEqual(mergeModelCatalogs(null, null), {});
    assert.deepEqual(mergeModelCatalogs({ opencode: [] }, undefined), { opencode: [] });
  });
});
