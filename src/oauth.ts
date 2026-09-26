import { createHash, randomBytes, timingSafeEqual } from "node:crypto";
import type { Config } from "./config.js";
import type { Database } from "./db.js";

export const FOUNDATION_SCOPES = ["foundation:read", "foundation:write", "offline_access"] as const;
export type FoundationScope = (typeof FOUNDATION_SCOPES)[number];

function sha256(value: string): string {
  return createHash("sha256").update(value).digest("hex");
}

function base64urlSha256(value: string): string {
  return createHash("sha256").update(value).digest("base64url");
}

function randomToken(bytes = 32): string {
  return randomBytes(bytes).toString("base64url");
}

function constantEqual(a: string, b: string): boolean {
  const left = Buffer.from(a);
  const right = Buffer.from(b);
  return left.length === right.length && timingSafeEqual(left, right);
}

function loopbackHostname(hostname: string): boolean {
  const normalized = hostname.replace(/^\[|\]$/g, "").toLowerCase();
  return normalized === "localhost" || normalized === "127.0.0.1" || normalized === "::1";
}

function normalizedResourcePath(pathname: string): string {
  if (pathname === "/") return pathname;
  return pathname.replace(/\/+$/, "");
}

export function resourceMatches(expected: string, candidate: string): boolean {
  try {
    const left = new URL(expected);
    const right = new URL(candidate);
    if (left.username || left.password || right.username || right.password) return false;
    if (left.search || left.hash || right.search || right.hash) return false;
    if (left.protocol !== right.protocol || left.port !== right.port) return false;
    if (normalizedResourcePath(left.pathname) !== normalizedResourcePath(right.pathname)) return false;

    if (left.hostname === right.hostname) return true;
    return loopbackHostname(left.hostname) && loopbackHostname(right.hostname);
  } catch {
    return false;
  }
}

function normalizeScopes(value: string | undefined): FoundationScope[] {
  const requested = (value ?? "foundation:read")
    .split(/\s+/)
    .map(v => v.trim())
    .filter(Boolean);
  const unique = [...new Set(requested)];
  for (const scope of unique) {
    if (!FOUNDATION_SCOPES.includes(scope as FoundationScope)) {
      throw new Error(`Unsupported scope: ${scope}`);
    }
  }
  return unique as FoundationScope[];
}

export interface OAuthClient {
  clientId: string;
  clientName: string;
  redirectUris: string[];
}

export interface AccessGrant {
  clientId: string;
  scopes: string[];
  expiresAt: Date;
}

export class OAuthService {
  constructor(private readonly config: Config, private readonly db: Database) {}

  enabled(): boolean {
    return this.config.oauthEnabled && Boolean(this.config.oauthApprovalSecret);
  }

  issuer(): string {
    return this.config.publicBaseUrl;
  }

  resource(): string {
    return new URL(this.config.mcpPath, this.config.publicBaseUrl + "/").toString();
  }

  acceptsResource(candidate: string): boolean {
    return resourceMatches(this.resource(), candidate);
  }

  protectedResourceMetadata(): Record<string, unknown> {
    return {
      resource: this.resource(),
      authorization_servers: [this.issuer()],
      scopes_supported: ["foundation:read", "foundation:write"],
      bearer_methods_supported: ["header"],
      resource_name: "Foundation MCP"
    };
  }

  authorizationServerMetadata(): Record<string, unknown> {
    const issuer = this.issuer();
    return {
      issuer,
      authorization_endpoint: new URL("/oauth/authorize", issuer).toString(),
      token_endpoint: new URL("/oauth/token", issuer).toString(),
      registration_endpoint: new URL("/oauth/register", issuer).toString(),
      response_types_supported: ["code"],
      grant_types_supported: ["authorization_code", "refresh_token"],
      code_challenge_methods_supported: ["S256"],
      token_endpoint_auth_methods_supported: ["none"],
      scopes_supported: [...FOUNDATION_SCOPES],
      authorization_response_iss_parameter_supported: true
    };
  }

