import type * as http from "node:http";
import type { OrchestratorDeps, AppOptions } from "../app.ts";

export interface RequestContext {
  req: http.IncomingMessage;
  res: http.ServerResponse;
  url: URL;
  pathname: string;
  query: URLSearchParams;
  body: any;
  deps: OrchestratorDeps;
  options: AppOptions;
  projectRoot: string;
  homeDir?: string;
}

export function sendJson(res: http.ServerResponse, status: number, data: unknown): void {
  if (res.writableEnded) return;
  const json = JSON.stringify(data);
  res.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Content-Length": Buffer.byteLength(json),
  });
  res.end(json);
}

export function sendError(res: http.ServerResponse, status: number, message: string): void {
  sendJson(res, status, { success: false, error: message });
}
