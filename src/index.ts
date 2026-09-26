#!/usr/bin/env node
import "dotenv/config";
import { createRuntime } from "./runtime.js";
import { serveHTTP } from "./http.js";

async function main(): Promise<void> {
  const runtime = await createRuntime();
  await serveHTTP(runtime.config, runtime.db, runtime.foundation, runtime.notion, runtime.oauth);
}


main().catch(error => {
  console.error("Foundation startup failed", error);
  process.exit(1);
});
