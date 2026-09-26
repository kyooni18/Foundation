import { describe, expect, it } from "vitest";
import { resourceMatches } from "../src/oauth.js";

describe("Foundation OAuth resource matching", () => {
  const expected = "http://127.0.0.1:8787/mcp";

  it("accepts equivalent loopback MCP resource spellings", () => {
    expect(resourceMatches(expected, expected)).toBe(true);
    expect(resourceMatches(expected, "http://127.0.0.1:8787/mcp/")).toBe(true);
    expect(resourceMatches(expected, "http://localhost:8787/mcp")).toBe(true);
    expect(resourceMatches(expected, "http://localhost:8787/mcp/")).toBe(true);
    expect(resourceMatches(expected, "http://[::1]:8787/mcp")).toBe(true);
  });

  it("still rejects different resource boundaries", () => {
    expect(resourceMatches(expected, "https://localhost:8787/mcp")).toBe(false);
    expect(resourceMatches(expected, "http://localhost:8788/mcp")).toBe(false);
    expect(resourceMatches(expected, "http://localhost:8787/api/v1")).toBe(false);
    expect(resourceMatches(expected, "http://example.com:8787/mcp")).toBe(false);
    expect(resourceMatches(expected, "http://localhost:8787/mcp?x=1")).toBe(false);
    expect(resourceMatches(expected, "not a URL")).toBe(false);
  });
});
