import { afterEach, describe, expect, it, vi } from "vitest";
import { loadConfig } from "../src/config.js";
import { OpenAIFileInterpreter } from "../src/interpreter.js";

const originalFetch = globalThis.fetch;
const originalEnv = { ...process.env };

afterEach(() => {
  globalThis.fetch = originalFetch;
  process.env = { ...originalEnv };
  vi.restoreAllMocks();
});

describe("OpenAIFileInterpreter", () => {
  it("decodes textual binary Files locally and returns searchable text", async () => {
    process.env.OPENAI_API_KEY = "test-key";
    const interpreter = new OpenAIFileInterpreter(loadConfig());
    const result = await interpreter.interpret({
      name: "note.txt",
      mimeType: "text/plain",
      base64: Buffer.from("  Foundation   text file  ").toString("base64")
    });

    expect(result.method).toBe("text");
    expect(result.text).toBe("Foundation text file");
    expect(result.model).toBe("local-utf8");
  });

  it("uses Responses vision for images", async () => {
    process.env.OPENAI_API_KEY = "test-key";
    process.env.OPENAI_INTERPRETATION_MODEL = "vision-test";

    let requestBody: any;
    globalThis.fetch = vi.fn(async (_input, init) => {
      requestBody = JSON.parse(String(init?.body));
      return new Response(JSON.stringify({ output_text: "A diagram labeled Foundation." }), {
        status: 200,
        headers: { "content-type": "application/json" }
      });
    }) as typeof fetch;

    const interpreter = new OpenAIFileInterpreter(loadConfig());
    const result = await interpreter.interpret({
      name: "diagram.png",
      mimeType: "image/png",
      base64: Buffer.from([0x89, 0x50, 0x4e, 0x47]).toString("base64")
    });

    expect(result.method).toBe("image");
    expect(result.model).toBe("vision-test");
    expect(result.text).toContain("Foundation");
    expect(requestBody.input[0].content.some((part: any) => part.type === "input_image")).toBe(true);
  });
});
