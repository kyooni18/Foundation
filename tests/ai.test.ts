import { afterEach, describe, expect, it, vi } from "vitest";
import { GeminiEmbeddingProvider, OpenAILLMProvider } from "../src/ai.js";
import { loadConfig } from "../src/config.js";

const originalFetch = globalThis.fetch;
const originalEnv = { ...process.env };

afterEach(() => {
  globalThis.fetch = originalFetch;
  process.env = { ...originalEnv };
  vi.restoreAllMocks();
});

describe("GeminiEmbeddingProvider", () => {
  it("requests maximum 3072D embeddings and applies retrieval prefixes", async () => {
    process.env.EMBEDDING_PROVIDER = "gemini";
    process.env.GEMINI_API_KEY = "test-key";
    delete process.env.GEMINI_EMBEDDING_MODEL;
    delete process.env.EMBEDDING_DIMENSIONS;

    const requests: Array<{ url: string; body: Record<string, unknown> }> = [];
    globalThis.fetch = vi.fn(async (input, init) => {
      requests.push({
        url: String(input),
        body: JSON.parse(String(init?.body)) as Record<string, unknown>
      });
      return new Response(JSON.stringify({
        embeddings: [{ values: Array(3072).fill(0.125) }]
      }), {
        status: 200,
        headers: { "content-type": "application/json" }
      });
    }) as typeof fetch;

    const provider = new GeminiEmbeddingProvider(loadConfig());
    const query = await provider.embed("파이어볼 파스타", "query");
    const document = await provider.embed("도쿄 파이어볼 파스타집 맛있는듯", "document");

    expect(query.vector).toHaveLength(3072);
    expect(query.provider).toBe("gemini");
    expect(query.model).toBe("gemini-embedding-2");
    expect(query.dimensions).toBe(3072);

    expect(document.vector).toHaveLength(3072);
    expect(document.provider).toBe("gemini");
    expect(document.model).toBe("gemini-embedding-2");
    expect(document.dimensions).toBe(3072);

    expect(requests).toHaveLength(2);
    expect(requests[0]!.url).toContain("/models/gemini-embedding-2:embedContent");
    expect(requests[0]!.body.output_dimensionality).toBe(3072);
    expect(requests[1]!.body.output_dimensionality).toBe(3072);

    const queryContent = requests[0]!.body.content as { parts: Array<{ text: string }> };
    const documentContent = requests[1]!.body.content as { parts: Array<{ text: string }> };
    expect(queryContent.parts[0]!.text).toBe("task: search result | query: 파이어볼 파스타");
    expect(documentContent.parts[0]!.text).toBe("title: none | text: 도쿄 파이어볼 파스타집 맛있는듯");
  });

  it("requires a Gemini API key", async () => {
    process.env.EMBEDDING_PROVIDER = "gemini";
    delete process.env.GEMINI_API_KEY;

    const provider = new GeminiEmbeddingProvider(loadConfig());
    await expect(provider.embed("test", "query")).rejects.toThrow(/GEMINI_API_KEY/);
  });

  it("grounds temporal extraction to the supplied source time and timezone", async () => {
    process.env.LLM_PROVIDER = "openai";
    process.env.OPENAI_API_KEY = "test-key";
    process.env.OPENAI_LLM_MODEL = "test-model";

    let requestBody: any;
    globalThis.fetch = vi.fn(async (_input, init) => {
      requestBody = JSON.parse(String(init?.body));
      return new Response(JSON.stringify({
        choices: [{
          message: {
            content: JSON.stringify({
              subject: "도쿄 방문",
              tags: ["도쿄"],
              atoms: [{
                content: "사용자는 2026-09-23에 도쿄에 있었다",
                tags: ["도쿄"],
                validFrom: "2026-09-23T09:00:00+09:00",
                expires: null,
                subject: "도쿄 방문",
                confidence: 0.9,
                memoryType: "Fact",
                scope: "Global"
              }]
            })
          }
        }]
      }), { status: 200, headers: { "content-type": "application/json" } });
    }) as typeof fetch;

    const provider = new OpenAILLMProvider(loadConfig());
    const result = await provider.extractNote("오늘 도쿄에 있었음", {
      sourceTime: "2026-09-23T12:34:00+09:00",
      timeZone: "Asia/Seoul"
    });

    expect(requestBody.messages[0].content).toContain("2026-09-23T12:34:00+09:00");
    expect(requestBody.messages[0].content).toContain("Asia/Seoul");
    expect(result.atoms[0]!.validFrom).toBe("2026-09-23T00:00:00.000Z");
    expect(result.atoms[0]!.memoryType).toBe("Fact");
    expect(result.atoms[0]!.confidence).toBe(0.9);
  });

  it("resolves durable memory against supplied candidate IDs", async () => {
    process.env.LLM_PROVIDER = "openai";
    process.env.OPENAI_API_KEY = "test-key";
    process.env.OPENAI_LLM_MODEL = "test-model";

    globalThis.fetch = vi.fn(async () => new Response(JSON.stringify({
      choices: [{
        message: {
          content: JSON.stringify({
            action: "supersede",
            targetAtomId: "11111111-1111-1111-1111-111111111111",
            canonicalContent: "사용자의 작업 경로는 /new/path 이다",
            tags: ["경로", "설정"],
            confidence: 0.94,
            rationale: "newer mutable setting"
          })
        }
      }]
    }), { status: 200, headers: { "content-type": "application/json" } })) as typeof fetch;

    const provider = new OpenAILLMProvider(loadConfig());
    const result = await provider.resolveMemory({
      content: "이제 작업 경로는 /new/path",
      tags: ["경로"],
      metadata: { memoryType: "Fact" },
      sourceTime: "2026-09-23T16:00:00+09:00",
      timeZone: "Asia/Seoul",
      candidates: [{
        id: "11111111-1111-1111-1111-111111111111",
        content: "사용자의 작업 경로는 /old/path 이다",
        tags: ["경로"],
        active: true,
        similarity: 0.91,
        createdAt: "2026-09-20T00:00:00.000Z",
        updatedAt: "2026-09-20T00:00:00.000Z",
        metadata: { memoryType: "Fact" }
      }]
    });

    expect(result.action).toBe("supersede");
    expect(result.targetAtomId).toBe("11111111-1111-1111-1111-111111111111");
    expect(result.canonicalContent).toContain("/new/path");
    expect(result.confidence).toBe(0.94);
  });
});
