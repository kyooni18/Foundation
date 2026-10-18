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
    expect(ranked[0]?.matchedBy).toEqual(expect.arrayContaining(["text", "symbol", "tag", "metadata"]));
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

// Independent names and near-substrings guard against special-casing Comet.
describe("identity and semantic calibration regressions", () => {
  const candidate = (semantic: number, overrides = {}) => ({ key: "x", exact: 0, lexical: 0, symbol: 0, semantic, tag: 0, metadata: 0, ...overrides });
  it("does not let even high embedding proximity invent an entity match", () => {
    expect(fuseSearchCandidates([candidate(0.95)], {requireAnchor:true})[0]?.eligible).toBe(false);
    expect(fuseSearchCandidates([candidate(0.65,{lexical:1})], {requireAnchor:true,queryTokenCount:1})[0]?.eligible).toBe(true);
  });
  it("separates semantic corroboration from standalone semantic acceptance", () => {
    expect(fuseSearchCandidates([candidate(0.66)])[0]?.eligible).toBe(false);
    expect(fuseSearchCandidates([candidate(0.70)])[0]?.eligible).toBe(true);
    expect(fuseSearchCandidates([candidate(0.64,{lexical:1})])[0]?.matchedBy).toContain("vector");
  });
});

it("does not assign different RRF credit to identical evidence based on UUID order", () => {
  const evidence = {exact:0,lexical:1,symbol:0,semantic:0,tag:0,metadata:0};
  const scores = fuseSearchCandidates([{key:"z",...evidence},{key:"a",...evidence}],{queryTokenCount:1});
  expect(scores[0]?.score).toBe(scores[1]?.score);
});
