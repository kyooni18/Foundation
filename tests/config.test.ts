import { afterEach, describe, expect, it } from "vitest";
import { loadConfig } from "../src/config.js";

const original = { ...process.env };
afterEach(() => {
  process.env = { ...original };
});

describe("config", () => {
  it("defaults to Gemini maximum-size embeddings", () => {
    delete process.env.AI_PROVIDER;
    delete process.env.LLM_PROVIDER;
    delete process.env.EMBEDDING_PROVIDER;
    delete process.env.EMBEDDING_MODEL;
    delete process.env.GEMINI_EMBEDDING_MODEL;
    delete process.env.EMBEDDING_DIMENSIONS;
    delete process.env.MCP_PATH;
    delete process.env.FOUNDATION_MCP_FILE_ACCESS;
    delete process.env.FOUNDATION_MCP_ADMIN;
    delete process.env.FOUNDATION_MCP_NOTION;

    const config = loadConfig();
    expect(config.mcpPath).toBe("/mcp");
    expect(config.mcpFileAccess).toBe("off");
    expect(config.mcpAdminEnabled).toBe(false);
    expect(config.mcpNotionEnabled).toBe(false);
    expect(config.embeddingDimensions).toBe(3072);
    expect(config.llmProvider).toBe("none");
    expect(config.embeddingProvider).toBe("gemini");
    expect(config.geminiEmbeddingModel).toBe("gemini-embedding-2");
  });


  it("supports Apple/OpenAI LLM selection independently of OpenAI/Gemini embeddings", () => {
    process.env.LLM_PROVIDER = "apple";
    process.env.EMBEDDING_PROVIDER = "gemini";
    delete process.env.GEMINI_EMBEDDING_MODEL;
    const appleGemini = loadConfig();
    expect(appleGemini.llmProvider).toBe("apple");
    expect(appleGemini.embeddingProvider).toBe("gemini");
    expect(appleGemini.geminiEmbeddingModel).toBe("gemini-embedding-2");

    process.env.LLM_PROVIDER = "openai";
    process.env.EMBEDDING_PROVIDER = "openai";
    const openai = loadConfig();
    expect(openai.llmProvider).toBe("openai");
    expect(openai.embeddingProvider).toBe("openai");
  });

  it("rejects non-maximum embedding dimensions", () => {
    process.env.EMBEDDING_DIMENSIONS = "1536";
    expect(() => loadConfig()).toThrow(/3072/);
  });

  it("configures REST, local OAuth, timezone, and Notion independently", () => {
    process.env.HOST = "127.0.0.1";
    process.env.PORT = "9876";
    process.env.FOUNDATION_API_KEY = "test-static-secret";
    process.env.FOUNDATION_TIME_ZONE = "Asia/Seoul";
    process.env.NOTION_DATA_SOURCE_ID = "test-datasource";
    delete process.env.FOUNDATION_PUBLIC_BASE_URL;
    delete process.env.OAUTH_ENABLED;
    delete process.env.OAUTH_APPROVAL_SECRET;

    const config = loadConfig();
    expect(config.restPath).toBe("/api/v1");
    expect(config.publicBaseUrl).toBe("http://127.0.0.1:9876");
    expect(config.oauthEnabled).toBe(true);
    expect(config.oauthApprovalSecret).toBe("test-static-secret");
    expect(config.timeZone).toBe("Asia/Seoul");
    expect(config.notionDataSourceId).toBe("test-datasource");
  });

  it("supports explicit MCP facade exposure settings", () => {
    process.env.FOUNDATION_MCP_FILE_ACCESS = "read";
    process.env.FOUNDATION_MCP_ADMIN = "true";
    process.env.FOUNDATION_MCP_NOTION = "1";

    const config = loadConfig();
    expect(config.mcpFileAccess).toBe("read");
    expect(config.mcpAdminEnabled).toBe(true);
    expect(config.mcpNotionEnabled).toBe(true);
  });

  it("rejects invalid MCP facade settings", () => {
    process.env.FOUNDATION_MCP_FILE_ACCESS = "everything";
    expect(() => loadConfig()).toThrow(/FOUNDATION_MCP_FILE_ACCESS/);
  });
});
