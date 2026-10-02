import { describe, expect, it } from "vitest";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { createMcpServer, mcpModelToolNames, mcpToolCallIsReadOnly } from "../src/mcp.js";

describe("Foundation MCP model surface", () => {
  it("defaults to the three high-level facades", () => {
    expect(mcpModelToolNames({
      mcpFileAccess: "off",
      mcpAdminEnabled: false,
      mcpNotionEnabled: false
    })).toEqual(["recall", "remember", "forget"]);
  });

  it("only exposes optional facades when enabled", () => {
    const config = {
      mcpFileAccess: "write" as const,
      mcpAdminEnabled: true,
      mcpNotionEnabled: true
    };
    expect(mcpModelToolNames(config, true)).toEqual([
      "recall", "remember", "forget", "file", "admin", "notion"
    ]);
    expect(mcpModelToolNames(config, false)).toEqual([
      "recall", "remember", "forget", "file", "admin"
    ]);
  });

  it("keeps recall output model-facing and minimal", async () => {
    const id = "bce4ed8c-dc35-4b3f-9fba-9248ca0d1cf0";
    const foundation = {
      search: async () => [{
        kind: "atom",
        id,
        atomId: id,
        score: 0.85,
        rawText: "Foundation MCP is available.",
        matchedBy: ["text", "vector"]
      }]
    } as unknown as Parameters<typeof createMcpServer>[0];
    const server = createMcpServer(foundation, {
      mcpFileAccess: "off",
      mcpAdminEnabled: false,
      mcpNotionEnabled: false
    });
    const client = new Client({ name: "foundation-test", version: "0.0.0" });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();

    await server.connect(serverTransport);
    await client.connect(clientTransport);
    const result = await client.callTool({ name: "recall", arguments: { query: "Foundation MCP", limit: 1 } });

    expect(result.content).toEqual([{
      type: "text",
      text: JSON.stringify([{ id, text: "Foundation MCP is available." }])
    }]);
    await client.close();
    await server.close();
  });

  it("keeps remember output model-facing and minimal", async () => {
    const id = "a2f99dd8-5a4c-4b62-b8b7-f27b29d16618";
    const foundation = {
      remember: async () => ({
        requestId: "req-1",
        sourceTime: "2026-09-27T13:00:00+09:00",
        decomposed: true,
        results: [{
          action: "new",
          targetAtomId: null,
          atom: { id, content: "Foundation remember output is compact." },
          confidence: 1,
          rationale: "No related candidate",
          embeddingDeferred: false,
          candidates: []
        }, {
          action: "duplicate",
          targetAtomId: id,
          atom: { id, content: "Duplicate decomposition result." },
          confidence: 1,
          rationale: "Exact active content match",
          embeddingDeferred: false,
          candidates: [{ id, similarity: 1 }]
        }]
      })
    } as unknown as Parameters<typeof createMcpServer>[0];
    const server = createMcpServer(foundation, {
      mcpFileAccess: "off",
      mcpAdminEnabled: false,
      mcpNotionEnabled: false
    });
    const client = new Client({ name: "foundation-test", version: "0.0.0" });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();

    await server.connect(serverTransport);
    await client.connect(clientTransport);
    const result = await client.callTool({ name: "remember", arguments: { content: "Foundation remember output is compact." } });

    expect(result.content).toEqual([{
      type: "text",
      text: JSON.stringify({ ids: [id] })
    }]);
    await client.close();
    await server.close();
  });

  it("classifies facade actions for OAuth read/write scopes", () => {
    expect(mcpToolCallIsReadOnly("recall", {})).toBe(true);
    expect(mcpToolCallIsReadOnly("remember", {})).toBe(false);
    expect(mcpToolCallIsReadOnly("forget", {})).toBe(false);
    expect(mcpToolCallIsReadOnly("file", { action: "list" })).toBe(true);
    expect(mcpToolCallIsReadOnly("file", { action: "delete" })).toBe(false);
    expect(mcpToolCallIsReadOnly("admin", { action: "stats" })).toBe(true);
    expect(mcpToolCallIsReadOnly("admin", { action: "reindex" })).toBe(false);
    expect(mcpToolCallIsReadOnly("notion", { action: "status" })).toBe(true);
    expect(mcpToolCallIsReadOnly("notion", { action: "sync" })).toBe(false);
  });
});
