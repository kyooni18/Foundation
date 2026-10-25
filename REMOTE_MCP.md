# Remote MCP: deployment and compatibility checklist

## Status — do not equate protocol tests with product certification

The opt-in Rust resource server uses **rmcp 3.7** Streamable HTTP at one URL,
`https://YOUR_DOMAIN/mcp`. It shares the existing DB-owning `Service` and its
authorization/projection code; there is no second memory database or user model.
Only `record`, `search`, `get`, and `forget` are exposed. Stdio and the private
`/v1` service retain their existing interfaces and credentials.

**Native ChatGPT and Claude account linking has NOT been verified.** No signed-in
consumer account, configured public development domain, or production issuer
credentials were supplied. See [the test report](reports/remote-mcp-verification.md)
for observed results and remaining acceptance gates. In particular, the Keycloak
configuration below is a provider prerequisite, not evidence that either UI
successfully completed a login. Do not call this deployment product-compatible
until the final checklist passes for both products.

## Client contracts checked against official documentation

Sources consulted:

* [OpenAI Plugins authentication](https://developers.openai.com/plugins/build/auth)
* [Claude consumer connector authentication](https://claude.com/docs/connectors/building/authentication)
* [Claude lazy auth / HTTP challenges](https://claude.com/docs/connectors/building/lazy-authentication)
* [MCP 2026-07-28 authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
* [MCP 2026-07-28 HTTP transport](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
* [Keycloak MCP authorization server configuration](https://www.keycloak.org/securing-apps/mcp-authz-server)

Published documents fetched successfully from this host during implementation:

| Client | CIMD client ID | Exact redirect |
|---|---|---|
| ChatGPT, RFC 9207 mode | `https://chatgpt.com/oauth/client.json` | `https://chatgpt.com/connector_platform_oauth_redirect` |
| Claude web/Desktop cloud connector | `https://claude.ai/oauth/mcp-oauth-client-metadata` | `https://claude.ai/api/mcp/auth_callback` |

ChatGPT's document currently offers plural
`token_endpoint_auth_methods_supported: ["none", "private_key_jwt"]` and legacy
singular `token_endpoint_auth_method: "private_key_jwt"`, with
`jwks_uri: https://chatgpt.com/oauth/jwks.json`. A provider that only reads the
singular field must support the signed assertion path; do not overwrite OpenAI's
metadata to force public-client mode. Claude's document is public (`none`) and
includes an additional JWT-bearer grant in its published grant list. Configure
Keycloak to accept public CIMD metadata containing confidential-only grants and
exclude those grants, rather than rejecting Claude's entire document.

Without complete RFC 9207 support, **copy** ChatGPT's displayed
`https://chatgpt.com/connector/oauth/{callback_id}` and
`https://chatgpt.com/oauth/{callback_id}/client.json` into the provider/configuration.
The braces are placeholders, not working values. Never invent an ID or use a
wildcard. Stable callback mode requires the exact same issuer in resource and AS
metadata, `authorization_response_iss_parameter_supported: true`, and the correct
`iss` in **both successful and error** authorization responses.

Claude uses only the **first** protected metadata authorization server (we publish
one), and selects CIMD only if AS metadata advertises **both** CIMD support and
`none` in token endpoint authentication methods. Otherwise it falls back to DCR.
Its discovery/registration/token deadline is 10 seconds; refresh is 30 seconds.
It needs a real HTTP `401` to begin login, not a `200` with an error string.
Scope step-up uses `403` plus `WWW-Authenticate: Bearer error="insufficient_scope"`.
This server also includes tool-result `_meta["mcp/www_authenticate"]` and tool
OAuth `securitySchemes` (top-level and `_meta` mirror) for OpenAI.

These are the **consumer web connector** contracts, not Anthropic's API-level
MCP connector. Claude Code's separate native-loopback client is outside this
web-connector configuration; no broad localhost redirect exceptions are enabled.

## Authorization server (external)

Prime Radiant is not an IdP. Use an independently operated HTTPS OAuth/OIDC AS.
The reference provider is **Keycloak 26.8.0**, whose own MCP guide documents CIMD,
DCR, resource indicators, and RFC 9207. CIMD and resource indicators are still
**experimental features**: enable `--features=cimd,resource-indicators`, review
upgrade notes, and pin your provider version. A managed provider must actually
offer these capabilities; a generic OIDC checkbox is not sufficient.

Configure a dedicated realm as follows (do not run the HTTP test fixture in production):

1. Give the realm a canonical HTTPS issuer, e.g. `https://auth.example.com/realms/radiant`.
   Preserve its exact spelling everywhere. Configure its proxy/hostname settings,
   TLS, owner login, consent, and normal backup/recovery independently.
2. Create a confidential resource-server client with **client ID equal to the
   canonical resource URI**, `https://YOUR_DOMAIN/mcp`.
   Set its `resource_url` attribute to that same URI. Keycloak introspection checks
   the caller's client ID against the token audience; a different client ID made
   introspection return `active: false` in the disposable-provider test. Disable implicit,
   password, and unnecessary grant types for MCP clients. Create a confidential
   introspection credential and put its secret in a private 0600 file on Radiant.
   This is a **resource-server credential**, NOT an API key or a consumer-client
   secret. Introspection on every OAuth request makes revoked sessions fail closed.
3. Create OIDC client scopes `memory.read`, `memory.write`, `memory.forget`, with
   inclusion in the access-token `scope`. Set them as supported/default/optional
   scopes as appropriate for each registration route. **Retain the provider's
   built-in `basic` subject/session mappers**; do not replace all built-in scopes
   with the three memory scopes. A JWT without `sub` cannot identify the trusted
   account and may fail provider introspection. For DCR/pre-registration,
   attach an Audience mapper targeting the resource-server client. With the
   resource-indicators feature, authorize/token `resource` must select the same
   `resource_url`; invalid or changed resources must be rejected (`invalid_target`).
   Do not accept an ID token as an access token or substitute a consumer client ID
   for the canonical resource audience.
4. Enforce Authorization Code and **PKCE S256** for public AND confidential MCP
   clients, using Keycloak client policies (PKCE enforcer) and S256 client settings.
   Keep token form-urlencoded parsing, refresh grants, consent, and RFC 9207 enabled.
   Access tokens must be RS256 JWTs, with `iss`, `aud`, `sub`, `iat`, `exp`, `scope`,
   and `azp` or `client_id`. Configure a short lifespan (300 seconds recommended;
   this resource server rejects lifetimes over 3600 seconds). Set Revoke Refresh
   Token on and Refresh Token Max Reuse to 0 for public-client refresh rotation.
5. For CIMD, adapt `deployment/keycloak-client-policies.example.json` to the exact
   resource URL. Import its `profiles` and `policies` sections at realm Client
   Policies (or PUT them separately to the realm admin
   `/client-policies/profiles` and `/client-policies/policies` endpoints).
   Merge with existing policies; don't replace unrelated realm policies blindly.
   The policy allows only the published client domains, HTTPS, required metadata
   properties and the one resource indicator. OpenAI's logo origin is separately
   allowlisted; do not replace this list with `*`. Retain metadata size/cache
   limits (Keycloak defaults: 5 KB, 300 s minimum cache). Validate that the
   document's `client_id` equals its URL, redirects match the document exactly,
   and signed assertions use the advertised JWKS and accepted algorithms.
6. Restrict AS egress to the allowlisted public HTTPS origins. Block private,
   link-local, metadata-service and internal-network destinations at the provider
   network boundary, including after DNS resolution; forbid HTTP downgrade and
   arbitrary redirect following. Do not let clients choose arbitrary JWKS or
   CIMD fetch URLs. The resource server itself fetches only administrator-pinned
   HTTPS discovery/JWKS/introspection URLs, never token `jku`/`x5u` or client URLs.
7. Keep advertised AS metadata truthful: issuer, auth/token endpoints, S256,
   supported scopes and client auth methods, CIMD only with its policy enabled,
   DCR only with a usable registration endpoint. In particular do not merely add
   `client_id_metadata_document_supported` to an otherwise incompatible IdP.
   Validate both clients against the actual issuer before deploying.

The resource server validates discovery on startup, pins issuer/JWKS/introspection
locations, verifies JWT signatures/audience/time, introspects active status, then
checks explicit `(subject, client_id)` bindings. Unlisted subjects never become
`user`. Scopes are an **additional** gate, not internal permission overrides.
Default sample bindings deliberately assign both assistants `agent`, not owner.
`user`, `agent`, `import`, original-source visibility, Claim publication and
forgotten/retracted policies remain the existing application policies.

### DCR compatibility fallback (bounded, exact redirects)

Keycloak's built-in registration policies alone are not a substitute for exact
product callback admission. If public DCR is enabled, run the optional
`deployment/dcr_gate.py` in front of **only** the provider's registration endpoint.
It delegates RFC7591 registration to Keycloak, not token issuance or user login.
Install its pinned aiohttp dependency in an isolated venv; edit a private 0600
copy of `deployment/dcr-gate.example.json` and initialize it once:

```sh
python3 -m venv /private/path/dcr-venv
/private/path/dcr-venv/bin/pip install -r deployment/dcr-gate-requirements.txt
/private/path/dcr-venv/bin/python deployment/dcr_gate.py /private/path/dcr.json --init-registry
/private/path/dcr-venv/bin/python deployment/dcr_gate.py /private/path/dcr.json
```

Route the exact HTTPS `registration_endpoint` advertised by Keycloak to the gate's
loopback listener, preserving `Host: auth.example.com`. Prevent all direct public
access to the upstream registration endpoint and client-management endpoints.
Put a short-lived, count-limited Keycloak **initial access token** in the configured
0600 file (at most 100 registrations, then manually renew after review). Do not
give that token to ChatGPT/Claude. The gate admits only one exact approved callback,
approved grants/scopes/auth methods, no remote URL-bearing metadata extensions,
8 KB bodies, 10 attempts/hour, and at most 100 persistent client IDs. The provider
must apply S256 and resource audience/scopes to every dynamically registered
client through realm policies/default client scopes. No open anonymous provider
registration route should remain behind the gate. Upstream must be HTTPS even
when the gate's public-facing listener is loopback.

Share the gate's **0600 client-ID-only** registry with the resource server under
the same trusted OS uid; set `dcr_registry` in `remote.json` and add explicit
bindings with your trusted subject and `client_id: "dcr:chatgpt"` / `"dcr:claude"`.
The gate tags IDs only from exact approved redirects. Each issued client ID
still gets a separate credential/provenance fingerprint. Removing/disabling its
registry entry revokes that client at the resource server. Keep the provider's
client ID/secret stable while a connection uses it. Client secrets are not stored
in the gate registry. Refresh tokens remain entirely client/provider managed.

If your managed provider cannot front its DCR endpoint with such a policy or an
equivalent native admission control, **disable DCR** and use CIMD or pre-registration;
do not advertise a broken or unrestricted fallback. The gate's end-to-end public
provider integration remains a deployment validation item, not a passed UI test.

### Pre-registered fallback

Create separate OAuth clients for ChatGPT and Claude when needed. Configure exact
redirects from the table or the actual ChatGPT management page; S256; authorization
code + rotating refresh; audience/scopes above. A secret is optional for public
clients (`none`), required only for confidential ones (`client_secret_basic` or
`client_secret_post` as supported by your provider). Add their exact issued IDs as
separate trusted-subject bindings in `remote.json`, and enter IDs/secrets in each
product's native OAuth configuration. Never substitute a Prime Radiant API key.

## Resource-server configuration and key CLI

Copy `deployment/remote.example.json` privately, edit domains/issuer/paths and
your immutable OAuth `sub` (from your trusted IdP account, not an unverified token),
and set ownership/mode to the service uid/0600. Paths may not traverse symlinks;
on macOS use `/private/...` rather than `/var/...` symlink aliases.

For a non-container service:

```sh
umask 077
primeradiant remote-key-init /private/path/remote-keys.db
primeradiant remote-key-issue /private/path/remote-keys.db \
  --kind agent --label script --scopes 'memory.read' --expires UNIX_TIMESTAMP > /private/path/script.key
primeradiant remote-key-list /private/path/remote-keys.db
primeradiant remote-key-revoke /private/path/remote-keys.db ID_FROM_LIST
primeradiant --db EXISTING_DB --embedding off serve \
  --credentials EXISTING_LOCAL_REGISTRY --remote-config /private/path/remote.json
```

Keys are `prk_` + 256 cryptographically random bits, printed once; only SHA-256
digests persist. Expiration is mandatory. Rotate by issuing a replacement,
updating the client, then revoking the old ID. A read-only scope cannot record;
even a fully scoped agent cannot forget a user-authored source. Existing local
64-hex stdio/API credentials remain local and are **not** accepted as remote keys.
Use `Authorization: Bearer prk_...` for generic clients. OAuth tokens go in the
same header on the **same** `/mcp` URL. Prefixes select disjoint validators.

Public routes are only `/mcp`, `/.well-known/oauth-protected-resource`, and
`/.well-known/oauth-protected-resource/mcp`. Metadata is unauthenticated and
contains the canonical `/mcp` resource, one exact issuer and all three scopes.
OAuth AS discovery stays at the issuer's RFC8414/OIDC well-known endpoint.

rmcp handles 2026-07-28 per-request protocol/capability metadata and method/name
headers (no initialize/session handshake), plus 2025-11-25 and 2025-06-18 legacy
initialization. The remote listener is stateless even for legacy clients, and
does not use transport session IDs as authentication. Do not write a 2026 client
that only changes the version string on a 2025 request.

## Mac Docker Compose + HTTPS without a VPS or new memory DB

The default Compose service does **not** enable remote MCP. Keep existing volumes
and the Compose project name; **never** run `down -v` or reinitialize an existing
memory database. Provision `/credentials/remote.json`, `introspection.secret`,
and the remote key registry as uid 10001/mode 0600, in the existing credentials
volume. You can pipe the edited JSON into an ephemeral `docker compose run --rm
--entrypoint sh radiant -c 'umask 077; cat > /credentials/remote.json'`. Do not put
actual secrets in shell history, Compose YAML, git, or environment examples.
Initialize the remote registry with `docker compose run --rm radiant remote-key-init
/credentials/remote-keys.db`; issue/list/revoke via the same local container CLI.

Use a **named Cloudflare Tunnel** or an equivalent secure HTTPS tunnel. The optional
`compose.remote.yaml` runs cloudflared in the radiant network namespace, so it can
reach the public **loopback-only** router at `http://127.0.0.1:8787`. It publishes
no Docker host port. In the tunnel's published application route, set:

* public hostname: the exact resource-server domain;
* service: `http://127.0.0.1:8787`;
* HTTP Host Header: that exact public domain;
* edge HTTPS-only (redirect HTTP to HTTPS), valid certificate, no browser Access
  interstitial on MCP/discovery paths, and a catch-all reject for other hostnames;
* no route to port 7823, `/v1`, the Docker socket, databases or maintenance APIs.

Store the tunnel token in a private file on the Mac, set `MCP_TUNNEL_TOKEN_FILE`
to its absolute path, then:

```sh
docker compose -f compose.yaml -f compose.remote.yaml --profile remote up -d
```

This uses an external, already configured HTTPS identity provider; the Mac does
not need a VPS or a replacement memory DB. A self-operated AS has its own persistence
and operational requirements; the disposable Keycloak test container is not one.
The tunnel overlay/image and actual public edge must be validated on your account.

For another reverse proxy, preserve canonical Host, strip untrusted forwarding
headers, terminate HTTPS, and proxy only the three public routes to loopback.
The Rust server deliberately ignores **all** forwarded headers; no client IP or
identity is derived from them. It checks Host exactly, permits only the canonical
Origin when present, bounds bodies/auth/network latency/concurrency and applies a
global 600-requests/minute cap. Add edge IP rate limits and connection/time/header
limits for Internet deployment. No in-app IP limiter trusts spoofable proxy IPs.

## Connect the two products (manual acceptance)

### ChatGPT Plugins / custom MCP

1. Start the public HTTPS service and inspect protected metadata with no token.
   `/mcp` without a credential must return `401` with `resource_metadata` in
   `WWW-Authenticate`, never a login HTML page or a `200` wrapper error.
2. In ChatGPT's custom MCP/Plugins management, enter **only**
   `https://YOUR_DOMAIN/mcp`, select OAuth, and prefer CIMD if offered. Inspect the
   displayed metadata/callback mode. Configure the provider for those exact URLs.
   For DCR select its supported route; for pre-registration enter that client's
   ID/optional secret, not an API key. Enable the needed workspace/developer
   permissions if the UI is unavailable on your plan.
3. Connect, follow native sign-in and consent as your explicitly bound owner
   subject, confirm four tools, record agent text, search approved Claims, get an
   authorized source and forget the agent source. Test an existing user source's
   protections too. Unapproved extracted content must not become searchable.
4. Wait for token expiration, test automatic refresh/reconnect, revoke the IdP
   session, and verify linking is offered again. Test scope step-up and cancellation.

### Claude web / Desktop cloud-hosted connector

1. In claude.ai Settings → Connectors → Add custom connector, enter the **same**
   HTTPS `/mcp` URL. Desktop's cloud connector shares this web flow; do not point
   it at localhost or confuse it with a local stdio configuration/API connector.
2. Prefer automatic CIMD/DCR. For manual OAuth use the advanced client ID/secret
   fields. Register exactly `https://claude.ai/api/mcp/auth_callback`. A public
   client can leave the secret blank. Directory-held Anthropic credentials do not
   automatically apply to a user-added custom connector.
3. Click Connect; verify real native login, consent, tool use, expiry/refresh,
   reconnect, revoked-session rejection, and scope step-up against this issuer.
   Repeat in Desktop's cloud-hosted connector if that surface is in use.

Record outcomes for **both** products in the verification report. An Inspector,
Claude Code, API connector, or generic SDK test cannot substitute for these steps.

## Troubleshooting

| Symptom | Check |
|---|---|
| No login/link UI | Real HTTP 401, unmodified WWW-Authenticate, reachable metadata, correct OAuth UI mode; no CDN login/interstitial/cache |
| Claude falls back to DCR unexpectedly | CIMD boolean **and** token auth `none`; first issuer; allow about five minutes for discovery caches |
| ChatGPT rejects stable callback | Exact issuer equality and RFC9207 `iss` on success/errors; use the displayed callback-ID URI otherwise |
| CIMD fails in provider | Fetch published document from provider network; HTTPS/domain/size policies; doc client_id equality; OpenAI plural vs singular auth field/JWKS; Claude extra grant; Cloudflare/WAF 403 |
| DCR 400/429/503 | Exact displayed callback, allowed fields/scopes, quota, private upstream, initial access token expiration/count, provider registration policy; no wildcards |
| `invalid_client` during refresh | Preserve registered ID/secret and supported auth method; check client auth, rotated refresh tokens and provider policy |
| `invalid_target` | Same exact `/mcp` resource at auth and token; Keycloak resource_url/audience configuration |
| OAuth 401 | RS256 signature/kid, issuer, audience, exp/iat/nbf, lifespan, active introspection, trusted subject AND azp/client_id mapping (including DCR registry) |
| OAuth 503 | Provider/JWKS/introspection unavailable, bad RS introspection secret, private file permissions; fail closed, don't substitute API keys for OAuth |
| 403 insufficient_scope | Reauthorize requested scopes; API key clients issue a properly scoped replacement; internal Forbidden cannot be cured by more OAuth scope |
| 421 / Origin rejection | Proxy must preserve canonical Host; don't trust forwarded Host; reject malicious Origin; cloud clients normally send no Origin |
| 2026 request rejected | Include per-request protocol/capability `_meta` and matching MCP method/name/version headers, or use an actual current SDK |
| New signing key initially rejected | Publish JWKS keys ≥60s before issuing with new kid; retain overlap; fixed pinned JWKS URL only; unknown kids don't cause unbounded fetches |

Never log bearer tokens, authorization codes, refresh tokens, client secrets,
registration tokens, or full sensitive request bodies at proxies or the AS.
