import { describe, expect, it } from "vitest";
import { mcpModelToolNames, mcpToolCallIsReadOnly } from "../src/mcp.js";

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
