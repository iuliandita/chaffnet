#!/usr/bin/env node

import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { ChaffnetClient } from "chaffnet";
import { loadConfig } from "./config.js";
import { createChaffnetMcpServer } from "./server.js";

async function main(): Promise<void> {
  const client = new ChaffnetClient(loadConfig());
  const server = createChaffnetMcpServer(client);
  const transport = new StdioServerTransport();
  let closing = false;

  const close = () => {
    if (closing) return;
    closing = true;
    void server.close().catch(() => {
      process.exitCode = 1;
    });
  };

  process.once("SIGINT", close);
  process.once("SIGTERM", close);
  await server.connect(transport);
}

try {
  await main();
} catch (error) {
  const message = error instanceof Error ? error.message : "unknown startup error";
  process.stderr.write(`chaffnet-mcp: ${message}\n`);
  process.exitCode = 1;
}
