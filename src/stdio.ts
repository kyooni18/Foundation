#!/usr/bin/env node
import "dotenv/config";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { createMcpServer } from "./mcp.js";
import { createRuntime } from "./runtime.js";

async function main(): Promise<void> {
  const runtime = await createRuntime();
  const server = createMcpServer(runtime.foundation, runtime.config, runtime.notion);
  const transport = new StdioServerTransport();

  const shutdown = async () => {
    await server.close().catch(() => undefined);
    await runtime.close().catch(() => undefined);
  };

  process.once("SIGINT", () => void shutdown().finally(() => process.exit(0)));
  process.once("SIGTERM", () => void shutdown().finally(() => process.exit(0)));

  await server.connect(transport);
}

main().catch(error => {
  console.error("Foundation stdio startup failed", error);
  process.exit(1);
});