  async register(input: Record<string, unknown>): Promise<Record<string, unknown>> {
    if (!this.enabled()) throw new Error("OAuth is disabled");
    const redirectUris = Array.isArray(input.redirect_uris)
      ? input.redirect_uris.filter((value): value is string => typeof value === "string")
      : [];
    if (!redirectUris.length) throw new Error("redirect_uris is required");
    for (const uri of redirectUris) {
      const parsed = new URL(uri);
      if (!["http:", "https:"].includes(parsed.protocol) && !parsed.protocol.endsWith(":")) {
        throw new Error("Invalid redirect URI");
      }
      if (parsed.protocol === "http:" && !["127.0.0.1", "localhost", "::1"].includes(parsed.hostname)) {
        throw new Error("Plain HTTP redirect URIs must be loopback");
      }
    }

    const clientId = randomToken(24);
    const clientName = typeof input.client_name === "string" && input.client_name.trim()
      ? input.client_name.trim().slice(0, 200)
      : "Foundation OAuth client";
    await this.db.query(
      "INSERT INTO oauth_clients(client_id,client_name,redirect_uris) VALUES($1,$2,$3::jsonb)",
      [clientId, clientName, JSON.stringify(redirectUris)]
    );

    return {
      client_id: clientId,
      client_name: clientName,
      redirect_uris: redirectUris,
      application_type: typeof input.application_type === "string" ? input.application_type : "native",
      grant_types: ["authorization_code", "refresh_token"],
      response_types: ["code"],
      token_endpoint_auth_method: "none",
      scope: "foundation:read foundation:write offline_access"
    };
  }

  async getClient(clientId: string): Promise<OAuthClient | null> {
    const result = await this.db.query(
      "SELECT client_id,client_name,redirect_uris FROM oauth_clients WHERE client_id=$1",
      [clientId]
    );
    if (!result.rows[0]) return null;
    const uris = Array.isArray(result.rows[0].redirect_uris)
      ? result.rows[0].redirect_uris.map(String)
      : [];
    return {
      clientId: String(result.rows[0].client_id),
      clientName: String(result.rows[0].client_name),
      redirectUris: uris
    };
  }

  validateApproval(secret: string): boolean {
    const expected = this.config.oauthApprovalSecret;
    return Boolean(expected && constantEqual(secret, expected));
  }

  async createAuthorizationCode(input: {
    clientId: string;
    redirectUri: string;
    codeChallenge: string;
    codeChallengeMethod: string;
    scope?: string;
  }): Promise<{ code: string; scopes: FoundationScope[] }> {
    if (input.codeChallengeMethod !== "S256") throw new Error("Only PKCE S256 is supported");
    if (!/^[A-Za-z0-9_-]{43,128}$/.test(input.codeChallenge)) throw new Error("Invalid code_challenge");
    const client = await this.getClient(input.clientId);
    if (!client) throw new Error("Unknown client_id");
    if (!client.redirectUris.includes(input.redirectUri)) throw new Error("redirect_uri is not registered");
    const scopes = normalizeScopes(input.scope);

    const code = randomToken(32);
    await this.db.query(
      `INSERT INTO oauth_authorization_codes(
         code_hash,client_id,redirect_uri,code_challenge,scopes,expires_at
       ) VALUES($1,$2,$3,$4,$5::text[],NOW() + INTERVAL '5 minutes')`,
      [sha256(code), input.clientId, input.redirectUri, input.codeChallenge, scopes]
    );
    return { code, scopes };
  }

  private async issueTokens(clientId: string, scopes: string[], familyId?: string): Promise<Record<string, unknown>> {
    const hasRefresh = scopes.includes("offline_access");
    const family = hasRefresh ? (familyId ?? randomBytes(16).toString("hex")) : undefined;

    const accessToken = randomToken(32);
    const accessExpires = new Date(Date.now() + this.config.oauthAccessTtlSeconds * 1000);
    await this.db.query(
      `INSERT INTO oauth_tokens(token_hash,client_id,token_kind,scopes,expires_at,family_id)
       VALUES($1,$2,'access',$3::text[],$4,$5::uuid)`,
      [sha256(accessToken), clientId, scopes, accessExpires, family ?? null]
    );

    let refreshToken: string | undefined;
    if (hasRefresh) {
      refreshToken = randomToken(40);
      const refreshExpires = new Date(Date.now() + this.config.oauthRefreshTtlSeconds * 1000);
      await this.db.query(
        `INSERT INTO oauth_tokens(token_hash,client_id,token_kind,scopes,expires_at,family_id)
         VALUES($1,$2,'refresh',$3::text[],$4,$5::uuid)`,
        [sha256(refreshToken), clientId, scopes, refreshExpires, family]
      );
    }

    return {
      access_token: accessToken,
      token_type: "Bearer",
      expires_in: this.config.oauthAccessTtlSeconds,
      scope: scopes.join(" "),
      ...(refreshToken ? { refresh_token: refreshToken } : {})
    };
  }

