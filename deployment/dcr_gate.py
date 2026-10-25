#!/usr/bin/env python3
"""Bounded RFC7591 admission proxy, NOT an authorization server.

Run only behind the authorization server's HTTPS reverse proxy. That proxy
must route its advertised registration_endpoint here and prevent direct access
to the provider's registration endpoint. All OAuth issuance stays in Keycloak.
"""
import argparse
import asyncio
import ipaddress
import json
import os
from pathlib import Path
import sqlite3
import stat
import time
from urllib.parse import urlsplit

SCOPES = {"memory.read", "memory.write", "memory.forget", "openid", "profile", "email", "offline_access"}
CALLBACKS = {
    "https://claude.ai/api/mcp/auth_callback": "claude",
    "https://chatgpt.com/connector_platform_oauth_redirect": "chatgpt",
}


def private(path):
    path = Path(path)
    # Resolve no symlink components, including the final component.
    if path.absolute() != path.resolve():
        raise ValueError("symlink path")
    s = path.stat()
    if not stat.S_ISREG(s.st_mode) or stat.S_IMODE(s.st_mode) != 0o600 or s.st_uid != os.geteuid() or s.st_nlink != 1 or s.st_size > 1024 * 1024:
        raise ValueError("private 0600 regular file required")
    return path.read_bytes()


def admission(document, callbacks):
    """Return (sanitized registration, trust tag). Reject extension URLs/SSRF."""
    if not isinstance(document, dict):
        raise ValueError("object required")
    allowed = {"redirect_uris", "client_name", "client_uri", "grant_types", "response_types", "token_endpoint_auth_method", "scope"}
    if set(document) - allowed:
        raise ValueError("unsupported registration fields")
    redirects = document.get("redirect_uris")
    if not isinstance(redirects, list) or len(redirects) != 1 or redirects[0] not in callbacks:
        raise ValueError("exact approved redirect required")
    if document.get("grant_types", ["authorization_code"]) not in (["authorization_code"], ["authorization_code", "refresh_token"], ["refresh_token", "authorization_code"]):
        raise ValueError("authorization code only")
    if document.get("response_types", ["code"]) != ["code"]:
        raise ValueError("code response required")
    if document.get("token_endpoint_auth_method", "client_secret_basic") not in {"none", "client_secret_basic", "client_secret_post"}:
        raise ValueError("unsupported token authentication")
    scope = document.get("scope", "")
    if not isinstance(scope, str) or not set(scope.split()) <= SCOPES:
        raise ValueError("unsupported scope")
    name = document.get("client_name", "MCP client")
    if not isinstance(name, str) or not 1 <= len(name) <= 200:
        raise ValueError("invalid client name")
    tag = callbacks[redirects[0]]
    # Informational client_uri is never fetched; require the appropriate origin.
    if "client_uri" in document and document["client_uri"] not in {"https://claude.ai", "https://claude.ai/", "https://chatgpt.com", "https://chatgpt.com/"}:
        raise ValueError("unsupported client URI")
    result = dict(document)
    result["grant_types"] = ["authorization_code", "refresh_token"]
    result["response_types"] = ["code"]
    return result, tag


def initialize_registry(path):
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.close(fd)
    with sqlite3.connect(path) as db:
        db.execute("CREATE TABLE clients(client_id TEXT PRIMARY KEY, tag TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1)")


def application(config):
    from aiohttp import web, ClientSession, ClientTimeout
    upstream = urlsplit(config["upstream"])
    if upstream.scheme != "https" or not upstream.hostname or upstream.username or upstream.password or upstream.query or upstream.fragment:
        raise ValueError("static HTTPS registration upstream required")
    private(config["initial_access_token_file"])
    private(config["registry"])
    callbacks = dict(CALLBACKS)
    # Exact callback-ID-specific ChatGPT URIs copied by the owner, never wildcards.
    for uri in config.get("chatgpt_callback_uris", []):
        p = urlsplit(uri)
        if p.scheme != "https" or p.netloc != "chatgpt.com" or not p.path.startswith("/connector/oauth/") or not p.path.removeprefix("/connector/oauth/").replace("-", "").replace("_", "").isalnum() or p.query or p.fragment:
            raise ValueError("invalid callback-ID URI")
        callbacks[uri] = "chatgpt"
    lock = asyncio.Lock()
    rate = [time.monotonic(), 0]

    async def register(request):
        if request.host != config["public_host"] or "Origin" in request.headers:
            raise web.HTTPForbidden()
        if request.content_type != "application/json":
            raise web.HTTPUnsupportedMediaType()
        if lock.locked():
            raise web.HTTPTooManyRequests()
        async with lock:
            if time.monotonic() - rate[0] > 3600:
                rate[:] = [time.monotonic(), 0]
            if rate[1] >= 10:
                raise web.HTTPTooManyRequests()
            rate[1] += 1
            try:
                body = await asyncio.wait_for(request.json(), timeout=5)
                registration, tag = admission(body, callbacks)
            except (ValueError, TypeError, asyncio.TimeoutError):
                return web.json_response({"error": "invalid_client_metadata"}, status=400)
            with sqlite3.connect(config["registry"]) as db:
                if db.execute("SELECT COUNT(*) FROM clients").fetchone()[0] >= 100:
                    raise web.HTTPTooManyRequests()
            token = private(config["initial_access_token_file"]).decode().strip()
            async with ClientSession(timeout=ClientTimeout(total=8), trust_env=False) as session:
                async with session.post(config["upstream"], json=registration, headers={"Authorization": "Bearer " + token}, allow_redirects=False) as response:
                    data = await response.content.read(65537)
                    if len(data) > 65536 or response.status != 201:
                        # Never return upstream diagnostics that may contain secrets.
                        return web.json_response({"error": "temporarily_unavailable"}, status=503)
                    registered = json.loads(data)
            client_id = registered.get("client_id")
            if not isinstance(client_id, str) or not 1 <= len(client_id) <= 2048 or registered.get("redirect_uris") != registration["redirect_uris"]:
                raise web.HTTPBadGateway()
            with sqlite3.connect(config["registry"]) as db:
                db.execute("INSERT INTO clients(client_id,tag) VALUES (?,?)", (client_id, tag))
            # Client management is deliberately not exposed by this minimal gate.
            registered.pop("registration_access_token", None)
            registered.pop("registration_client_uri", None)
            return web.json_response(registered, status=201, headers={"Cache-Control": "no-store"})

    app = web.Application(client_max_size=8192)
    app.router.add_post(config["path"], register)
    return app


def main():
    from aiohttp import web
    p = argparse.ArgumentParser()
    p.add_argument("config")
    p.add_argument("--init-registry", action="store_true")
    args = p.parse_args()
    config = json.loads(private(args.config))
    if args.init_registry:
        initialize_registry(config["registry"])
        return
    host, port = config["bind"].rsplit(":", 1)
    if not ipaddress.ip_address(host).is_loopback:
        raise ValueError("gate must bind loopback")
    web.run_app(application(config), host=host, port=int(port), access_log=None, print=None)


if __name__ == "__main__":
    main()
