import { spawn } from "node:child_process";
import { mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import type { Config } from "./config.js";
import { normalizeText } from "./utils.js";

export interface InterpretFileInput {
  name: string;
  mimeType: string | null;
  base64: string;
}

export interface InterpretFileResult {
  text: string;
  provider: "openai";
  model: string;
  method: "image" | "file" | "audio" | "video" | "text";
  metadata: Record<string, unknown>;
}

export interface FileInterpreter {
  available(): boolean;
  interpret(input: InterpretFileInput): Promise<InterpretFileResult>;
}

function outputText(response: any): string {
  if (typeof response?.output_text === "string" && response.output_text.trim()) {
    return response.output_text.trim();
  }
  const chunks: string[] = [];
  for (const item of Array.isArray(response?.output) ? response.output : []) {
    for (const content of Array.isArray(item?.content) ? item.content : []) {
      if (typeof content?.text === "string") chunks.push(content.text);
    }
  }
  return chunks.join("\n").trim();
}

function extensionForMime(mimeType: string | null): string {
  const map: Record<string, string> = {
    "video/mp4": ".mp4",
    "video/quicktime": ".mov",
    "video/webm": ".webm",
    "video/x-matroska": ".mkv",
    "audio/mpeg": ".mp3",
    "audio/mp4": ".m4a",
    "audio/wav": ".wav",
    "audio/x-wav": ".wav",
    "audio/webm": ".webm",
    "audio/ogg": ".ogg"
  };
  return mimeType ? map[mimeType] ?? "" : "";
}

function run(command: string, args: string[], timeoutMs: number): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ["ignore", "ignore", "pipe"] });
    let stderr = "";
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", chunk => { stderr += chunk; });
    child.on("error", reject);
    const timer = setTimeout(() => child.kill("SIGKILL"), timeoutMs);
    child.on("close", code => {
      clearTimeout(timer);
      if (code === 0) resolve();
      else reject(new Error(`${command} failed (${code ?? "signal"}): ${stderr.slice(-1000)}`));
    });
  });
}

export class OpenAIFileInterpreter implements FileInterpreter {
  constructor(private readonly config: Config) {}

  available(): boolean {
    return Boolean(this.config.openAIAPIKey);
  }