  async exchangeAuthorizationCode(input: {
    code: string;
    clientId: string;
    redirectUri: string;
    codeVerifier: string;
  }): Promise<Record<string, unknown>> {
    const hash = sha256(input.code);
    return this.db.transaction(async client => {
      const result = await client.query(
        `SELECT * FROM oauth_authorization_codes
         WHERE code_hash=$1 FOR UPDATE`,
        [hash]
      );
      const row = result.rows[0];
      if (!row) throw new Error("Invalid authorization code");
      if (row.consumed_at) throw new Error("Authorization code already consumed");
      if (new Date(row.expires_at).getTime() <= Date.now()) throw new Error("Authorization code expired");
      if (String(row.client_id) !== input.clientId || String(row.redirect_uri) !== input.redirectUri) {
        throw new Error("Authorization code binding mismatch");
      }
      if (base64urlSha256(input.codeVerifier) !== String(row.code_challenge)) {
        throw new Error("Invalid PKCE code_verifier");
      }
      await client.query("UPDATE oauth_authorization_codes SET consumed_at=NOW() WHERE code_hash=$1", [hash]);
      const scopes = Array.isArray(row.scopes) ? row.scopes.map(String) : [];
      // issueTokens uses the pool after this short transaction; the code is already marked consumed.
      return { scopes };
    }).then(({ scopes }) => this.issueTokens(input.clientId, scopes));
  }

  async refresh(input: {
    refreshToken: string;
    clientId: string;
    scope?: string;
  }): Promise<Record<string, unknown>> {
    const hash = sha256(input.refreshToken);
    const result = await this.db.query(
      `SELECT * FROM oauth_tokens WHERE token_hash=$1 AND token_kind='refresh' LIMIT 1`,
      [hash]
    );
    const row = result.rows[0];
    if (!row) throw new Error("Invalid refresh token");

    const familyId = row.family_id ? String(row.family_id) : undefined;
    if (row.revoked_at) {
      if (familyId) {
        await this.db.query("UPDATE oauth_tokens SET revoked_at=COALESCE(revoked_at,NOW()) WHERE family_id=$1", [familyId]);
      }
      throw new Error("Refresh token reuse detected");
    }
    if (new Date(row.expires_at).getTime() <= Date.now()) throw new Error("Refresh token expired");
    if (String(row.client_id) !== input.clientId) throw new Error("Refresh token client mismatch");

    const originalScopes = Array.isArray(row.scopes) ? row.scopes.map(String) : [];
    const requested = input.scope ? normalizeScopes(input.scope) : originalScopes as FoundationScope[];
    if (requested.some(scope => !originalScopes.includes(scope))) throw new Error("Refresh scope escalation is not allowed");

    await this.db.query("UPDATE oauth_tokens SET revoked_at=NOW() WHERE token_hash=$1", [hash]);
    return this.issueTokens(input.clientId, requested, familyId);
  }

  async verifyAccessToken(token: string, requiredScopes: string[] = []): Promise<AccessGrant | null> {
    const result = await this.db.query(
      `SELECT client_id,scopes,expires_at,revoked_at
       FROM oauth_tokens
       WHERE token_hash=$1 AND token_kind='access'
       LIMIT 1`,
      [sha256(token)]
    );
    const row = result.rows[0];
    if (!row || row.revoked_at || new Date(row.expires_at).getTime() <= Date.now()) return null;
    const scopes = Array.isArray(row.scopes) ? row.scopes.map(String) : [];
    if (requiredScopes.some(scope => !scopes.includes(scope))) return null;
    return {
      clientId: String(row.client_id),
      scopes,
      expiresAt: new Date(row.expires_at)
    };
  }

  async cleanup(): Promise<void> {
    await this.db.query("DELETE FROM oauth_authorization_codes WHERE expires_at < NOW() - INTERVAL '1 hour'");
    await this.db.query("DELETE FROM oauth_tokens WHERE expires_at < NOW() - INTERVAL '7 days'");
  }
}
