import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import type { RequestContext } from "./types.ts";
import { sendJson, sendError } from "./types.ts";
import type {
  HarnessCommandItem,
  HarnessCommandKind,
  HarnessCommandSource,
  GetHarnessCommandsResponse,
} from "../../shared/protocol.ts";

const SUPPORTED_HARNESSES = [
  "claude-code",
  "opencode",
  "codex",
  "antigravity",
  "cursor",
] as const;

export type SupportedHarness = (typeof SUPPORTED_HARNESSES)[number];

interface BuiltinCommandDef {
  name: string;
  description: string;
  kind?: HarnessCommandKind;
}

const BUILTIN_COMMANDS: Record<SupportedHarness, BuiltinCommandDef[]> = {
  "claude-code": [
    { name: "compact", description: "Compact and summarize conversation context to free up tokens" },
    { name: "clear", description: "Clear conversation history and start fresh" },
    { name: "review", description: "Review recent code changes or pull request" },
    { name: "init", description: "Initialize project configuration (CLAUDE.md)" },
    { name: "doctor", description: "Run diagnostics on Claude Code installation and configuration" },
    { name: "cost", description: "Display token usage and session cost summary" },
    { name: "status", description: "Show current session, model, and workspace status" },
    { name: "memory", description: "View or edit persistent project memory (CLAUDE.md)" },
    { name: "mcp", description: "Inspect and manage connected MCP servers" },
    { name: "pr-comments", description: "Fetch and address pull request review comments" },
  ],
  opencode: [
    { name: "compact", description: "Compact and summarize session history" },
    { name: "init", description: "Initialize project AGENTS.md instructions" },
    { name: "review", description: "Review code changes in workspace" },
    { name: "undo", description: "Undo the last assistant turn and file edits" },
    { name: "redo", description: "Redo a previously undone turn" },
    { name: "share", description: "Create a shareable link for the current session" },
    { name: "export", description: "Export session transcript to markdown or JSON" },
  ],
  codex: [
    { name: "compact", description: "Compact conversation history to save context window" },
    { name: "review", description: "Run a code review over working tree or branch diff" },
    { name: "init", description: "Generate an AGENTS.md scaffold for this repository" },
    { name: "diff", description: "Show git diff of changes made in the session" },
    { name: "status", description: "Display current Codex model, approvals, and session info" },
  ],
  antigravity: [
    { name: "plan", description: "Create a structured step-by-step plan before execution" },
    { name: "compact", description: "Compact or summarize context to free up the context window" },
    { name: "goal", description: "Run a thorough autonomous goal loop until completion" },
    { name: "schedule", description: "Run an instruction on a recurring schedule or timer" },
    { name: "browser", description: "Browse web pages or interact with web applications" },
    { name: "grill-me", description: "Interactive design interview to align on requirements" },
    { name: "teamwork", description: "Spawn a swarm of autonomous agents working together" },
    { name: "learn", description: "Persist learned setup or user corrections for future tasks" },
    { name: "boost", description: "Deep strategic planning, multi-perspective review, and verification" },
  ],
  cursor: [
    { name: "compact", description: "Compact conversation context" },
    { name: "review", description: "Review recent workspace changes" },
    { name: "init", description: "Initialize Cursor rules and project configuration" },
  ],
};

export function normalizeCommandName(raw: string): string {
  return raw.trim().replace(/^\/+/, "").replace(/\.(md|markdown)$/i, "").trim();
}

/**
 * Extracts `name` (optional override) and `description` from markdown content:
 * 1. Checks YAML frontmatter (`--- ... ---`) for `description:` and `name:`.
 * 2. Falls back to the first non-empty, non-heading (or stripped heading) line.
 */
