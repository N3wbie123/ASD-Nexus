/**
 * Orchestrator Server Entrypoint (src/server/index.ts)
 */

import * as path from "node:path";
import { createOrchestratorServer } from "./app.ts";

const host = process.env.ASD_NEXUS_HOST || "127.0.0.1";
const rawPort = Number(process.env.ASD_NEXUS_PORT ?? process.env.PORT ?? "3850");
const port = Number.isFinite(rawPort) && rawPort >= 0 ? rawPort : 3850;
const defaultProjectRoot = path.resolve(
  process.env.ASD_NEXUS_PROJECT_ROOT || process.cwd()
);
const homeDir = process.env.ASD_HOME_DIR || undefined;
const staticWebDir = path.resolve(import.meta.dirname, "../../dist/web");

const server = createOrchestratorServer({
  defaultProjectRoot,
  homeDir,
  staticWebDir,
});

server.listen(port, host, () => {
  const addr = server.address();
  const actualPort = typeof addr === "object" && addr ? addr.port : port;
  console.log(
    `ASD-Nexus Orchestrator listening on http://${host}:${actualPort} (projectRoot: ${defaultProjectRoot})`
  );
});

const shutdown = (): void => {
  server.close(() => {
    process.exit(0);
  });
};

process.once("SIGINT", shutdown);
process.once("SIGTERM", shutdown);
