import { describe, expect, it } from "vitest";
import { analyzeSearchQuery, fuseSearchCandidates } from "../src/search.js";

describe("search query analysis", () => {
  it("extracts explicit metadata filters without losing the useful query text", () => {
    const query = analyzeSearchQuery('project:Mun memoryType:Decision MotionScheduler lowering');
    expect(query.metadataFilters).toEqual({ project: "Mun", memoryType: "Decision" });
    expect(query.text).toBe("MotionScheduler lowering");
    expect(query.tags).toContain("motionscheduler");
    expect(query.tags).toContain("motion");
    expect(query.tags).toContain("scheduler");
    expect(query.symbolText.toLowerCase()).toContain("motion scheduler");
  });

  it("uses metadata values as search text for metadata-only queries", () => {
    const query = analyzeSearchQuery('repo:"Yeet" kind:architecture');
    expect(query.text).toContain("Yeet");
    expect(query.text).toContain("architecture");
  });
});

describe("reciprocal-rank fusion", () => {
  it("lets corroborating metadata and tags outrank a semantic-only candidate", () => {
    const ranked = fuseSearchCandidates([
      {
        key: "semantic-only",
        exact: 0,
        lexical: 0.05,
        symbol: 0,
        semantic: 0.92,
        tag: 0,
        metadata: 0,
        metadataObject: {},
        changedAt: "2026-09-20T00:00:00Z"
      },
      {
        key: "structured-hit",
        exact: 0,
        lexical: 0.62,
        symbol: 0.7,
        semantic: 0.72,
        tag: 1,
        metadata: 1,
        metadataObject: { importance: "high", confidence: 0.9 },
        changedAt: "2026-09-20T00:00:00Z"
      }
    ], { now: Date.parse("2026-09-29T00:00:00Z") });

    expect(ranked[0]?.key).toBe("structured-hit");
    expect(ranked[0]?.matchedBy).toEqual(expect.arrayContaining(["text", "symbol", "vector", "tag", "metadata"]));
  });

  it("keeps exact matches dominant", () => {
    const ranked = fuseSearchCandidates([
      {
        key: "exact",
        exact: 1,
        lexical: 1,
        symbol: 1,
        semantic: 0.2,
        tag: 0,
        metadata: 0,
        metadataObject: {},
        changedAt: null
      },
      {
        key: "semantic",
        exact: 0,
        lexical: 0.2,
        symbol: 0.2,
        semantic: 0.99,
        tag: 1,
        metadata: 0.2,
        metadataObject: {},
        changedAt: null
      }
    ]);

    expect(ranked[0]?.key).toBe("exact");
  });
});
