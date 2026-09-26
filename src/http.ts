import { timingSafeEqual } from "node:crypto";
import express, { type NextFunction, type Request, type Response } from "express";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import type { Config } from "./config.js";
import type { Database } from "./db.js";
import type { Foundation } from "./foundation.js";
import type { NotionSync } from "./notion.js";
import type { OAuthService } from "./oauth.js";
import { createMcpServer, mcpToolCallIsReadOnly } from "./mcp.js";

function sameSecret(a: string, b: string): boolean {
  const left = Buffer.from(a);
  const right = Buffer.from(b);
  return left.length === right.length && timingSafeEqual(left, right);
}

function rpcError(res: Response, status: number, code: number, message: string): void {
  res.status(status).json({ jsonrpc: "2.0", error: { code, message }, id: null });
}

function apiError(res: Response, status: number, error: unknown): void {
  res.status(status).json({ ok: false, error: error instanceof Error ? error.message : String(error) });
}

function requiredScope(req: Request, rpc: boolean): "foundation:read" | "foundation:write" {
  if (!rpc) return req.method === "GET" || req.method === "HEAD" ? "foundation:read" : "foundation:write";
  const body = req.body as any;
  if (body?.method === "tools/call") {
    const name = String(body?.params?.name ?? "");
    const args = body?.params?.arguments;
    return mcpToolCallIsReadOnly(name, args) ? "foundation:read" : "foundation:write";
  }
  return "foundation:read";
}

function authGate(config: Config, oauth: OAuthService | undefined, rpc = false) {
  return (req: Request, res: Response, next: NextFunction): void => {
    void (async () => {
      const hasStatic = Boolean(config.apiKey);
      const hasOAuth = Boolean(oauth?.enabled());
      if (!hasStatic && !hasOAuth) {
        next();
        return;
      }

      const header = req.headers.authorization ?? "";
      const token = header.startsWith("Bearer ") ? header.slice(7) : "";
      if (token && config.apiKey && sameSecret(config.apiKey, token)) {
        next();
        return;
      }

      const scope = requiredScope(req, rpc);
      if (token && oauth?.enabled()) {
        const grant = await oauth.verifyAccessToken(token);
        if (grant) {
          if (grant.scopes.includes(scope)) {
            next();
            return;
          }
          const metadataUrl = new URL("/.well-known/oauth-protected-resource", config.publicBaseUrl).toString();
          res.setHeader(
            "WWW-Authenticate",
            `Bearer error="insufficient_scope", scope="${scope}", resource_metadata="${metadataUrl}"`
          );
          if (rpc) rpcError(res, 403, -32003, "Insufficient OAuth scope");
          else res.status(403).json({ ok: false, error: "Insufficient OAuth scope", requiredScope: scope });
          return;
        }
      }

      const metadataUrl = new URL("/.well-known/oauth-protected-resource", config.publicBaseUrl).toString();
      res.setHeader(
        "WWW-Authenticate",
        oauth?.enabled()
          ? `Bearer resource_metadata="${metadataUrl}", scope="${scope}"`
          : 'Bearer realm="foundation"'
      );
      if (rpc) rpcError(res, 401, -32001, "Unauthorized");
      else res.status(401).json({ ok: false, error: "Unauthorized" });
    })().catch(error => {
      if (!res.headersSent) apiError(res, 500, error);
    });
  };
}

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function asyncRoute(
  handler: (req: Request, res: Response) => Promise<void>
): (req: Request, res: Response) => void {
  return (req, res) => {
    void handler(req, res).catch(error => {
      if (!res.headersSent) apiError(res, 400, error);
    });
  };
}

function integer(value: unknown, fallback: number, min: number, max: number): number {
  const parsed = value === undefined ? fallback : Number(value);
  if (!Number.isInteger(parsed) || parsed < min || parsed > max) {
    throw new Error(`Expected integer between ${min} and ${max}`);
  }
  return parsed;
}

function param(req: Request, name: string): string {
  const value = req.params[name];
  if (typeof value === "string" && value) return value;
  if (Array.isArray(value) && typeof value[0] === "string" && value[0]) return value[0];
  throw new Error(`${name} is required`);
}