  private async json(pathname: string, body: Record<string, unknown>): Promise<any> {
    if (!this.config.openAIAPIKey) throw new Error("OPENAI_API_KEY is required for File interpretation");
    const response = await fetch(`${this.config.openAIBaseURL}${pathname}`, {
      method: "POST",
      headers: {
        authorization: `Bearer ${this.config.openAIAPIKey}`,
        "content-type": "application/json"
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(Math.max(this.config.aiTimeoutMs, 120_000))
    });
    if (!response.ok) {
      throw new Error(`OpenAI interpretation failed (${response.status}): ${(await response.text()).slice(0, 1000)}`);
    }
    return response.json();
  }

  private async responses(content: Array<Record<string, unknown>>): Promise<string> {
    const response = await this.json("/responses", {
      model: this.config.openAIInterpretationModel,
      input: [{
        role: "user",
        content: [
          {
            type: "input_text",
            text: "Create a dense, searchable textual interpretation of this file. Preserve visible/transcribed text, named entities, dates, measurements, relationships, actions, and uncertainty. Do not invent details. Write plain text suitable for semantic search and later fact extraction."
          },
          ...content
        ]
      }]
    });
    const text = outputText(response);
    if (!text) throw new Error("OpenAI returned no interpreted text");
    return text;
  }

  private async transcribe(bytes: Buffer, name: string, mimeType: string | null): Promise<string> {
    if (!this.config.openAIAPIKey) throw new Error("OPENAI_API_KEY is required for audio transcription");
    const form = new FormData();
    form.set("model", this.config.openAITranscriptionModel);
    const uploadBytes = Uint8Array.from(bytes).buffer as ArrayBuffer;
    form.set("file", new File([uploadBytes], name || "audio", { type: mimeType || "application/octet-stream" }));
    const response = await fetch(`${this.config.openAIBaseURL}/audio/transcriptions`, {
      method: "POST",
      headers: { authorization: `Bearer ${this.config.openAIAPIKey}` },
      body: form,
      signal: AbortSignal.timeout(Math.max(this.config.aiTimeoutMs, 120_000))
    });
    if (!response.ok) {
      throw new Error(`OpenAI transcription failed (${response.status}): ${(await response.text()).slice(0, 1000)}`);
    }
    const payload = await response.json() as any;
    const text = typeof payload?.text === "string" ? payload.text.trim() : "";
    if (!text) throw new Error("OpenAI transcription returned no text");
    return text;
  }

  private async interpretVideo(bytes: Buffer, input: InterpretFileInput): Promise<InterpretFileResult> {
    const dir = await mkdtemp(path.join(tmpdir(), "foundation-video-"));
    try {
      const source = path.join(dir, `source${path.extname(input.name) || extensionForMime(input.mimeType) || ".bin"}`);
      const framesDir = path.join(dir, "frames");
      const audio = path.join(dir, "audio.m4a");
      await writeFile(source, bytes);
      await run("mkdir", ["-p", framesDir], 10_000);

      try {
        await run("ffmpeg", [
          "-hide_banner", "-loglevel", "error", "-i", source,
          "-vf", "fps=1/15,scale='min(1280,iw)':-2",
          "-frames:v", "12",
          path.join(framesDir, "%03d.jpg")
        ], 120_000);
      } catch {
        await run("ffmpeg", [
          "-hide_banner", "-loglevel", "error", "-i", source,
          "-vf", "thumbnail,scale='min(1280,iw)':-2",
          "-frames:v", "1",
          path.join(framesDir, "%03d.jpg")
        ], 120_000);
      }

      let transcript = "";
      try {
        await run("ffmpeg", [
          "-hide_banner", "-loglevel", "error", "-i", source,
          "-vn", "-ac", "1", "-ar", "16000", "-c:a", "aac", "-b:a", "64k", audio
        ], 120_000);
        const audioBytes = await readFile(audio);
        if (audioBytes.length > 0) transcript = await this.transcribe(audioBytes, "video-audio.m4a", "audio/mp4");
      } catch {
        transcript = "";
      }

      const frameNames = (await readdir(framesDir)).filter(name => name.endsWith(".jpg")).sort().slice(0, 12);
      const images: Array<Record<string, unknown>> = [];
      for (const frame of frameNames) {
        const frameBytes = await readFile(path.join(framesDir, frame));
        images.push({
          type: "input_image",
          image_url: `data:image/jpeg;base64,${frameBytes.toString("base64")}`
        });
      }
      if (transcript) {
        images.unshift({
          type: "input_text",
          text: `Audio transcript:\n${transcript}`
        });
      }
      if (!images.length) throw new Error("Video yielded no interpretable frames or audio");

      const text = await this.responses(images);
      return {
        text,
        provider: "openai",
        model: this.config.openAIInterpretationModel,
        method: "video",
        metadata: { frames: frameNames.length, transcribedAudio: Boolean(transcript) }
      };
    } finally {
      await rm(dir, { recursive: true, force: true });
    }
  }

  async interpret(input: InterpretFileInput): Promise<InterpretFileResult> {
    if (!this.available()) throw new Error("File interpretation requires OPENAI_API_KEY");
    const bytes = Buffer.from(input.base64, "base64");
    if (!bytes.length) throw new Error("File has no binary data");
    if (bytes.length > 50 * 1024 * 1024) throw new Error("File interpretation is limited to 50 MiB per File");

    const mime = (input.mimeType || "application/octet-stream").toLowerCase();

    if (mime.startsWith("text/") || mime === "application/json" || mime === "application/xml") {
      const decoded = normalizeText(bytes.toString("utf8"));
      if (!decoded) throw new Error("Text File decoded to empty content");
      return {
        text: decoded,
        provider: "openai",
        model: "local-utf8",
        method: "text",
        metadata: { bytes: bytes.length }
      };
    }

    if (mime.startsWith("image/")) {
      const text = await this.responses([{
        type: "input_image",
        image_url: `data:${mime};base64,${input.base64}`
      }]);
      return {
        text,
        provider: "openai",
        model: this.config.openAIInterpretationModel,
        method: "image",
        metadata: { bytes: bytes.length }
      };
    }

    if (mime.startsWith("audio/")) {
      const transcript = await this.transcribe(bytes, input.name, mime);
      return {
        text: transcript,
        provider: "openai",
        model: this.config.openAITranscriptionModel,
        method: "audio",
        metadata: { bytes: bytes.length }
      };
    }

    if (mime.startsWith("video/")) {
      return this.interpretVideo(bytes, input);
    }

    const text = await this.responses([{
      type: "input_file",
      filename: input.name,
      file_data: `data:${mime};base64,${input.base64}`
    }]);
    return {
      text,
      provider: "openai",
      model: this.config.openAIInterpretationModel,
      method: "file",
      metadata: { bytes: bytes.length, mimeType: mime }
    };
  }
}
