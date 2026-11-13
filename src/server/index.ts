import type { AddressInfo } from "node:net";
import { createOrchestratorApp } from "./app.ts";
import {
  SIDECAR_READY_PROTOCOL,
  type SidecarReadyInfo,
} from "../shared/protocol.ts";
import { ASD_VERSION } from "./sdk.ts";

export interface ParsedCliArgs {
  port: number;
  host: string;
  projectRoot: string;
  home?: string;
  token?: string;
}

export function parseCliArgs(args: string[]): ParsedCliArgs {
  let port = 0;
  let host = "127.0.0.1";
  let projectRoot = process.cwd();
  let home: string | undefined = undefined;
  let token: string | undefined = undefined;

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (!arg) continue;

    if (arg === "--port" && i + 1 < args.length) {
      port = parseInt(args[++i] ?? "0", 10);
    } else if (arg.startsWith("--port=")) {
      port = parseInt(arg.slice(7), 10);
    } else if (arg === "--host" && i + 1 < args.length) {
      host = args[++i] ?? "127.0.0.1";
    } else if (arg.startsWith("--host=")) {
      host = arg.slice(7);
    } else if (arg === "--project-root" && i + 1 < args.length) {
      projectRoot = args[++i] ?? process.cwd();
    } else if (arg.startsWith("--project-root=")) {
      projectRoot = arg.slice(15);
    } else if (arg === "--home" && i + 1 < args.length) {
      home = args[++i];
    } else if (arg.startsWith("--home=")) {
      home = arg.slice(7);
    } else if (arg === "--token" && i + 1 < args.length) {
      token = args[++i];
    } else if (arg.startsWith("--token=")) {
      token = arg.slice(8);
    }
  }

  return { port, host, projectRoot, home, token };
}

export function startSidecar(cliArgs: string[] = process.argv.slice(2)) {
  const { port, host, projectRoot, home, token } = parseCliArgs(cliArgs);

  const server = createOrchestratorApp(undefined, {
    token,
    defaultProjectRoot: projectRoot,
    homeDir: home,
  });

  server.listen(port, host, () => {
    const address = server.address() as AddressInfo;
    const resolvedPort = address.port;
    const baseUrl = `http://${host}:${resolvedPort}`;

    const readyInfo: SidecarReadyInfo & {
      baseUrl: string;
      projectRoot: string;
      version: string;
      tokenRequired: boolean;
    } = {
      protocol: SIDECAR_READY_PROTOCOL,
      protocolVersion: 1,
      pid: process.pid,
      host,
      port: resolvedPort,
      baseUrl,
      projectRoot,
      version: ASD_VERSION,
      tokenRequired: Boolean(token),
      startedAt: new Date().toISOString(),
    };

    process.stdout.write(JSON.stringify(readyInfo) + "\n");
  });

  let shuttingDown = false;
  const shutdown = () => {
    if (shuttingDown) return;
    shuttingDown = true;
    server.close(() => {
      process.exit(0);
    });
    const forceTimer = setTimeout(() => {
      process.exit(0);
    }, 2000);
    forceTimer.unref();
  };

  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);

  if (!process.stdin.isTTY) {
    process.stdin.on("end", shutdown);
    process.stdin.on("close", shutdown);
    process.stdin.resume();
  }

  return server;
}

// Start sidecar when executed directly
startSidecar();