export async function serveHTTP(
  config: Config,
  db: Database,
  foundation: Foundation,
  notion?: NotionSync,
  oauth?: OAuthService
): Promise<void> {
  const loopback = ["127.0.0.1", "localhost", "::1"].includes(config.host);
  if (!loopback && !config.apiKey && !oauth?.enabled()) {
    throw new Error("API key or OAuth is required when Foundation binds to a non-loopback host");
  }


  const app = express();
  app.disable("x-powered-by");
  app.use(express.json({ limit: "64mb" }));
  app.use(express.urlencoded({ extended: false, limit: "1mb" }));

  app.get("/live", (_req, res) => res.json({ ok: true }));
  app.get("/health", async (_req, res) => {
    try {
      res.json(await db.health());
    } catch (error) {
      apiError(res, 503, error);
    }
  });


  if (oauth?.enabled()) {
    const metadataHandler = (_req: Request, res: Response) => {
      res.json(oauth.protectedResourceMetadata());
    };
    app.get("/.well-known/oauth-protected-resource", metadataHandler);
    app.get("/.well-known/oauth-protected-resource/mcp", metadataHandler);
    app.get("/.well-known/oauth-authorization-server", (_req, res) => {
      res.json(oauth.authorizationServerMetadata());
    });

    app.post("/oauth/register", asyncRoute(async (req, res) => {
      res.status(201).json(await oauth.register(req.body as Record<string, unknown>));
    }));

    app.get("/oauth/authorize", asyncRoute(async (req, res) => {
      const clientId = stringValue(req.query.client_id);
      const redirectUri = stringValue(req.query.redirect_uri);
      const responseType = stringValue(req.query.response_type);
      const codeChallenge = stringValue(req.query.code_challenge);
      const method = stringValue(req.query.code_challenge_method);
      const scope = stringValue(req.query.scope) || "foundation:read";
      const state = stringValue(req.query.state);
      const resource = stringValue(req.query.resource);

      if (responseType !== "code") throw new Error("response_type must be code");
      if (method !== "S256") throw new Error("code_challenge_method must be S256");
      if (resource && !oauth.acceptsResource(resource)) throw new Error("resource does not match Foundation MCP");

      const client = await oauth.getClient(clientId);
      if (!client) throw new Error("Unknown client_id");
      if (!client.redirectUris.includes(redirectUri)) throw new Error("redirect_uri is not registered");

      const hidden = [
        ["client_id", clientId],
        ["redirect_uri", redirectUri],
        ["response_type", responseType],
        ["code_challenge", codeChallenge],
        ["code_challenge_method", method],
        ["scope", scope],
        ["state", state],
        ["resource", resource]
      ].filter(([, value]) => value)
       .map(([name, value]) => `<input type="hidden" name="${escapeHtml(name!)}" value="${escapeHtml(value!)}">`)
       .join("");

      res.type("html").send(`<!doctype html>
<html>
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Authorize Foundation</title></head>
<body>
  <main style="font-family:system-ui;max-width:34rem;margin:4rem auto;padding:0 1.5rem">
    <h1>Authorize Foundation</h1>
    <p><strong>${escapeHtml(client.clientName)}</strong> is requesting <code>${escapeHtml(scope)}</code>.</p>
    <form method="post" action="/oauth/authorize">
      ${hidden}
      <label>Foundation approval secret<br><input name="approval_secret" type="password" required autocomplete="current-password" style="width:100%;margin:.5rem 0 1rem"></label>
      <button type="submit">Authorize</button>
    </form>
  </main>
</body>
</html>`);
    }));

    app.post("/oauth/authorize", asyncRoute(async (req, res) => {
      const body = req.body as Record<string, unknown>;
      const clientId = stringValue(body.client_id);
      const redirectUri = stringValue(body.redirect_uri);
      const responseType = stringValue(body.response_type);
      const codeChallenge = stringValue(body.code_challenge);
      const method = stringValue(body.code_challenge_method);
      const scope = stringValue(body.scope) || "foundation:read";
      const state = stringValue(body.state);
      const resource = stringValue(body.resource);
      const approvalSecret = stringValue(body.approval_secret);

      if (responseType !== "code") throw new Error("response_type must be code");
      if (resource && !oauth.acceptsResource(resource)) throw new Error("resource does not match Foundation MCP");
      if (!oauth.validateApproval(approvalSecret)) {
        res.status(403).type("html").send("<h1>Authorization denied</h1>");
        return;
      }

      const grant = await oauth.createAuthorizationCode({
        clientId,
        redirectUri,
        codeChallenge,
        codeChallengeMethod: method,
        scope
      });

      const redirect = new URL(redirectUri);
      redirect.searchParams.set("code", grant.code);
      if (state) redirect.searchParams.set("state", state);
      redirect.searchParams.set("iss", oauth.issuer());
      res.redirect(302, redirect.toString());
    }));

    app.post("/oauth/token", asyncRoute(async (req, res) => {
      const body = req.body as Record<string, unknown>;
      const grantType = stringValue(body.grant_type);
      const clientId = stringValue(body.client_id);

      res.setHeader("Cache-Control", "no-store");
      res.setHeader("Pragma", "no-cache");

      if (grantType === "authorization_code") {
        res.json(await oauth.exchangeAuthorizationCode({
          code: stringValue(body.code),
          clientId,
          redirectUri: stringValue(body.redirect_uri),
          codeVerifier: stringValue(body.code_verifier)
        }));
        return;
      }
      if (grantType === "refresh_token") {
        res.json(await oauth.refresh({
          refreshToken: stringValue(body.refresh_token),
          clientId,
          scope: stringValue(body.scope) || undefined
        }));
        return;
      }
      throw new Error("Unsupported grant_type");
    }));
  }

  app.use(config.mcpPath, authGate(config, oauth, true));
  app.use(config.restPath, authGate(config, oauth, false));

  app.post(config.mcpPath, async (req: Request, res: Response) => {
    const server = createMcpServer(foundation, config, notion);
    const transport = new StreamableHTTPServerTransport({ enableJsonResponse: true });
    res.on("close", () => {
      void transport.close();
      void server.close();
    });
    try {
      await server.connect(transport);
      await transport.handleRequest(req, res, req.body);
    } catch (error) {
      if (!res.headersSent) rpcError(res, 500, -32603, error instanceof Error ? error.message : "Internal server error");
    }
  });
  app.get(config.mcpPath, (_req, res) => rpcError(res, 405, -32000, "Use POST for stateless MCP"));
  app.delete(config.mcpPath, (_req, res) => rpcError(res, 405, -32000, "Foundation MCP is stateless"));

  const api = express.Router();

  api.get("/stats", asyncRoute(async (_req, res) => {
    res.json(await foundation.stats());
  }));

  api.get("/search", asyncRoute(async (req, res) => {
    const query = typeof req.query.q === "string" ? req.query.q : "";
    if (!query.trim()) throw new Error("q is required");
    res.json(await foundation.search(query, integer(req.query.limit, 10, 1, 100)));
  }));

  api.post("/notes", asyncRoute(async (req, res) => {
    const body = req.body as Record<string, unknown>;
    if (typeof body.rawText !== "string") throw new Error("rawText is required");
    res.status(201).json(await foundation.ingestNote({
      rawText: body.rawText,
      name: typeof body.name === "string" ? body.name : undefined,
      vaultUid: body.vaultUid === null || typeof body.vaultUid === "string" ? body.vaultUid : undefined,
      metadata: body.metadata && typeof body.metadata === "object" ? body.metadata as Record<string, unknown> : undefined,
      sourceTime: typeof body.sourceTime === "string" ? body.sourceTime : undefined
    }));
  }));

  api.post("/remember", asyncRoute(async (req, res) => {
    const body = req.body as Record<string, unknown>;
    if (typeof body.content !== "string") throw new Error("content is required");
    res.status(201).json(await foundation.remember({
      content: body.content,
      sourceTime: typeof body.sourceTime === "string" ? body.sourceTime : undefined,
      requestId: typeof body.requestId === "string" ? body.requestId : undefined,
      tags: Array.isArray(body.tags) ? body.tags.filter((value): value is string => typeof value === "string") : [],
      metadata: body.metadata && typeof body.metadata === "object"
        ? body.metadata as Record<string, unknown>
        : {}
    }));
  }));

  api.post("/reindex", asyncRoute(async (req, res) => {
    const body = req.body as Record<string, unknown>;
    const kind = body.kind === "atoms" || body.kind === "files" ? body.kind : "all";
    res.json(await foundation.reindex({
      kind,
      limit: integer(body.limit, 1000, 1, 10_000),
      force: body.force === true,
      concurrency: integer(body.concurrency, 4, 1, 16)
    }));
  }));

  api.get("/atoms", asyncRoute(async (req, res) => {
    res.json(await foundation.listAtoms(
      integer(req.query.limit, 50, 1, 500),
      integer(req.query.offset, 0, 0, 10_000_000)
    ));
  }));
  api.post("/atoms", asyncRoute(async (req, res) => {
    const body = req.body as Record<string, unknown>;
    if (typeof body.content !== "string") throw new Error("content is required");
    res.status(201).json(await foundation.createAtom({
      content: body.content,
      type: body.type === "aicreated" || body.type === "imported" ? body.type : "usercreated",
      tags: Array.isArray(body.tags) ? body.tags.filter((v): v is string => typeof v === "string") : [],
      metadata: body.metadata && typeof body.metadata === "object" ? body.metadata as Record<string, unknown> : {}
    }));
  }));
  api.get("/atoms/:id", asyncRoute(async (req, res) => {
    res.json(await foundation.getAtom(param(req, "id")));
  }));
  api.patch("/atoms/:id", asyncRoute(async (req, res) => {
    res.json(await foundation.updateAtom(param(req, "id"), req.body));
  }));
  api.delete("/atoms/:id", asyncRoute(async (req, res) => {
    res.json(await foundation.deleteAtom(param(req, "id")));
  }));
  api.get("/atoms/:id/similar", asyncRoute(async (req, res) => {
    res.json(await foundation.findSimilarAtoms({
      id: param(req, "id"),
      limit: integer(req.query.limit, 10, 1, 100),
      minScore: req.query.minScore === undefined ? 0.85 : Number(req.query.minScore)
    }));
  }));
  api.post("/atoms/:targetId/merge", asyncRoute(async (req, res) => {
    const sourceId = (req.body as Record<string, unknown>).sourceId;
    if (typeof sourceId !== "string") throw new Error("sourceId is required");
    res.json(await foundation.mergeAtoms(sourceId, param(req, "targetId")));
  }));

  api.get("/files", asyncRoute(async (req, res) => {
    res.json(await foundation.listFiles(
      integer(req.query.limit, 50, 1, 500),
      integer(req.query.offset, 0, 0, 10_000_000)
    ));
  }));
  api.post("/files", asyncRoute(async (req, res) => {
    res.status(201).json(await foundation.createFile(req.body));
  }));
  api.get("/files/:id", asyncRoute(async (req, res) => {
    res.json(await foundation.getFile(param(req, "id")));
  }));
  api.patch("/files/:id", asyncRoute(async (req, res) => {
    res.json(await foundation.updateFile(param(req, "id"), req.body));
  api.post("/files/:id/interpret", asyncRoute(async (req, res) => {
    res.json(await foundation.interpretFile(param(req, "id")));
  }));
  }));
  api.delete("/files/:id", asyncRoute(async (req, res) => {
    res.json(await foundation.deleteFile(param(req, "id")));
  }));

  api.get("/vaults", asyncRoute(async (req, res) => {
    res.json(await foundation.listVaults(
      integer(req.query.limit, 50, 1, 500),
      integer(req.query.offset, 0, 0, 10_000_000)
    ));
  }));
  api.post("/vaults", asyncRoute(async (req, res) => {
    res.status(201).json(await foundation.createVault(req.body));
  }));
  api.get("/vaults/:idOrUid", asyncRoute(async (req, res) => {
    res.json(await foundation.getVault(param(req, "idOrUid")));
  }));
  api.delete("/vaults/:idOrUid", asyncRoute(async (req, res) => {
    res.json(await foundation.deleteVault(param(req, "idOrUid"), req.query.deleteFiles === "true"));
  }));


  if (notion) {
    api.get("/notion/status", asyncRoute(async (_req, res) => {
      res.json(await notion.status());
    }));
    api.post("/notion/sync", asyncRoute(async (req, res) => {
      const body = req.body as Record<string, unknown>;
      res.json(await notion.sync({
        dryRun: body.dryRun === true,
        limit: integer(body.limit, 10_000, 1, 10_000)
      }));
    }));
  }

  app.use(config.restPath, api);

  const server = app.listen(config.port, config.host, () => {
    console.log(`Foundation listening on http://${config.host}:${config.port}${config.mcpPath} (REST ${config.restPath})`);
  });

  const shutdown = async () => {
    server.close();
    await db.close().catch(() => undefined);
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}
