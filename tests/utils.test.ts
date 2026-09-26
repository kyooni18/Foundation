import { describe, expect, it } from "vitest";
import { contentHash, normalizeTags, normalizeText, vectorLiteral } from "../src/utils.js";

describe("normalization", () => {
  it("normalizes text and tags deterministically", () => {
    expect(normalizeText("  도쿄   파스타  ")).toBe("도쿄 파스타");
    expect(normalizeTags(["Tokyo", "tokyo", " Pasta "])).toEqual(["tokyo", "pasta"]);
    expect(contentHash(" A  B ")).toBe(contentHash("A B"));
  });

  it("enforces 3072-dimensional vectors", () => {
    expect(vectorLiteral(Array(3072).fill(0))).toMatch(/^\[/);
    expect(() => vectorLiteral(Array(1024).fill(0))).toThrow(/3072/);
  });
});
