#!/usr/bin/env node
import "dotenv/config";
import { createRuntime } from "./runtime.js";

function option(name: string): string | undefined {
  const prefix = `--${name}=`;
  return process.argv.slice(2).find(value => value.startsWith(prefix))?.slice(prefix.length);
}

function flag(name: string): boolean {
  return process.argv.slice(2).includes(`--${name}`);
}

function numberOption(name: string, fallback: number): number {
  const raw = option(name);
  if (!raw) return fallback;
  const value = Number(raw);
  if (!Number.isInteger(value)) throw new Error(`--${name} must be an integer`);
  return value;
}

function print(value: unknown): void {
  process.stdout.write(JSON.stringify(value, null, 2) + "\n");
}

function usage(): never {
  process.stderr.write(`Foundation CLI

Usage:
  foundation stats
  foundation search <query> [--limit=10]
  foundation ingest <raw text> [--name=Note] [--vault=<uid>] [--source-time=<ISO-8601>]
  foundation remember <text> [--source-time=<ISO-8601>] [--request-id=<id>]
  foundation reindex [--kind=all|atoms|files] [--limit=1000] [--concurrency=4] [--force]
  foundation list atoms|files|vaults [--limit=50] [--offset=0]
  foundation get atom|file|vault <id-or-uid>
  foundation delete atom|file|vault <id-or-uid> [--delete-files]
  foundation similar <atom-id> [--limit=10] [--min-score=0.85]
  foundation merge <source-atom-id> <target-atom-id>
  foundation interpret <file-id>
  foundation deactivate <atom-id>
  foundation notion-status
  foundation notion-sync [--dry-run] [--limit=10000]
`);
  process.exit(2);
}

async function main(): Promise<void> {
  const [command, ...args] = process.argv.slice(2).filter(value => !value.startsWith("--"));
  if (!command || command === "help" || command === "--help") usage();

  const runtime = await createRuntime();
  try {
    const f = runtime.foundation;
    switch (command) {
      case "stats":
        print(await f.stats());
        return;
      case "search":
        if (!args.length) usage();
        print(await f.search(args.join(" "), numberOption("limit", 10)));
        return;
      case "ingest":
        if (!args.length) usage();
        print(await f.ingestNote({
          rawText: args.join(" "),
          name: option("name"),
          vaultUid: option("vault"),
          sourceTime: option("source-time")
        }));
        return;
      case "remember":
        if (!args.length) usage();
        print(await f.remember({
          content: args.join(" "),
          sourceTime: option("source-time"),
          requestId: option("request-id")
        }));
        return;
      case "reindex": {
        const kind = option("kind") ?? "all";
        if (!["all", "atoms", "files"].includes(kind)) throw new Error("--kind must be all, atoms, or files");
        print(await f.reindex({
          kind: kind as "all" | "atoms" | "files",
          limit: numberOption("limit", 1000),
          concurrency: numberOption("concurrency", 4),
          force: flag("force")
        }));
        return;
      }
      case "list": {
        const kind = args[0];
        const limit = numberOption("limit", 50);
        const offset = numberOption("offset", 0);
        if (kind === "atoms") print(await f.listAtoms(limit, offset));
        else if (kind === "files") print(await f.listFiles(limit, offset));
        else if (kind === "vaults") print(await f.listVaults(limit, offset));
        else usage();
        return;
      }
      case "get": {
        const [kind, id] = args;
        if (!id) usage();
        if (kind === "atom") print(await f.getAtom(id));
        else if (kind === "file") print(await f.getFile(id));
        else if (kind === "vault") print(await f.getVault(id));
        else usage();
        return;
      }
      case "delete": {
        const [kind, id] = args;
        if (!id) usage();
        if (kind === "atom") print(await f.deleteAtom(id));
        else if (kind === "file") print(await f.deleteFile(id));
        else if (kind === "vault") print(await f.deleteVault(id, flag("delete-files")));
        else usage();
        return;
      }
      case "similar": {
        const [id] = args;
        if (!id) usage();
        print(await f.findSimilarAtoms({
          id,
          limit: numberOption("limit", 10),
          minScore: Number(option("min-score") ?? "0.85")
        }));
        return;
      }
      case "deactivate": {
        const [id] = args;
        if (!id) usage();
        print(await f.updateAtom(id, { active: false }));
        return;
      }
      case "interpret": {
        const [id] = args;
        if (!id) usage();
        print(await f.interpretFile(id));
        return;
      }
      case "notion-status":
        print(await runtime.notion.status());
        return;
      case "notion-sync":
        print(await runtime.notion.sync({
          dryRun: flag("dry-run"),
          limit: numberOption("limit", 10_000)
        }));
        return;

      case "merge": {
        const [sourceId, targetId] = args;
        if (!sourceId || !targetId) usage();
        print(await f.mergeAtoms(sourceId, targetId));
        return;
      }
      default:
        usage();
    }
  } finally {
    await runtime.close();
  }
}

main().catch(error => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
