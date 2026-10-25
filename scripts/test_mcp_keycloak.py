#!/usr/bin/env python3
"""Disposable real AS smoke test. No paid calls or account credentials.

This is NOT a ChatGPT/Claude consumer UI test. Never deploy this HTTP fixture.
Docker + Python 3 stdlib only; removes its container/anonymous volumes on exit.
"""
import base64
import hashlib
import html
import http.cookiejar
import json
from pathlib import Path
import re
import secrets
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request

IMAGE = "quay.io/keycloak/keycloak:26.8.0@sha256:27b3230fbabb8c9ff3c66c864cf09dcd8a41aca82e5100679d109ef0eeb05741"
CALLBACKS = ["https://chatgpt.com/connector_platform_oauth_redirect", "https://claude.ai/api/mcp/auth_callback"]
RESOURCE = "https://memory.example/mcp"


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def run():
    name = "pr-mcp-test-" + secrets.token_hex(5)
    password = secrets.token_urlsafe(32)
    subprocess.run(["docker", "run", "-d", "--name", name, "-p", "127.0.0.1::8080", "-e", "KC_BOOTSTRAP_ADMIN_USERNAME=fixture", "-e", "KC_BOOTSTRAP_ADMIN_PASSWORD=" + password, IMAGE, "start-dev", "--features=cimd,resource-indicators"], check=True, stdout=subprocess.DEVNULL)
    try:
        port = json.loads(subprocess.check_output(["docker", "inspect", name]))[0]["NetworkSettings"]["Ports"]["8080/tcp"][0]["HostPort"]
        base = "http://127.0.0.1:" + port

        def request(path, data=None, token=None, form=False, method=None, opener=None):
            headers = {}
            if token:
                headers["Authorization"] = "Bearer " + token
            if data is not None:
                headers["Content-Type"] = "application/x-www-form-urlencoded" if form else "application/json"
                data = urllib.parse.urlencode(data).encode() if form else json.dumps(data).encode()
            req = urllib.request.Request(base + path if path.startswith("/") else path, data=data, headers=headers, method=method)
            return (opener.open if opener else urllib.request.urlopen)(req, timeout=10)

        for _ in range(120):
            try:
                with request("/realms/master/.well-known/openid-configuration") as r:
                    if r.status == 200:
                        break
            except (OSError, urllib.error.URLError):
                time.sleep(1)
        else:
            raise RuntimeError("Keycloak failed to start")
        admin = json.load(request("/realms/master/protocol/openid-connect/token", {"grant_type": "password", "client_id": "admin-cli", "username": "fixture", "password": password}, form=True))["access_token"]
        realm = {
            "realm": "radiant-test", "enabled": True, "sslRequired": "none",
            "accessTokenLifespan": 300, "revokeRefreshToken": True, "refreshTokenMaxReuse": 0,
            "users": [{"username": "owner", "firstName": "Fixture", "lastName": "Owner", "email": "owner@example.test", "emailVerified": True, "requiredActions": [], "enabled": True, "credentials": [{"type": "password", "value": password, "temporary": False}]}],
            "clientScopes": [{"name": s, "protocol": "openid-connect", "attributes": {"include.in.token.scope": "true"}} for s in ["memory.read", "memory.write", "memory.forget"]] + [{"name":"basic", "protocol":"openid-connect", "attributes":{"include.in.token.scope":"false"}, "protocolMappers":[{"name":"sub", "protocol":"openid-connect", "protocolMapper":"oidc-sub-mapper", "config":{"access.token.claim":"true", "introspection.token.claim":"true"}}, {"name":"session", "protocol":"openid-connect", "protocolMapper":"oidc-session-state-mapper", "config":{"access.token.claim":"true", "introspection.token.claim":"true"}}]}],
            "clients": [{"clientId": RESOURCE, "enabled": True, "protocol": "openid-connect", "publicClient": False, "secret": password, "standardFlowEnabled": False, "serviceAccountsEnabled": True, "attributes": {"resource_url": RESOURCE}}],
        }
        request("/admin/realms", realm, admin).close()
        policy = json.loads((Path(__file__).resolve().parents[1] / "deployment/keycloak-client-policies.example.json").read_text().replace("https://memory.example.com/mcp", RESOURCE))
        request("/admin/realms/radiant-test/client-policies/profiles", policy["profiles"], admin, method="PUT").close()
        request("/admin/realms/radiant-test/client-policies/policies", policy["policies"], admin, method="PUT").close()
        root = "/realms/radiant-test"
        metadata = json.load(request(root + "/.well-known/openid-configuration"))
        assert metadata["issuer"] == base + root
        assert "S256" in metadata["code_challenge_methods_supported"]
        assert metadata["authorization_response_iss_parameter_supported"] is True
        assert metadata["client_id_metadata_document_supported"] is True
        assert "none" in metadata["token_endpoint_auth_methods_supported"]
        print("PASS: real AS discovery, S256, RFC 9207, CIMD advertisement, public client auth")

        # Free official metadata fetches from the disposable provider network,
        # not an emulated OAuth client document or a consumer account login.
        if "--published-cimd" in __import__("sys").argv:
            for identity, callback in [("https://chatgpt.com/oauth/client.json", CALLBACKS[0]), ("https://claude.ai/oauth/mcp-oauth-client-metadata", CALLBACKS[1])]:
                challenge = base64.urlsafe_b64encode(hashlib.sha256(secrets.token_urlsafe(48).encode()).digest()).rstrip(b"=").decode()
                query = urllib.parse.urlencode({"client_id":identity,"redirect_uri":callback,"response_type":"code","scope":"memory.read","resource":RESOURCE,"code_challenge":challenge,"code_challenge_method":"S256","state":"fixture"})
                opener = urllib.request.build_opener(NoRedirect())
                try:
                    page = request(metadata["authorization_endpoint"] + "?" + query, opener=opener).read().decode()
                    assert "kc-form-login" in page
                except urllib.error.HTTPError as e:
                    params = urllib.parse.parse_qs(urllib.parse.urlsplit(e.headers.get("Location", "")).query)
                    messages = re.findall(r'<(?:p|span)[^>]*class="[^"]*(?:instruction|error)[^"]*"[^>]*>([^<]*)', e.read().decode())
                    raise AssertionError((identity, e.code, messages, {k:params[k] for k in ("error","error_description") if k in params})) from None
                print("PASS: provider fetched published CIMD, accepted exact callback and reached login:", identity)

        # Bounded initial-access token: no anonymous registration service.
        initial = json.load(request("/admin/realms/radiant-test/clients-initial-access", {"count": 3, "expiration": 300}, admin))["token"]
        for index, callback in enumerate(CALLBACKS):
            registration = json.load(request(metadata["registration_endpoint"], {"client_name": "fixture", "redirect_uris": [callback], "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"], "token_endpoint_auth_method": "none"}, initial))
            client = registration["client_id"]
            # Provision the same strict PKCE and audience policy required for pre-registered clients.
            clients = json.load(request("/admin/realms/radiant-test/clients?clientId=" + urllib.parse.quote(client), token=admin))
            cid = clients[0]["id"]
            config = clients[0]
            config["attributes"] = {**config.get("attributes", {}), "pkce.code.challenge.method": "S256"}
            config["defaultClientScopes"] = ["memory.read", "memory.write", "memory.forget"]
            config["protocolMappers"] = [{"name": "resource", "protocol": "openid-connect", "protocolMapper": "oidc-audience-mapper", "config": {"included.client.audience": RESOURCE, "access.token.claim": "true"}}]
            request("/admin/realms/radiant-test/clients/" + cid, {k:v for k,v in config.items() if k != "protocolMappers"}, admin, method="PUT").close()
            scopes = json.load(request("/admin/realms/radiant-test/client-scopes", token=admin))
            for scope in scopes:
                if scope["name"].startswith("memory.") or scope["name"] in ("basic", "profile", "email", "roles", "web-origins", "acr"):
                    request("/admin/realms/radiant-test/clients/" + cid + "/default-client-scopes/" + scope["id"], token=admin, method="PUT").close()
            request("/admin/realms/radiant-test/clients/" + cid + "/protocol-mappers/models", config["protocolMappers"][0], admin).close()
            print("PASS: bounded DCR and explicit client configuration for", "ChatGPT" if index == 0 else "Claude")

            def authorize(redirect=callback, verifier=None, resource=RESOURCE):
                verifier = verifier or secrets.token_urlsafe(48)
                challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
                query = urllib.parse.urlencode({"client_id": client, "redirect_uri": redirect, "response_type": "code", "scope": "memory.read memory.write memory.forget", "state": "fixture-state", "code_challenge": challenge, "code_challenge_method": "S256", "resource": resource})
                return metadata["authorization_endpoint"] + "?" + query, verifier

            # A different exact callback is rejected; no wildcard redirect matching.
            bad, _ = authorize(redirect=callback + "/evil")
            try:
                response = request(bad)
                body = response.read().decode()
                assert "Invalid parameter: redirect_uri" in body
            except urllib.error.HTTPError as e:
                assert e.code == 400

            # Keycloak treats loopback as a secure browser context. urllib does
            # not; permit its Secure fixture cookies on this loopback-only HTTP
            # test. This is not used by the production resource server or AS.
            jar = http.cookiejar.CookieJar(policy=http.cookiejar.DefaultCookiePolicy(secure_protocols=("https", "http")))
            browser = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar), NoRedirect())
            auth_url, verifier = authorize()
            try:
                page = request(auth_url, opener=browser).read().decode()
            except urllib.error.HTTPError as e:
                params = urllib.parse.parse_qs(urllib.parse.urlsplit(e.headers.get("Location", "")).query)
                raise AssertionError({k: params[k] for k in ("error", "error_description") if k in params}) from None
            action = html.unescape(re.search(r'<form[^>]+action="([^"]+)"', page).group(1))
            try:
                request(action, {"username": "owner", "password": password, "credentialId": ""}, form=True, opener=browser)
                raise AssertionError("expected authorization redirect")
            except urllib.error.HTTPError as e:
                if e.code not in (302, 303):
                    error_page = e.read().decode()
                    messages = re.findall(r'<(?:p|span)[^>]*(?:class="[^"]*(?:instruction|error)[^"]*")[^>]*>([^<]*)', error_page)
                    raise AssertionError(f"login HTTP {e.code}: {messages}") from None
                location = e.headers["Location"]
            parsed = urllib.parse.urlsplit(location)
            assert location.startswith(callback + "?"), "unexpected login redirect path: " + urllib.parse.urlsplit(location).path
            params = urllib.parse.parse_qs(parsed.query)
            assert params["state"] == ["fixture-state"] and params["iss"] == [metadata["issuer"]]
            code = params["code"][0]
            token_request = {"grant_type": "authorization_code", "client_id": client, "code": code, "redirect_uri": callback, "code_verifier": verifier, "resource": RESOURCE}
            tokens = json.load(request(metadata["token_endpoint"], token_request, form=True))
            claims = json.loads(base64.urlsafe_b64decode(tokens["access_token"].split(".")[1] + "=="))
            assert claims["aud"] == RESOURCE or RESOURCE in claims["aud"]
            assert "memory.read" in claims["scope"].split()
            assert "sub" in claims, "provider is missing its built-in subject mapper/basic scope"
            refresh = json.load(request(metadata["token_endpoint"], {"grant_type": "refresh_token", "client_id": client, "refresh_token": tokens["refresh_token"], "resource": RESOURCE}, form=True))
            assert refresh["refresh_token"] != tokens["refresh_token"]
            introspection = {"client_id": RESOURCE, "client_secret": password, "token": refresh["access_token"]}
            active = json.load(request(metadata["introspection_endpoint"], introspection, form=True))
            if not active.get("active"):
                refreshed_claims = json.loads(base64.urlsafe_b64decode(refresh["access_token"].split(".")[1] + "=="))
                raise AssertionError({"introspection": active, "claim_names": list(refreshed_claims), "aud": refreshed_claims.get("aud")})
            request("/admin/realms/radiant-test/users/" + claims["sub"] + "/logout", token=admin, method="POST").close()
            assert json.load(request(metadata["introspection_endpoint"], introspection, form=True))["active"] is False
            print("PASS: authenticated introspection rejects revoked session access token")
            try:
                request(metadata["token_endpoint"], token_request, form=True)
                raise AssertionError("authorization code replay accepted")
            except urllib.error.HTTPError as e:
                assert e.code == 400
            print("PASS: exact callback, login, PKCE exchange, resource audience, issuer response, refresh rotation, code replay rejection")
        print("NOTE: native ChatGPT/Claude linking, CIMD fetch/validation, public HTTPS and full resource-server/provider integration remain separate tests.")
    finally:
        subprocess.run(["docker", "rm", "-fv", name], check=False, stdout=subprocess.DEVNULL)


if __name__ == "__main__":
    run()