export function parseMarkdownCommandMeta(
  content: string,
  fallbackName: string
): { name: string; description: string; body: string } {
  const trimmed = content.replace(/^\uFEFF/, "");
  let name = normalizeCommandName(fallbackName);
  let description = "";
  let body = trimmed;

  const fmMatch = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/.exec(trimmed);
  if (fmMatch) {
    const fmBlock = fmMatch[1] ?? "";
    body = (fmMatch[2] ?? "").trim();

    const lines = fmBlock.split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i] ?? "";
      const kv = /^([A-Za-z0-9_-]+)\s*:\s*(.*)$/.exec(line);
      if (!kv) continue;
      const key = (kv[1] ?? "").toLowerCase();
      let val = (kv[2] ?? "").trim();

      if ((val === "|" || val === ">" || val === "|-" || val === ">-") && i + 1 < lines.length) {
        const folded: string[] = [];
        while (i + 1 < lines.length && /^[ \t]+/.test(lines[i + 1] ?? "")) {
          i++;
          folded.push((lines[i] ?? "").trim());
        }
        val = folded.join(" ").trim();
      } else {
        val = val.replace(/^["']|["']$/g, "").trim();
      }

      if (key === "name" && val) {
        name = normalizeCommandName(val);
      } else if (key === "description" && val && !description) {
        description = val;
      }
    }
  }

  if (!description) {
    const bodyLines = body.split(/\r?\n/);
    for (const rawLine of bodyLines) {
      const cleaned = rawLine
        .trim()
        .replace(/^#+\s*/, "")
        .replace(/^>\s*/, "")
        .trim();
      if (!cleaned || cleaned.startsWith("```") || cleaned.startsWith("<!--")) {
        continue;
      }
      description = cleaned;
      break;
    }
  }

  if (description.length > 200) {
    description = `${description.slice(0, 197)}...`;
  }

  return {
    name: name || normalizeCommandName(fallbackName),
    description: description || `${name} (${fallbackName})`,
    body: body || trimmed,
  };
}

function scanFlatMarkdownDir(
  dirPath: string,
  harness: SupportedHarness,
  source: HarnessCommandSource,
  kind: HarnessCommandKind,
  prefix = ""
): HarnessCommandItem[] {
  if (!fs.existsSync(dirPath)) return [];
  const results: HarnessCommandItem[] = [];
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(dirPath, { withFileTypes: true });
  } catch {
    return [];
  }

  for (const entry of entries) {
    const fullPath = path.join(dirPath, entry.name);
    if (entry.isDirectory()) {
      // Check if it contains SKILL.md or nested command files
      const skillMd = path.join(fullPath, "SKILL.md");
      if (fs.existsSync(skillMd)) {
        try {
          const content = fs.readFileSync(skillMd, "utf8");
          const meta = parseMarkdownCommandMeta(content, entry.name);
          results.push({
            name: meta.name,
            description: meta.description,
            harness,
            source,
            kind: "skill",
            path: skillMd,
          });
        } catch {
          // ignore unreadable file
        }
      } else {
        const subPrefix = prefix ? `${prefix}:${entry.name}` : entry.name;
        results.push(...scanFlatMarkdownDir(fullPath, harness, source, kind, subPrefix));
      }
    } else if (entry.isFile() && /\.md$/i.test(entry.name)) {
      if (entry.name.toLowerCase() === "readme.md") continue;
      const baseName = entry.name.replace(/\.md$/i, "");
      const fallbackName = prefix ? `${prefix}:${baseName}` : baseName;
      try {
        const content = fs.readFileSync(fullPath, "utf8");
        const meta = parseMarkdownCommandMeta(content, fallbackName);
        results.push({
          name: prefix && !meta.name.includes(":") ? fallbackName : meta.name,
          description: meta.description,
          harness,
          source,
          kind,
          path: fullPath,
        });
      } catch {
        // ignore unreadable file
      }
    }
  }

  return results;
}

function scanSkillsDir(
  dirPath: string,
  harness: SupportedHarness,
  source: HarnessCommandSource
): HarnessCommandItem[] {
  if (!fs.existsSync(dirPath)) return [];
  const results: HarnessCommandItem[] = [];
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(dirPath, { withFileTypes: true });
  } catch {
    return [];
  }

  for (const entry of entries) {
    const fullPath = path.join(dirPath, entry.name);
    if (entry.isDirectory()) {
      const skillMd = path.join(fullPath, "SKILL.md");
      const lowerSkillMd = path.join(fullPath, "skill.md");
      const targetMd = fs.existsSync(skillMd)
        ? skillMd
        : fs.existsSync(lowerSkillMd)
          ? lowerSkillMd
          : null;
      if (targetMd) {
        try {
          const content = fs.readFileSync(targetMd, "utf8");
          const meta = parseMarkdownCommandMeta(content, entry.name);
          results.push({
            name: meta.name,
            description: meta.description,
            harness,
            source,
            kind: "skill",
            path: targetMd,
          });
        } catch {
          // ignore
        }
      }
    } else if (entry.isFile() && /\.md$/i.test(entry.name) && entry.name.toLowerCase() !== "readme.md") {
      const baseName = entry.name.replace(/\.md$/i, "");
      try {
        const content = fs.readFileSync(fullPath, "utf8");
        const meta = parseMarkdownCommandMeta(content, baseName);
        results.push({
          name: meta.name,
          description: meta.description,
          harness,
          source,
          kind: "skill",
          path: fullPath,
        });
      } catch {
        // ignore
      }
    }
  }

  return results;
}

