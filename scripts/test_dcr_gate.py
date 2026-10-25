#!/usr/bin/env python3
"""Offline registration-admission policy tests, no third-party dependencies."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import sqlite3

spec = importlib.util.spec_from_file_location("gate", Path(__file__).resolve().parents[1] / "deployment/dcr_gate.py")
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class Admission(unittest.TestCase):
    def test_both_clients_and_exact_callbacks(self):
        for uri, tag in gate.CALLBACKS.items():
            request, actual = gate.admission({"redirect_uris": [uri], "token_endpoint_auth_method": "none", "grant_types": ["authorization_code", "refresh_token"]}, gate.CALLBACKS)
            self.assertEqual(actual, tag)
            self.assertEqual(request["redirect_uris"], [uri])
            for bad in [uri + "/evil", uri + "?redirect=https://evil.example", "http://127.0.0.1/callback", "https://evil.example", "https://claude.ai/*"]:
                with self.assertRaises(ValueError):
                    gate.admission({"redirect_uris": [bad]}, gate.CALLBACKS)

    def test_reject_extensions_ssrf_grants_scopes(self):
        good = {"redirect_uris": [next(iter(gate.CALLBACKS))]}
        for key, value in [("jwks_uri", "https://127.0.0.1/private"), ("logo_uri", "http://169.254.169.254/"), ("grant_types", ["client_credentials"]), ("scope", "memory.read admin"), ("response_types", ["token"]), ("token_endpoint_auth_method", "private_key_jwt")]:
            with self.assertRaises(ValueError):
                gate.admission({**good, key: value}, gate.CALLBACKS)

    def test_private_registry_schema_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp).resolve() / "registry.db"
            gate.initialize_registry(path)
            gate.private(path)
            with self.assertRaises(FileExistsError):
                gate.initialize_registry(path)
            with sqlite3.connect(path) as db:
                db.execute("INSERT INTO clients(client_id,tag) VALUES (?,?)", ("test-client", "claude"))
                self.assertEqual(db.execute("SELECT enabled FROM clients").fetchone(), (1,))


if __name__ == "__main__":
    unittest.main()
