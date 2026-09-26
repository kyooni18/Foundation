import type { AIProvider } from "./ai.js";
import { createAIProvider } from "./ai.js";
import type { Config } from "./config.js";
import { loadConfig } from "./config.js";
import { Database } from "./db.js";
import { Foundation } from "./foundation.js";
import { NotionSync } from "./notion.js";
import { OAuthService } from "./oauth.js";
import { OpenAIFileInterpreter } from "./interpreter.js";

export interface FoundationRuntime {
  config: Config;
  db: Database;
  ai: AIProvider;
  foundation: Foundation;
  notion: NotionSync;
  oauth: OAuthService;
  interpreter: OpenAIFileInterpreter;
  close(): Promise<void>;
}

export async function createRuntime(): Promise<FoundationRuntime> {
  const config = loadConfig();
  const db = new Database(config);
  await db.initialize();
  const ai = createAIProvider(config);
  const interpreter = new OpenAIFileInterpreter(config);
  const foundation = new Foundation(db, ai, config.timeZone, interpreter);
  const notion = new NotionSync(config, db, foundation);
  const oauth = new OAuthService(config, db);

  return {
    config,
    db,
    ai,
    foundation,
    notion,
    oauth,
    interpreter,
    close: () => db.close()
  };
}