export function discoverCommandsForHarness(
  harness: SupportedHarness,
  options: { projectRoot: string; homeDir?: string }
): HarnessCommandItem[] {
  const home = path.resolve(options.homeDir || os.homedir());
  const project = path.resolve(options.projectRoot || process.cwd());

  const collected: HarnessCommandItem[] = [];

  // 1. Built-in slash commands
  for (const b of BUILTIN_COMMANDS[harness] || []) {
    collected.push({
      name: b.name,
      description: b.description,
      harness,
      source: "builtin",
      kind: b.kind ?? "command",
    });
  }

  // 2. Global + Project directories per harness
  switch (harness) {
    case "claude-code": {
      collected.push(
        ...scanFlatMarkdownDir(path.join(home, ".claude", "commands"), harness, "global", "command"),
        ...scanSkillsDir(path.join(home, ".claude", "skills"), harness, "global"),
        ...scanFlatMarkdownDir(path.join(project, ".claude", "commands"), harness, "project", "command"),
        ...scanSkillsDir(path.join(project, ".claude", "skills"), harness, "project")
      );
      break;
    }
    case "opencode": {
      const globalOc = path.join(home, ".config", "opencode");
      const projectOc = path.join(project, ".opencode");
      collected.push(
        ...scanFlatMarkdownDir(path.join(globalOc, "command"), harness, "global", "command"),
        ...scanFlatMarkdownDir(path.join(globalOc, "commands"), harness, "global", "command"),
        ...scanSkillsDir(path.join(globalOc, "skills"), harness, "global"),
        ...scanFlatMarkdownDir(path.join(projectOc, "command"), harness, "project", "command"),
        ...scanFlatMarkdownDir(path.join(projectOc, "commands"), harness, "project", "command"),
        ...scanSkillsDir(path.join(projectOc, "skills"), harness, "project")
      );
      break;
    }
    case "codex": {
      const globalCx = path.join(home, ".codex");
      collected.push(
        ...scanFlatMarkdownDir(path.join(globalCx, "prompts"), harness, "global", "command"),
        ...scanSkillsDir(path.join(globalCx, "skills"), harness, "global"),
        ...scanSkillsDir(path.join(project, ".codex", "skills"), harness, "project"),
        ...scanSkillsDir(path.join(project, ".agents", "skills"), harness, "project")
      );
      break;
    }
    case "antigravity": {
      const globalGemini = path.join(home, ".gemini");
      collected.push(
        ...scanSkillsDir(path.join(globalGemini, "skills"), harness, "global"),
        ...scanSkillsDir(
          path.join(globalGemini, "antigravity-cli", "builtin", "skills"),
          harness,
          "global"
        ),
        ...scanSkillsDir(path.join(project, ".gemini", "skills"), harness, "project"),
        ...scanFlatMarkdownDir(path.join(project, ".agent", "workflows"), harness, "project", "workflow"),
        ...scanSkillsDir(path.join(project, ".agent", "skills"), harness, "project")
      );
      break;
    }
    case "cursor": {
      const globalCr = path.join(home, ".cursor");
      const projectCr = path.join(project, ".cursor");
      collected.push(
        ...scanFlatMarkdownDir(path.join(globalCr, "commands"), harness, "global", "command"),
        ...scanSkillsDir(path.join(globalCr, "skills"), harness, "global"),
        ...scanFlatMarkdownDir(path.join(projectCr, "commands"), harness, "project", "command"),
        ...scanSkillsDir(path.join(projectCr, "skills"), harness, "project")
      );
      break;
    }
  }

  // Deduplicate by (harness, name): project > global > builtin
  const priorityOrder: Record<HarnessCommandSource, number> = {
    builtin: 1,
    global: 2,
    project: 3,
  };
  const byName = new Map<string, HarnessCommandItem>();
  for (const item of collected) {
    const key = item.name.toLowerCase();
    const existing = byName.get(key);
    if (!existing || priorityOrder[item.source] >= priorityOrder[existing.source]) {
      byName.set(key, item);
    }
  }

  return Array.from(byName.values());
}

