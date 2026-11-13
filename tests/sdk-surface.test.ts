import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import {
  ASD_VERSION,
  buildResumeCommand,
  buildSpawnSpec,
  withReadOnlyProjectDb,
} from "../src/server/sdk.ts";
import type { SqliteDriver } from "../src/server/sdk.ts";

function getAllSourceFiles(dir: string): string[] {
  if (!fs.existsSync(dir)) return [];
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const files: string[] = [];
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...getAllSourceFiles(fullPath));
    } else if (/\.ts$/.test(entry.name)) {
      files.push(fullPath);
    }
  }
  return files;
}

const srcDir = path.resolve(import.meta.dirname, "../src");

describe("ASD v2.6.0 SDK Surface & Boundary Contract", () => {
  it("verifies live link to ASD v2.6.0 runtime", () => {
    assert.equal(
      ASD_VERSION,
      "2.6.0",
      "ASD_VERSION must be 2.6.0 matching the live asd-runtime dependency"
    );
  });

  describe("SDK_SURFACE_CLEANUP_PLAN §3 cleanliness audits", () => {
    const srcFiles = getAllSourceFiles(srcDir);

    it("scans at least one source file in src/", () => {
      assert.ok(
        srcFiles.length > 0,
        "Source directory src/ must contain TypeScript files"
      );
    });

    it("verifies zero references to the dashboard server entry", () => {
      for (const file of srcFiles) {
        const content = fs.readFileSync(file, "utf8");
        assert.equal(
          /\bstartDashboardServer\b/.test(content),
          false,
          `${file} must not reference or import the ASD ui server entry`
        );
      }
    });

    it("verifies zero imports from the removed claude adapter subpath", () => {
      for (const file of srcFiles) {
        const content = fs.readFileSync(file, "utf8");
        const match = /agent-swarm-director\/adapters\/claude(\.ts)?["']/.test(
          content
        );
        assert.equal(
          match,
          false,
          `${file} must not import the removed claude adapter subpath (use claude-code)`
        );
      }
    });

    it("verifies zero references to removed v2.6.0 symbols", () => {
      const removedSymbols = [
        "installGlobalConfigs",
        "syncRepoVersionStrings",
        "registerCoreHarnessProvisioner",
        "getCoreHarnessProvisioner",
        "approveTaskWorktree",
        "rejectTaskWorktree",
        "isProtectedPath",
        "buildHeadlessCommand",
        "buildInteractiveResumeCommand",
        "verifyPidIdentity",
      ];

      for (const file of srcFiles) {
        const content = fs.readFileSync(file, "utf8");
        for (const symbol of removedSymbols) {
          const regex = new RegExp(`\\b${symbol}\\b`);
          assert.equal(
            regex.test(content),
            false,
            `${file} must not import or call removed symbol ${symbol}`
          );
        }
      }
    });

    it("verifies zero positional calls to buildResumeHarnessCommand", () => {
      for (const file of srcFiles) {
        const content = fs.readFileSync(file, "utf8");
        const positionalCall = /\bbuildResumeHarnessCommand\s*\(\s*["']/.test(
          content
        );
        assert.equal(
          positionalCall,
          false,
          `${file} must call buildResumeHarnessCommand with an options object only`
        );
      }
    });

    it("verifies zero child_process imports in src/", () => {
      for (const file of srcFiles) {
        const content = fs.readFileSync(file, "utf8");
        const imported =
          /from\s+["'](node:)?child_process["']/.test(content) ||
          /require\(["'](node:)?child_process["']\)/.test(content);
        assert.equal(
          imported,
          false,
          `${file} must not import child_process; process work goes through the SDK`
        );
      }
    });

    it("verifies zero raw child_process harness spawns", () => {
      for (const file of srcFiles) {
        const content = fs.readFileSync(file, "utf8");
        const rawHarnessSpawn =
          /(spawn|exec|execSync|spawnSync)\s*\(\s*["'](claude|codex|opencode|agy|cursor-agent)\b/.test(
            content
          );
        assert.equal(
          rawHarnessSpawn,
          false,
          `${file} must not directly invoke raw harness CLI processes`
        );
      }
    });
  });

  describe("Harness command construction specifications", () => {
    it("buildSpawnSpec generates streaming flags and permission skip for claude-code", () => {
      const spec = buildSpawnSpec({
        harness: "claude-code",
        prompt: "Check clean-room architecture",
        streaming: true,
      });

      assert.equal(spec.command, "claude");
      assert.ok(
        spec.args.includes("--output-format"),
        "Args must include --output-format"
      );
      const formatIdx = spec.args.indexOf("--output-format");
      assert.equal(
        spec.args[formatIdx + 1],
        "stream-json",
        "--output-format must be stream-json"
      );
      assert.ok(spec.args.includes("--verbose"), "Args must include --verbose");
      assert.ok(
        spec.args.includes("--dangerously-skip-permissions"),
        "Args must include --dangerously-skip-permissions"
      );
    });

    it("buildResumeCommand strictly accepts an options object and formats claude-code stream flags", () => {
      const resume = buildResumeCommand({
        harness: "claude-code",
        harnessSessionId: "test-sess-001",
        prompt: "Next step",
        streaming: true,
      });

      assert.equal(resume.command, "claude");
      assert.ok(resume.args.includes("--resume"), "Args must include --resume");
      const resumeIdx = resume.args.indexOf("--resume");
      assert.equal(
        resume.args[resumeIdx + 1],
        "test-sess-001",
        "--resume must target harnessSessionId"
      );
      assert.ok(
        resume.args.includes("--output-format"),
        "Args must include --output-format"
      );
      const formatIdx = resume.args.indexOf("--output-format");
      assert.equal(
        resume.args[formatIdx + 1],
        "stream-json",
        "--output-format must be stream-json"
      );
      assert.ok(resume.args.includes("--verbose"), "Args must include --verbose");
    });

    it("withReadOnlyProjectDb opens the project database, executes the callback, and closes the connection", () => {
      let closed = false;
      const fakeProjectRoot = process.cwd();

      const result = withReadOnlyProjectDb(fakeProjectRoot, (db: SqliteDriver) => {
        const originalClose = db.close.bind(db);
        db.close = () => {
          closed = true;
          originalClose();
        };
        return db.listSessions();
      });

      assert.ok(Array.isArray(result), "Callback should return session rows array");
      assert.equal(closed, true, "Database connection must be closed in finally block");
    });
  });
});
