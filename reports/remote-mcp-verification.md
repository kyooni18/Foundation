# Remote MCP verification

## Observed results

This is an implementation/test report, **not ChatGPT or Claude certification**.
No authenticated consumer account or public HTTPS development endpoint was
available. No paid provider requests were made. Existing persistent volumes were
not recreated, migrated, or deleted by this work.

Passed commands:

* `cargo test --locked -- --test-threads=1`: all executed tests passed;
  existing benchmark/worker/paid tests retain their ignored status.
* `cargo test --locked --lib transport::remote -- --test-threads=1`: 5 passed,
  including the final argument-schema enforcement test added after the full run.
* `cargo clippy --locked --all-targets -- -D warnings`: passed.
* `python3 scripts/test_dcr_gate.py`: 3 passed (exact callbacks, bounded admission,
  malicious/unsupported registration inputs).
* `python3 scripts/test_mcp_keycloak.py`: passed against disposable pinned
  Keycloak 26.8.0. Real discovery, S256 policy, issuer response, bounded initial
  registration, exact ChatGPT and Claude redirects, login, code exchange,
  canonical resource audience, refresh rotation, code replay rejection, and
  authenticated introspection before/after session revocation were exercised.
  These are simulated clients using the actual callback strings, not product UIs.

Rust remote tests exercise API-key hash-only persistence, expiration, revocation,
replacement-key issuance, scope enforcement, explicit principal binding, signed
JWT validation (issuer/audience/time/algorithm), unknown-key JWKS refresh, and
OAuth/API-key access through the same router. Loopback HTTP fixtures test
2026-07-28 direct requests and 2025-11-25/2025-06-18 initialization, discovery,
HTTP challenges, four-tool listings/security schemes, argument validation,
record/get/search/forget and forgotten-data hiding. An official rmcp Streamable
HTTP client connects with both credential types. Existing transport tests cover
stdio/private HTTP projection and authorization invariants.

## Failed or unverified gates

`python3 scripts/test_mcp_keycloak.py --published-cimd` **failed**: the container's
provider returned HTTP 400, `Client Metadata fetch failed`, for
`https://chatgpt.com/oauth/client.json`. Host-side published documents were
retrieved and their contracts inspected during implementation, but this does
not prove provider-side CIMD interoperability. Claude's provider-side published
fetch was not reached in that run. Resolve provider egress/TLS and rerun; do not
disable certificate validation or replace the official metadata documents.

CIMD security is delegated to Keycloak and its explicit URL/redirect client
policies, not reimplemented in the resource server. Adversarial live CIMD
fetch/redirect/DNS tests, full production DCR gate-to-provider deployment, and
pre-registered confidential consumer-client login have not been verified.
The disposable provider smoke test and resource-server tests are separate; a
full HTTPS real-provider-to-resource-server end-to-end test remains outstanding.
MCP Inspector was not run. Its success would not replace either consumer UI test.

## Remaining manual acceptance steps

Follow `REMOTE_MCP.md` to provision HTTPS issuer, strict client policies,
resource-server introspection client, subject/client bindings, and one public
`https://YOUR_DOMAIN/mcp`. Verify both protected metadata URLs and exact issuer.
Rerun published-CIMD tests from the provider's actual network; verify signed
ChatGPT assertions if its metadata selects `private_key_jwt`.

For **each** of ChatGPT Plugins/custom MCP and Claude web custom connector:

1. Add that same HTTPS URL using native OAuth, with no API key.
2. Observe discovery and CIMD; separately test bounded DCR and configured-client
   fallbacks. Check exact callback, consent, issuer and S256 without logging
   codes, assertions, tokens, cookies, or secrets.
3. List exactly record/search/get/forget; execute each under the assigned agent
   principal and verify Original Sources and unpublished Claims stay hidden.
4. Wait for access-token expiration and execute again to prove automatic refresh;
   reconnect, revoke the session, and prove authorization fails closed/relinking.
5. Test ChatGPT's displayed callback-ID URI if stable RFC 9207 mode is unavailable.
   Test Claude Desktop's cloud connector separately from local stdio and the
   Anthropic API-level MCP feature.

Acceptance criteria 2–4 are **not yet established** without these account-level
tests. Deployment configuration examples are prerequisites, not successful-login
evidence. No production credentials or tokens are included in the repository.