export function discoverAllHarnessCommands(options: {
  harness?: string;
  projectRoot: string;
  homeDir?: string;
}): HarnessCommandItem[] {
  const requested = options.harness?.trim().toLowerCase();
  const targets: SupportedHarness[] =
    requested && (SUPPORTED_HARNESSES as readonly string[]).includes(requested)
      ? [requested as SupportedHarness]
      : [...SUPPORTED_HARNESSES];

  const all: HarnessCommandItem[] = [];
  for (const h of targets) {
    all.push(
      ...discoverCommandsForHarness(h, {
        projectRoot: options.projectRoot,
        homeDir: options.homeDir,
      })
    );
  }
  return all;
}

/**
 * Resolves a slash prompt (`/command args...`) for headless execution across the 5 harnesses.
 *
 * - Native built-in CLI commands (like `/compact`, `/review`, `/init`, etc. when not shadowed
 *   by a custom local `.md` skill/command/workflow) are passed through unchanged.
 * - When a prompt starts with `/name` matching a discovered `project` or `global` `.md`
 *   command/skill/workflow file (`item.path`), headless CLIs (`codex exec`, `agy -p`,
 *   `agent -p`, `opencode run`, `claude -p`) often fail to expand custom skills or local
 *   slash templates automatically. We read the markdown instructions and combine them
 *   safely with the user's arguments so all 5 harnesses execute the command reliably.
 */
export function resolveSlashCommandPromptForTurn(options: {
  prompt: string;
  harness: string;
  projectRoot: string;
  homeDir?: string;
}): {
  prompt: string;
  matchedCommand?: HarnessCommandItem;
  expanded: boolean;
} {
  const rawPrompt = options.prompt ?? "";
  const trimmed = rawPrompt.trim();
  if (!trimmed.startsWith("/")) {
    return { prompt: rawPrompt, expanded: false };
  }

  const match = /^\/([^\s]+)(?:\s+([\s\S]*))?$/.exec(trimmed);
  if (!match?.[1]) {
    return { prompt: rawPrompt, expanded: false };
  }

  const rawCmdName = normalizeCommandName(match[1]);
  const userArgs = (match[2] ?? "").trim();
  if (!rawCmdName) {
    return { prompt: rawPrompt, expanded: false };
  }

  const commands = discoverAllHarnessCommands({
    harness: options.harness,
    projectRoot: options.projectRoot,
    homeDir: options.homeDir,
  });

  const cmdLower = rawCmdName.toLowerCase();
  const matched = commands.find((c) => c.name.toLowerCase() === cmdLower);
  if (!matched) {
    return { prompt: rawPrompt, expanded: false };
  }

  // Built-in commands without a backing markdown file are passed directly to the harness CLI
  if (matched.source === "builtin" || !matched.path || !fs.existsSync(matched.path)) {
    return { prompt: rawPrompt, matchedCommand: matched, expanded: false };
  }

  try {
    const rawMd = fs.readFileSync(matched.path, "utf8");
    const parsed = parseMarkdownCommandMeta(rawMd, matched.name);
    let templateBody = parsed.body.trim();

    if (templateBody.includes("$ARGUMENTS")) {
      templateBody = templateBody.replaceAll("$ARGUMENTS", userArgs);
      return {
        prompt: templateBody,
        matchedCommand: matched,
        expanded: true,
      };
    }

    const argsSection = userArgs
      ? `\n\n## User Arguments\n${userArgs}`
      : "";

    const expandedPrompt =
      `Execute the following ${matched.kind} (\`/${matched.name}\` from \`${matched.path}\`):\n\n` +
      `${templateBody}${argsSection}`;

    return {
      prompt: expandedPrompt,
      matchedCommand: matched,
      expanded: true,
    };
  } catch {
    return { prompt: rawPrompt, matchedCommand: matched, expanded: false };
  }
}

export function getHarnessCommandsRoute(ctx: RequestContext): void {
  try {
    const harness = ctx.query.get("harness") || undefined;
    const projectRoot = ctx.query.get("projectRoot") || ctx.projectRoot;
    const commands = discoverAllHarnessCommands({
      harness,
      projectRoot,
      homeDir: ctx.homeDir,
    });
    const resp: GetHarnessCommandsResponse = { commands };
    sendJson(ctx.res, 200, resp);
  } catch (err: unknown) {
    sendError(ctx.res, 500, err instanceof Error ? err.message : String(err));
  }
}
