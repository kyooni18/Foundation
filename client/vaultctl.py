#!/usr/bin/env python3

from __future__ import annotations

import base64
import json
import os
import shlex
import socket
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


SYNC_COMMANDS = {"full-push", "full-pull", "delta-push", "delta-pull"}
DEFAULT_BASE_URL = "http://localhost:8000"
DEFAULT_API_KEY = "host"
DEFAULT_MAX_UPLOAD_BYTES = 8 * 1024 * 1024
MIN_MAX_UPLOAD_BYTES = 256 * 1024


class VaultCtlError(RuntimeError):
    pass


@dataclass
class StoredProfile:
    base_url: Optional[str] = None
    api_key: Optional[str] = None
    vault_uid: Optional[str] = None
    local_path: Optional[str] = None
    device_id: Optional[str] = None
    limit: Optional[int] = None
    max_upload_bytes: Optional[int] = None
    state_file: Optional[str] = None

    @staticmethod
    def from_json(raw: Dict[str, Any]) -> "StoredProfile":
        return StoredProfile(
            base_url=trimmed_non_empty(raw.get("baseURL") or raw.get("base_url")),
            api_key=trimmed_non_empty(raw.get("apiKey") or raw.get("api_key")),
            vault_uid=trimmed_non_empty(raw.get("vaultUID") or raw.get("vault_uid")),
            local_path=trimmed_non_empty(raw.get("localPath") or raw.get("local_path")),
            device_id=trimmed_non_empty(raw.get("deviceID") or raw.get("device_id")),
            limit=raw.get("limit"),
            max_upload_bytes=raw.get("maxUploadBytes", raw.get("max_upload_bytes")),
            state_file=trimmed_non_empty(raw.get("stateFile") or raw.get("state_file")),
        )

    def to_json(self) -> Dict[str, Any]:
        out: Dict[str, Any] = {}
        if self.base_url is not None:
            out["baseURL"] = self.base_url
        if self.api_key is not None:
            out["apiKey"] = self.api_key
        if self.vault_uid is not None:
            out["vaultUID"] = self.vault_uid
        if self.local_path is not None:
            out["localPath"] = self.local_path
        if self.device_id is not None:
            out["deviceID"] = self.device_id
        if self.limit is not None:
            out["limit"] = self.limit
        if self.max_upload_bytes is not None:
            out["maxUploadBytes"] = self.max_upload_bytes
        if self.state_file is not None:
            out["stateFile"] = self.state_file
        return out

    def is_empty(self) -> bool:
        return (
            self.base_url is None
            and self.api_key is None
            and self.vault_uid is None
            and self.local_path is None
            and self.device_id is None
            and self.limit is None
            and self.max_upload_bytes is None
            and self.state_file is None
        )


@dataclass
class ProfileOverrides:
    base_url: Optional[str] = None
    api_key: Optional[str] = None
    vault_uid: Optional[str] = None
    local_path: Optional[str] = None
    device_id: Optional[str] = None
    limit: Optional[int] = None
    max_upload_bytes: Optional[int] = None
    state_file: Optional[str] = None
    selected_profile: Optional[str] = None
    set_default: bool = False
    since_unix_ms: Optional[int] = None


@dataclass
class SyncOptions:
    command: str
    base_url: str
    api_key: str
    vault_uid: str
    local_path: Path
    device_id: str
    limit: Optional[int]
    max_upload_bytes: int
    state_file: Path


@dataclass
class BatchedPushResult:
    sent_batches: int
    sent_changes: int
    applied_changes: int
    latest_change_id: Optional[int]
    latest_change_unix_ms: Optional[int]


def eprint(message: str) -> None:
    sys.stderr.write(f"{message}\n")
    sys.stderr.flush()


def info(message: str) -> None:
    print(f"[vaultctl] {message}")


def warn(message: str) -> None:
    eprint(f"[vaultctl] WARN: {message}")


def trimmed_non_empty(raw: Optional[Any]) -> Optional[str]:
    if raw is None:
        return None
    text = str(raw).strip()
    return text if text else None


def normalize_path_string(raw: str) -> str:
    return os.path.abspath(os.path.expanduser(raw))


def mask_secret(raw: Optional[str]) -> str:
    value = trimmed_non_empty(raw)
    if value is None:
        return "-"
    if len(value) <= 8:
        return "********"
    return f"{value[:4]}********{value[-4:]}"


def config_file_path() -> Path:
    override = trimmed_non_empty(os.environ.get("FOUNDATION_VAULTCTL_CONFIG"))
    if override:
        return Path(normalize_path_string(override))
    return Path.home() / ".foundation" / "vaultctl" / "config.json"


def empty_config() -> Dict[str, Any]:
    return {"default_profile": None, "profiles": {}}


def load_config() -> Dict[str, Any]:
    path = config_file_path()
    if not path.exists():
        return empty_config()
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        raise VaultCtlError(f"Failed to load config: {exc}") from exc

    if not isinstance(raw, dict):
        raise VaultCtlError("Failed to load config: root must be JSON object")

    profiles = raw.get("profiles")
    if not isinstance(profiles, dict):
        profiles = {}

    default_profile = raw.get("defaultProfile", raw.get("default_profile"))
    if default_profile is not None and not isinstance(default_profile, str):
        default_profile = None

    return {
        "default_profile": default_profile,
        "profiles": profiles,
    }


def save_config(config: Dict[str, Any]) -> None:
    path = config_file_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    saveable = {
        "defaultProfile": config.get("default_profile"),
        "profiles": config.get("profiles", {}),
    }
    data = json.dumps(saveable, indent=2, sort_keys=True) + "\n"
    path.write_text(data, encoding="utf-8")


def normalize_profile_name(raw: str) -> str:
    name = trimmed_non_empty(raw)
    if not name:
        raise VaultCtlError("Profile name cannot be empty")
    if "/" in name or "\\" in name:
        raise VaultCtlError("Profile name cannot contain path separators")
    return name


def parse_int(raw: Optional[str], default: int) -> int:
    if raw is None:
        return default
    try:
        return int(str(raw).strip())
    except Exception:
        return default


def parse_required_int(flag: str, raw: str, positive: bool = True) -> int:
    try:
        value = int(raw)
    except Exception as exc:
        raise VaultCtlError(f"{flag} must be an integer") from exc
    if positive and value <= 0:
        raise VaultCtlError(f"{flag} must be a positive integer")
    return value


def next_value(flag: str, index: int, arguments: List[str]) -> Tuple[str, int]:
    if index >= len(arguments):
        raise VaultCtlError(f"Missing value for {flag}")
    return arguments[index], index + 1


def should_start_interactive_shell(arguments: List[str]) -> bool:
    if len(arguments) <= 1:
        return True
    return arguments[1] in {"shell", "repl", "interactive"}


def parse_overrides(arguments: List[str]) -> ProfileOverrides:
    overrides = ProfileOverrides()
    index = 0

    while index < len(arguments):
        arg = arguments[index]
        index += 1

        if arg == "--base-url":
            value, index = next_value(arg, index, arguments)
            overrides.base_url = value
        elif arg == "--api-key":
            value, index = next_value(arg, index, arguments)
            overrides.api_key = value
        elif arg == "--vault-uid":
            value, index = next_value(arg, index, arguments)
            overrides.vault_uid = value
        elif arg == "--local-path":
            value, index = next_value(arg, index, arguments)
            overrides.local_path = normalize_path_string(value)
        elif arg == "--device-id":
            value, index = next_value(arg, index, arguments)
            overrides.device_id = value
        elif arg == "--limit":
            value, index = next_value(arg, index, arguments)
            overrides.limit = parse_required_int("--limit", value, positive=True)
        elif arg == "--max-upload-bytes":
            value, index = next_value(arg, index, arguments)
            overrides.max_upload_bytes = parse_required_int("--max-upload-bytes", value, positive=True)
        elif arg == "--state-file":
            value, index = next_value(arg, index, arguments)
            overrides.state_file = normalize_path_string(value)
        elif arg == "--profile":
            value, index = next_value(arg, index, arguments)
            overrides.selected_profile = value
        elif arg == "--set-default":
            overrides.set_default = True
        elif arg == "--since-unix-ms":
            value, index = next_value(arg, index, arguments)
            overrides.since_unix_ms = parse_required_int("--since-unix-ms", value, positive=False)
        elif arg in {"--help", "-h"}:
            raise VaultCtlError(usage_text())
        else:
            raise VaultCtlError(f"Unknown option: {arg}")

    return overrides


def parse_profile_command(arguments: List[str]) -> Dict[str, Any]:
    if not arguments:
        raise VaultCtlError(f"Missing `profile` subcommand.\n\n{usage_text()}")

    subcommand = arguments[0]
    rest = arguments[1:]

    if subcommand == "list":
        return {"type": "profile-list"}
    if subcommand == "show":
        return {"type": "profile-show", "name": rest[0] if rest else None}
    if subcommand == "save":
        if not rest:
            raise VaultCtlError("Missing profile name for `profile save`")
        name = rest[0]
        overrides = parse_overrides(rest[1:])
        return {"type": "profile-save", "name": name, "overrides": overrides}
    if subcommand == "use":
        if not rest:
            raise VaultCtlError("Missing profile name for `profile use`")
        return {"type": "profile-use", "name": rest[0]}
    if subcommand in {"remove", "delete"}:
        if not rest:
            raise VaultCtlError("Missing profile name for `profile remove`")
        return {"type": "profile-remove", "name": rest[0]}

    raise VaultCtlError(f"Unknown profile subcommand: {subcommand}")


def parse_command(arguments: List[str]) -> Dict[str, Any]:
    if len(arguments) <= 1:
        return {"type": "help"}

    command = arguments[1]
    rest = arguments[2:]

    if command in {"help", "--help", "-h"}:
        return {"type": "help"}

    if command == "config":
        if not rest:
            raise VaultCtlError(f"Missing `config` subcommand.\n\n{usage_text()}")
        if rest[0] == "path":
            return {"type": "config-path"}
        raise VaultCtlError(f"Unknown config subcommand: {rest[0]}")

    if command == "profile":
        return parse_profile_command(rest)

    if command == "sync":
        if not rest:
            raise VaultCtlError("Missing sync command")
        sync_command = rest[0]
        if sync_command not in SYNC_COMMANDS:
            raise VaultCtlError(f"Unknown sync command: {sync_command}")
        overrides = parse_overrides(rest[1:])
        return {"type": "sync", "command": sync_command, "overrides": overrides}

    if command in SYNC_COMMANDS:
        overrides = parse_overrides(rest)
        return {"type": "sync", "command": command, "overrides": overrides}

    if command == "status":
        overrides = parse_overrides(rest)
        return {"type": "status", "overrides": overrides}

    raise VaultCtlError(f"Unknown command: {command}\n\n{usage_text()}")


def print_profile_list(config: Dict[str, Any]) -> None:
    profiles = config.get("profiles", {})
    if not profiles:
        print("No saved profiles.")
        return

    default_profile = config.get("default_profile")
    for name in sorted(profiles.keys()):
        profile = StoredProfile.from_json(profiles.get(name, {}))
        marker = "*" if name == default_profile else " "
        print(
            f"{marker} {name}  "
            f"url={profile.base_url or '-'}  "
            f"vault={profile.vault_uid or '-'}  "
            f"path={profile.local_path or '-'}"
        )


def print_profile(name: str, profile: StoredProfile, is_default: bool) -> None:
    print(f"name: {name}")
    print(f"default: {'yes' if is_default else 'no'}")
    print(f"base_url: {profile.base_url or '-'}")
    print(f"api_key: {mask_secret(profile.api_key)}")
    print(f"vault_uid: {profile.vault_uid or '-'}")
    print(f"local_path: {profile.local_path or '-'}")
    print(f"device_id: {profile.device_id or '-'}")
    print(f"limit: {profile.limit if profile.limit is not None else '-'}")
    print(f"max_upload_bytes: {profile.max_upload_bytes if profile.max_upload_bytes is not None else '-'}")
    print(f"state_file: {profile.state_file or '-'}")


def merge_profile(profile: Optional[StoredProfile], overrides: ProfileOverrides) -> StoredProfile:
    resolved = StoredProfile()
    if profile:
        resolved = StoredProfile(
            base_url=profile.base_url,
            api_key=profile.api_key,
            vault_uid=profile.vault_uid,
            local_path=profile.local_path,
            device_id=profile.device_id,
            limit=profile.limit,
            max_upload_bytes=profile.max_upload_bytes,
            state_file=profile.state_file,
        )

    if overrides.base_url is not None:
        resolved.base_url = overrides.base_url
    if overrides.api_key is not None:
        resolved.api_key = overrides.api_key
    if overrides.vault_uid is not None:
        resolved.vault_uid = overrides.vault_uid
    if overrides.local_path is not None:
        resolved.local_path = overrides.local_path
    if overrides.device_id is not None:
        resolved.device_id = overrides.device_id
    if overrides.limit is not None:
        resolved.limit = overrides.limit
    if overrides.max_upload_bytes is not None:
        resolved.max_upload_bytes = overrides.max_upload_bytes
    if overrides.state_file is not None:
        resolved.state_file = overrides.state_file

    return resolved


def resolve_sync_options(
    command: str,
    overrides: ProfileOverrides,
    *,
    require_local_path: bool,
) -> Tuple[SyncOptions, Optional[str]]:
    config = load_config()
    selected_profile_name = overrides.selected_profile or config.get("default_profile")
    profile_json = None
    if selected_profile_name:
        profile_json = config.get("profiles", {}).get(selected_profile_name)
        if profile_json is None:
            raise VaultCtlError(f"Profile not found: {selected_profile_name}")

    stored = StoredProfile.from_json(profile_json or {})
    resolved = merge_profile(stored, overrides)

    env_base_url = trimmed_non_empty(os.environ.get("FOUNDATION_BASE_URL"))
    env_api_key = trimmed_non_empty(os.environ.get("FOUNDATION_API_KEY"))
    env_vault_uid = trimmed_non_empty(os.environ.get("FOUNDATION_VAULT_UID"))
    env_max_upload_bytes = parse_int(os.environ.get("FOUNDATION_MAX_UPLOAD_BYTES"), DEFAULT_MAX_UPLOAD_BYTES)

    local_path_raw = trimmed_non_empty(resolved.local_path)
    local_path: Optional[Path] = None
    if local_path_raw:
        local_path = Path(normalize_path_string(local_path_raw))

    if require_local_path and local_path is None:
        raise VaultCtlError("Missing local path. Set it in a profile or pass --local-path.")

    base_url = trimmed_non_empty(resolved.base_url) or env_base_url or DEFAULT_BASE_URL
    api_key = trimmed_non_empty(resolved.api_key) or env_api_key or DEFAULT_API_KEY
    derived_vault_uid = local_path.name if local_path is not None else None
    vault_uid = trimmed_non_empty(resolved.vault_uid) or env_vault_uid or derived_vault_uid
    if vault_uid is None:
        raise VaultCtlError("Missing vault UID. Set --vault-uid or --local-path.")

    validate_vault_uid(vault_uid)

    device_id = trimmed_non_empty(resolved.device_id) or socket.gethostname() or "python-vaultctl"
    limit = resolved.limit

    max_upload_bytes = resolved.max_upload_bytes
    if max_upload_bytes is None:
        max_upload_bytes = env_max_upload_bytes
    max_upload_bytes = max(MIN_MAX_UPLOAD_BYTES, int(max_upload_bytes))

    if local_path is None:
        # Status-only fallback when no local path is available.
        local_path = Path.cwd()

    state_file_raw = trimmed_non_empty(resolved.state_file)
    if state_file_raw is None:
        state_file = local_path / ".foundation-sync" / "state.json"
    else:
        state_file = Path(normalize_path_string(state_file_raw))

    options = SyncOptions(
        command=command,
        base_url=base_url,
        api_key=api_key,
        vault_uid=vault_uid,
        local_path=local_path,
        device_id=device_id,
        limit=limit,
        max_upload_bytes=max_upload_bytes,
        state_file=state_file,
    )
    return options, selected_profile_name


def validate_vault_uid(raw: str) -> None:
    value = raw.strip()
    if not value:
        raise VaultCtlError("--vault-uid is required")
    if "\x00" in value:
        raise VaultCtlError("--vault-uid contains invalid null byte")
    if "/" in value or "\\" in value:
        raise VaultCtlError("--vault-uid cannot contain path separators")
    if value in {".", ".."}:
        raise VaultCtlError("--vault-uid cannot be '.' or '..'")


def ensure_directory_exists(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)


def ensure_existing_directory(path: Path) -> None:
    if not path.exists() or not path.is_dir():
        raise VaultCtlError(
            "Local vault path does not exist or is not a directory: "
            f"{path}. If this is iCloud Drive, use the real path under "
            "~/Library/Mobile Documents/com~apple~CloudDocs/..."
        )


def prepare_local_vault_directory(path: Path, command: str) -> None:
    if command in {"full-push", "delta-push"}:
        ensure_existing_directory(path)
    else:
        ensure_directory_exists(path)


def default_state(vault_uid: str) -> Dict[str, Any]:
    return {
        "vault_uid": vault_uid,
        "local_snapshot": {},
        "last_server_change_id": None,
        "last_server_change_unix_ms": None,
    }


def load_state(state_file: Path, vault_uid: str) -> Dict[str, Any]:
    if not state_file.exists():
        return default_state(vault_uid)
    try:
        decoded = json.loads(state_file.read_text(encoding="utf-8"))
    except Exception:
        info(f"State file is invalid, starting with a fresh state: {state_file}")
        return default_state(vault_uid)

    if not isinstance(decoded, dict):
        info(f"State file is invalid, starting with a fresh state: {state_file}")
        return default_state(vault_uid)

    if decoded.get("vault_uid") != vault_uid:
        info(f"State file vault_uid mismatch, starting with a fresh state for {vault_uid}.")
        return default_state(vault_uid)

    if "local_snapshot" not in decoded or not isinstance(decoded["local_snapshot"], dict):
        decoded["local_snapshot"] = {}
    return decoded


def save_state(state: Dict[str, Any], state_file: Path) -> None:
    state_file.parent.mkdir(parents=True, exist_ok=True)
    data = json.dumps(state, indent=2, sort_keys=True) + "\n"
    state_file.write_text(data, encoding="utf-8")


def normalize_relative_path(raw: str) -> str:
    trimmed = raw.strip()
    if not trimmed:
        raise VaultCtlError("Empty file path is not allowed")
    if "\x00" in trimmed:
        raise VaultCtlError("File path contains null byte")

    unified = trimmed.replace("\\", "/")
    if unified.startswith("/"):
        raise VaultCtlError(f"Absolute paths are not allowed: {raw}")

    segments: List[str] = []
    for part in unified.split("/"):
        if not part or part == ".":
            continue
        if part == "..":
            raise VaultCtlError(f"Path traversal '..' is not allowed: {raw}")
        segments.append(part)

    if not segments:
        raise VaultCtlError("Empty file path is not allowed")
    return "/".join(segments)


def should_ignore_path(relative_path: str) -> bool:
    return relative_path == ".foundation-sync/state.json" or relative_path.startswith(".foundation-sync/")


def resolve_path(root: Path, relative_path: str) -> Path:
    normalized = normalize_relative_path(relative_path)
    output = root
    for segment in normalized.split("/"):
        output = output / segment
    return output


def file_fingerprint(path: Path) -> str:
    # FNV-1a 64-bit fingerprint for cheap change detection.
    value = 0xCBF29CE484222325
    prime = 0x100000001B3
    with path.open("rb") as handle:
        while True:
            chunk = handle.read(64 * 1024)
            if not chunk:
                break
            for byte in chunk:
                value ^= byte
                value = (value * prime) & 0xFFFFFFFFFFFFFFFF
    return f"{value:016x}"


def scan_fingerprint(path: Path, relative_path: str) -> Optional[str]:
    try:
        return file_fingerprint(path)
    except Exception:
        warn(f"Failed to fingerprint {relative_path}; change detection will fall back to timestamp/size.")
        return None


def scan_local_vault(root: Path, progress_label: Optional[str] = None) -> Dict[str, Dict[str, Any]]:
    snapshot: Dict[str, Dict[str, Any]] = {}
    scanned_dirs = -1  # root itself 제외
    visited_files = 0
    scanned_dirs = 0
    scanned_files = 0
    last_progress_at = time.monotonic()

    for current_root, _, file_names in os.walk(root):
        scanned_dirs += 1
        if progress_label and (scanned_dirs % 50 == 0 or time.monotonic() - last_progress_at >= 2.0):
            info(
                f"{progress_label}: traversed {max(scanned_dirs, 0)} subdirector(ies), "
                f"indexed {len(snapshot)} unique file(s) ({visited_files} visited) ..."
            )
            last_progress_at = time.monotonic()

        current_root_path = Path(current_root)
        for file_name in file_names:
            full_path = current_root_path / file_name
            try:
                if not full_path.is_file():
                    continue
                visited_files += 1
                relative = normalize_relative_path(full_path.relative_to(root).as_posix())
                if should_ignore_path(relative):
                    continue

                stat = full_path.stat()
                snapshot[relative] = {
                    "size_bytes": int(stat.st_size),
                    "modified_unix_ms": int(stat.st_mtime * 1000),
                    "content_fingerprint": scan_fingerprint(full_path, relative),
                }
                if progress_label and (visited_files % 250 == 0 or time.monotonic() - last_progress_at >= 2.0):
                    info(
                        f"{progress_label}: traversed {max(scanned_dirs, 0)} subdirector(ies), "
                        f"indexed {len(snapshot)} unique file(s) ({visited_files} visited) ..."
                    )
                    last_progress_at = time.monotonic()
            except Exception as exc:
                warn(f"Failed to read {full_path}: {exc}")

    return snapshot


def unix_milliseconds_now() -> int:
    return int(round(time.time() * 1000))


def read_local_file(root: Path, relative_path: str) -> bytes:
    file_path = resolve_path(root, relative_path)
    try:
        return file_path.read_bytes()
    except Exception as exc:
        raise VaultCtlError(f"Failed to read file: {file_path}") from exc


def classify_local_change(
    previous: Optional[Dict[str, Any]],
    current: Dict[str, Any],
    prefer_fingerprint_fallback: bool,
) -> str:
    if previous is None:
        return "changed"

    if previous.get("size_bytes") != current.get("size_bytes"):
        return "changed"
    if previous.get("modified_unix_ms") != current.get("modified_unix_ms"):
        return "changed"

    previous_fp = previous.get("content_fingerprint")
    current_fp = current.get("content_fingerprint")
    if previous_fp is not None and current_fp is not None:
        return "unchanged" if previous_fp == current_fp else "changed"

    if prefer_fingerprint_fallback and previous_fp != current_fp:
        return "uncertain"

    return "unchanged"


def is_likely_icloud_drive_path(path: Path) -> bool:
    normalized = str(path.resolve())
    return "/Library/Mobile Documents/" in normalized or "/com~apple~CloudDocs/" in normalized


def make_change_payload(
    *,
    file_path: str,
    action: str,
    changed_at_unix_ms: Optional[int],
    content_base64: Optional[str],
) -> Dict[str, Any]:
    return {
        "file_path": file_path,
        "action": action,
        "changed_at_unix_ms": changed_at_unix_ms,
        "content_base64": content_base64,
        "content_sha256": None,
    }


def build_delta_changes(
    *,
    previous: Dict[str, Dict[str, Any]],
    current: Dict[str, Dict[str, Any]],
    local_root: Path,
    prefer_fingerprint_fallback: bool,
) -> List[Dict[str, Any]]:
    changes: List[Dict[str, Any]] = []

    for path in sorted(current.keys()):
        current_meta = current[path]
        previous_meta = previous.get(path)
        if previous_meta is None:
            encoded = base64.b64encode(read_local_file(local_root, path)).decode("ascii")
            changes.append(
                make_change_payload(
                    file_path=path,
                    action="added",
                    changed_at_unix_ms=current_meta.get("modified_unix_ms"),
                    content_base64=encoded,
                )
            )
            continue

        classification = classify_local_change(previous_meta, current_meta, prefer_fingerprint_fallback)
        if classification != "unchanged":
            encoded = base64.b64encode(read_local_file(local_root, path)).decode("ascii")
            changes.append(
                make_change_payload(
                    file_path=path,
                    action="modified",
                    changed_at_unix_ms=current_meta.get("modified_unix_ms"),
                    content_base64=encoded,
                )
            )

    deleted_timestamp = unix_milliseconds_now()
    for path in sorted(previous.keys()):
        if path not in current:
            changes.append(
                make_change_payload(
                    file_path=path,
                    action="deleted",
                    changed_at_unix_ms=deleted_timestamp,
                    content_base64=None,
                )
            )

    return changes


def build_delta_changes_using_remote_status(
    *,
    local_snapshot: Dict[str, Dict[str, Any]],
    previous_local_snapshot: Dict[str, Dict[str, Any]],
    remote_file_timestamps: List[Dict[str, Any]],
    local_root: Path,
    force_full_mirror: bool,
    prefer_fingerprint_fallback: bool,
) -> List[Dict[str, Any]]:
    remote_by_path: Dict[str, Dict[str, Any]] = {}
    for remote in remote_file_timestamps:
        file_path = remote.get("file_path")
        if not isinstance(file_path, str):
            continue
        try:
            normalized = normalize_relative_path(file_path)
        except VaultCtlError:
            continue
        if should_ignore_path(normalized):
            continue
        remote_by_path[normalized] = remote

    changes: List[Dict[str, Any]] = []
    skipped_stale = 0

    for path in sorted(local_snapshot.keys()):
        local_meta = local_snapshot[path]
        local_change = classify_local_change(
            previous_local_snapshot.get(path),
            local_meta,
            prefer_fingerprint_fallback,
        )

        remote_meta = remote_by_path.get(path)
        if remote_meta is None:
            encoded = base64.b64encode(read_local_file(local_root, path)).decode("ascii")
            changes.append(
                make_change_payload(
                    file_path=path,
                    action="added",
                    changed_at_unix_ms=local_meta.get("modified_unix_ms"),
                    content_base64=encoded,
                )
            )
            continue

        if bool(remote_meta.get("is_deleted")):
            if force_full_mirror or local_change == "changed":
                encoded = base64.b64encode(read_local_file(local_root, path)).decode("ascii")
                changes.append(
                    make_change_payload(
                        file_path=path,
                        action="added",
                        changed_at_unix_ms=local_meta.get("modified_unix_ms"),
                        content_base64=encoded,
                    )
                )
            else:
                skipped_stale += 1
            continue

        remote_updated = int(remote_meta.get("updated_unix_ms") or 0)
        remote_size = int(remote_meta.get("size_bytes") or 0)
        local_modified = int(local_meta.get("modified_unix_ms") or 0)
        local_size = int(local_meta.get("size_bytes") or 0)

        if not force_full_mirror and local_modified < remote_updated and local_change == "unchanged":
            skipped_stale += 1
            continue

        upload_for_uncertain = local_change == "changed" or (
            local_change == "uncertain" and local_modified >= remote_updated
        )

        if (
            upload_for_uncertain
            or local_modified > remote_updated
            or local_size != remote_size
            or (force_full_mirror and local_modified != remote_updated)
        ):
            encoded = base64.b64encode(read_local_file(local_root, path)).decode("ascii")
            changes.append(
                make_change_payload(
                    file_path=path,
                    action="modified",
                    changed_at_unix_ms=local_modified,
                    content_base64=encoded,
                )
            )

    deleted_timestamp = unix_milliseconds_now()
    for remote in remote_file_timestamps:
        file_path = remote.get("file_path")
        if not isinstance(file_path, str):
            continue
        try:
            normalized = normalize_relative_path(file_path)
        except VaultCtlError:
            continue
        if should_ignore_path(normalized):
            continue
        if bool(remote.get("is_deleted")):
            continue
        if normalized in local_snapshot:
            continue
        if not force_full_mirror and normalized not in previous_local_snapshot:
            continue
        changes.append(
            make_change_payload(
                file_path=normalized,
                action="deleted",
                changed_at_unix_ms=deleted_timestamp,
                content_base64=None,
            )
        )

    if skipped_stale > 0:
        warn(f"Skipped {skipped_stale} local file(s) because server has newer timestamps.")

    return changes


def validate_delta_changes_for_upload(changes: List[Dict[str, Any]]) -> None:
    invalid_paths: List[str] = []
    for change in changes:
        action = str(change.get("action", "")).lower()
        if action not in {"added", "modified"}:
            continue
        if trimmed_non_empty(change.get("content_base64")) is None:
            invalid_paths.append(str(change.get("file_path", "<unknown>")))
    if invalid_paths:
        joined = ", ".join(invalid_paths)
        raise VaultCtlError(f"Missing content_base64 for added/modified changes: {joined}")


def compact_payload(value: Any) -> Any:
    if isinstance(value, dict):
        out = {}
        for key, item in value.items():
            if item is None:
                continue
            out[key] = compact_payload(item)
        return out
    if isinstance(value, list):
        return [compact_payload(item) for item in value]
    return value


def encoded_byte_count(payload: Dict[str, Any]) -> int:
    compact = compact_payload(payload)
    body = json.dumps(compact, separators=(",", ":"), ensure_ascii=False)
    return len(body.encode("utf-8"))


def decode_server_message(raw_data: bytes) -> str:
    if not raw_data:
        return "Unknown server error."
    try:
        decoded = json.loads(raw_data.decode("utf-8"))
        if isinstance(decoded, dict):
            reason = trimmed_non_empty(decoded.get("reason"))
            if reason:
                return reason
            error = trimmed_non_empty(decoded.get("error"))
            if error:
                return error
    except Exception:
        pass

    text = trimmed_non_empty(raw_data.decode("utf-8", errors="replace"))
    return text or "Unknown server error."


def api_post(base_url: str, api_key: str, path: str, body: Dict[str, Any]) -> Dict[str, Any]:
    url = base_url.rstrip("/") + path
    compact = compact_payload(body)
    encoded_body = json.dumps(compact, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    request = urllib.request.Request(
        url=url,
        data=encoded_body,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
            "Authorization": f"Bearer {api_key}",
        },
        method="POST",
    )

    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            status_code = int(response.status)
            response_data = response.read()
    except urllib.error.HTTPError as exc:
        message = decode_server_message(exc.read())
        raise VaultCtlError(f"HTTP {exc.code}: {message}") from exc
    except urllib.error.URLError as exc:
        raise VaultCtlError(f"Network error: {exc.reason}") from exc

    if status_code < 200 or status_code >= 300:
        raise VaultCtlError(f"HTTP {status_code}: {decode_server_message(response_data)}")

    try:
        decoded = json.loads(response_data.decode("utf-8"))
    except Exception as exc:
        raise VaultCtlError(f"Failed to decode response for {path}: {exc}") from exc

    if not isinstance(decoded, dict):
        raise VaultCtlError(f"Failed to decode response for {path}: root must be object")
    if decoded.get("ok") is False:
        raise VaultCtlError(str(decoded.get("error") or f"Server returned ok=false for {path}"))
    return decoded


def apply_delta_changes(changed_files: List[Dict[str, Any]], local_root: Path) -> None:
    for change in changed_files:
        file_path_raw = change.get("file_path")
        if not isinstance(file_path_raw, str):
            continue
        relative_path = normalize_relative_path(file_path_raw)
        if should_ignore_path(relative_path):
            continue

        target_file = resolve_path(local_root, relative_path)
        action = str(change.get("action", "")).lower()
        if action in {"deleted", "remove", "removed"}:
            if target_file.exists():
                target_file.unlink()
            continue

        content_base64 = change.get("content_base64")
        if not isinstance(content_base64, str):
            raise VaultCtlError(f"Missing content_base64 for changed file: {relative_path}")
        try:
            content_data = base64.b64decode(content_base64, validate=True)
        except Exception as exc:
            raise VaultCtlError(f"Invalid content_base64 for changed file: {relative_path}") from exc

        target_file.parent.mkdir(parents=True, exist_ok=True)
        target_file.write_bytes(content_data)


def apply_full_snapshot(snapshot_files: List[Dict[str, Any]], local_root: Path) -> None:
    incoming_paths: set[str] = set()

    for item in snapshot_files:
        file_path_raw = item.get("file_path")
        content_base64 = item.get("content_base64")
        if not isinstance(file_path_raw, str):
            continue
        relative_path = normalize_relative_path(file_path_raw)
        if should_ignore_path(relative_path):
            continue
        if not isinstance(content_base64, str):
            raise VaultCtlError(f"Missing content_base64 in snapshot for: {relative_path}")

        incoming_paths.add(relative_path)
        try:
            content_data = base64.b64decode(content_base64, validate=True)
        except Exception as exc:
            raise VaultCtlError(f"Invalid content_base64 in snapshot for: {relative_path}") from exc

        target_file = resolve_path(local_root, relative_path)
        target_file.parent.mkdir(parents=True, exist_ok=True)
        target_file.write_bytes(content_data)

    current_local = scan_local_vault(local_root)
    for existing_path in current_local.keys():
        if existing_path in incoming_paths:
            continue
        resolve_path(local_root, existing_path).unlink(missing_ok=True)


def push_delta_changes_in_batches(options: SyncOptions, changes: List[Dict[str, Any]]) -> BatchedPushResult:
    if not changes:
        return BatchedPushResult(
            sent_batches=0,
            sent_changes=0,
            applied_changes=0,
            latest_change_id=None,
            latest_change_unix_ms=None,
        )

    validate_delta_changes_for_upload(changes)

    batches: List[List[Dict[str, Any]]] = []
    current_batch: List[Dict[str, Any]] = []

    for change in changes:
        candidate_batch = current_batch + [change]
        candidate_payload = {
            "vault_uid": options.vault_uid,
            "device_id": options.device_id,
            "changes": candidate_batch,
        }
        candidate_size = encoded_byte_count(candidate_payload)
        if candidate_size <= options.max_upload_bytes:
            current_batch = candidate_batch
            continue

        if not current_batch:
            raise VaultCtlError(
                "A single file change exceeds --max-upload-bytes "
                f"({options.max_upload_bytes}). Increase limits or split the file."
            )

        batches.append(current_batch)
        current_batch = [change]

        single_payload = {
            "vault_uid": options.vault_uid,
            "device_id": options.device_id,
            "changes": current_batch,
        }
        single_size = encoded_byte_count(single_payload)
        if single_size > options.max_upload_bytes:
            raise VaultCtlError(
                "A single file change exceeds --max-upload-bytes "
                f"({options.max_upload_bytes}). Increase limits or split the file."
            )

    if current_batch:
        batches.append(current_batch)

    applied_changes = 0
    latest_change_id: Optional[int] = None
    latest_change_unix_ms: Optional[int] = None

    for index, batch in enumerate(batches):
        payload = {
            "vault_uid": options.vault_uid,
            "device_id": options.device_id,
            "changes": batch,
        }
        response = api_post(options.base_url, options.api_key, "/vaults/sync/push", payload)
        applied_changes += int(response.get("applied_changes") or 0)
        latest_change_id = response.get("latest_change_id")
        latest_change_unix_ms = response.get("latest_change_unix_ms")
        info(f"Uploaded batch {index + 1}/{len(batches)}: {len(batch)} change(s).")

    return BatchedPushResult(
        sent_batches=len(batches),
        sent_changes=len(changes),
        applied_changes=applied_changes,
        latest_change_id=latest_change_id,
        latest_change_unix_ms=latest_change_unix_ms,
    )


def run_full_push(options: SyncOptions, state: Dict[str, Any]) -> Dict[str, Any]:
    info(f"Starting full-push for vault `{options.vault_uid}` from {options.local_path}")
    info("Scanning local vault files (this can take a while on iCloud/network storage) ...")
    snapshot = scan_local_vault(options.local_path, progress_label="full-push scan")
    info(f"Scanned local vault: {len(snapshot)} file(s) detected.")
    if not snapshot:
        warn(
            "Local vault is empty at "
            f"{options.local_path}. If this vault is in iCloud Drive, verify files are local."
        )

    payload_files: List[Dict[str, Any]] = []
    for path in sorted(snapshot.keys()):
        encoded = base64.b64encode(read_local_file(options.local_path, path)).decode("ascii")
        payload_files.append({"file_path": path, "content_base64": encoded})

    full_payload = {
        "vault_uid": options.vault_uid,
        "device_id": options.device_id,
        "uploaded_at_unix_ms": unix_milliseconds_now(),
        "files": payload_files,
    }
    full_payload_size = encoded_byte_count(full_payload)
    info(f"Prepared full-push payload: {len(payload_files)} file(s), {full_payload_size} bytes.")

    if full_payload_size <= options.max_upload_bytes:
        info("Uploading full snapshot via /vaults/sync/full-push ...")
        response = api_post(options.base_url, options.api_key, "/vaults/sync/full-push", full_payload)
        info(
            "Full push complete: "
            f"{len(payload_files)} file(s), {int(response.get('applied_changes') or 0)} applied change(s)."
        )
        next_state = dict(state)
        next_state["local_snapshot"] = snapshot
        next_state["last_server_change_id"] = response.get("latest_change_id")
        next_state["last_server_change_unix_ms"] = response.get("latest_change_unix_ms")
        return next_state

    warn(
        f"full-push payload is {full_payload_size} bytes (max {options.max_upload_bytes}); "
        "using batched delta-push fallback."
    )

    latest_change_id_from_status: Optional[int] = None
    latest_change_unix_ms_from_status: Optional[int] = None

    try:
        info("Loading remote status for batched full-push fallback ...")
        status = api_post(
            options.base_url,
            options.api_key,
            "/vaults/sync/status",
            {
                "vault_uid": options.vault_uid,
                "since_unix_ms": None,
                "limit": options.limit,
            },
        )
        latest_change_id_from_status = status.get("latest_change_id")
        latest_change_unix_ms_from_status = status.get("latest_change_unix_ms")

        remote_timestamps = status.get("file_timestamps") or []
        if not isinstance(remote_timestamps, list):
            remote_timestamps = []
        info(f"Remote status loaded: {len(remote_timestamps)} indexed file(s).")

        changes = build_delta_changes_using_remote_status(
            local_snapshot=snapshot,
            previous_local_snapshot=state.get("local_snapshot", {}),
            remote_file_timestamps=remote_timestamps,
            local_root=options.local_path,
            force_full_mirror=True,
            prefer_fingerprint_fallback=is_likely_icloud_drive_path(options.local_path),
        )
    except Exception as exc:
        warn(f"Failed to load /vaults/sync/status for full-push fallback, using local state diff: {exc}")
        info("Building local diff from previous sync state ...")
        changes = build_delta_changes(
            previous=state.get("local_snapshot", {}),
            current=snapshot,
            local_root=options.local_path,
            prefer_fingerprint_fallback=is_likely_icloud_drive_path(options.local_path),
        )

    if not changes:
        info("Full push fallback found no differences; nothing to upload.")
        next_state = dict(state)
        next_state["local_snapshot"] = snapshot
        if latest_change_id_from_status is not None:
            next_state["last_server_change_id"] = latest_change_id_from_status
        if latest_change_unix_ms_from_status is not None:
            next_state["last_server_change_unix_ms"] = latest_change_unix_ms_from_status
        return next_state

    push_result = push_delta_changes_in_batches(options, changes)
    info(
        "Full push fallback complete: "
        f"sent {push_result.sent_changes} change(s) in {push_result.sent_batches} batch(es), "
        f"applied {push_result.applied_changes} change(s)."
    )

    next_state = dict(state)
    next_state["local_snapshot"] = snapshot
    next_state["last_server_change_id"] = (
        push_result.latest_change_id
        if push_result.latest_change_id is not None
        else latest_change_id_from_status
    )
    next_state["last_server_change_unix_ms"] = (
        push_result.latest_change_unix_ms
        if push_result.latest_change_unix_ms is not None
        else latest_change_unix_ms_from_status
    )
    return next_state


def run_full_pull(options: SyncOptions, state: Dict[str, Any]) -> Dict[str, Any]:
    response = api_post(
        options.base_url,
        options.api_key,
        "/vaults/sync/full-pull",
        {"vault_uid": options.vault_uid, "limit": options.limit},
    )
    snapshot = response.get("snapshot_files") or []
    if not isinstance(snapshot, list):
        snapshot = []

    apply_full_snapshot(snapshot, options.local_path)
    local_snapshot = scan_local_vault(options.local_path)
    info(f"Full pull complete: {len(snapshot)} file(s) written.")

    next_state = dict(state)
    next_state["local_snapshot"] = local_snapshot
    next_state["last_server_change_id"] = response.get("latest_change_id")
    next_state["last_server_change_unix_ms"] = response.get("latest_change_unix_ms")
    return next_state


def run_delta_push(options: SyncOptions, state: Dict[str, Any]) -> Dict[str, Any]:
    current_snapshot = scan_local_vault(options.local_path)
    if not current_snapshot:
        warn(
            "Local vault is empty at "
            f"{options.local_path}. If this vault is in iCloud Drive, verify files are local."
        )

    latest_change_id_from_status: Optional[int] = None
    latest_change_unix_ms_from_status: Optional[int] = None

    try:
        status = api_post(
            options.base_url,
            options.api_key,
            "/vaults/sync/status",
            {
                "vault_uid": options.vault_uid,
                "since_unix_ms": None,
                "limit": options.limit,
            },
        )
        latest_change_id_from_status = status.get("latest_change_id")
        latest_change_unix_ms_from_status = status.get("latest_change_unix_ms")
        remote_timestamps = status.get("file_timestamps") or []
        if not isinstance(remote_timestamps, list):
            remote_timestamps = []

        changes = build_delta_changes_using_remote_status(
            local_snapshot=current_snapshot,
            previous_local_snapshot=state.get("local_snapshot", {}),
            remote_file_timestamps=remote_timestamps,
            local_root=options.local_path,
            force_full_mirror=False,
            prefer_fingerprint_fallback=is_likely_icloud_drive_path(options.local_path),
        )
        change_log = status.get("change_log")
        change_log_count = len(change_log) if isinstance(change_log, list) else 0
        info(f"Remote status loaded: file_index={len(remote_timestamps)}, changelog={change_log_count}.")
    except Exception as exc:
        warn(f"Failed to load /vaults/sync/status, falling back to local state diff: {exc}")
        changes = build_delta_changes(
            previous=state.get("local_snapshot", {}),
            current=current_snapshot,
            local_root=options.local_path,
            prefer_fingerprint_fallback=is_likely_icloud_drive_path(options.local_path),
        )

    if not changes:
        info("No local changes detected; skipping delta push.")
        next_state = dict(state)
        next_state["local_snapshot"] = current_snapshot
        if latest_change_id_from_status is not None:
            next_state["last_server_change_id"] = latest_change_id_from_status
        if latest_change_unix_ms_from_status is not None:
            next_state["last_server_change_unix_ms"] = latest_change_unix_ms_from_status
        return next_state

    push_result = push_delta_changes_in_batches(options, changes)
    info(
        "Delta push complete: "
        f"sent {push_result.sent_changes} change(s) in {push_result.sent_batches} batch(es), "
        f"applied {push_result.applied_changes} change(s)."
    )

    next_state = dict(state)
    next_state["local_snapshot"] = current_snapshot
    next_state["last_server_change_id"] = push_result.latest_change_id
    next_state["last_server_change_unix_ms"] = push_result.latest_change_unix_ms
    return next_state


def run_delta_pull(options: SyncOptions, state: Dict[str, Any]) -> Dict[str, Any]:
    since = state.get("last_server_change_unix_ms")
    if since is None:
        info("No local server watermark; falling back to full-pull.")
        return run_full_pull(options, state)

    response = api_post(
        options.base_url,
        options.api_key,
        "/vaults/sync/pull",
        {"vault_uid": options.vault_uid, "since_unix_ms": since, "limit": options.limit},
    )

    mode = str(response.get("mode") or "")
    if mode == "delta":
        changed_files = response.get("changed_files") or []
        if not isinstance(changed_files, list):
            changed_files = []
        apply_delta_changes(changed_files, options.local_path)
        info(f"Delta pull complete: {len(changed_files)} change(s) applied.")
    else:
        snapshot = response.get("snapshot_files") or []
        if not isinstance(snapshot, list):
            snapshot = []
        apply_full_snapshot(snapshot, options.local_path)
        info(f"Delta pull returned full snapshot: {len(snapshot)} file(s) written.")

    local_snapshot = scan_local_vault(options.local_path)
    next_state = dict(state)
    next_state["local_snapshot"] = local_snapshot
    next_state["last_server_change_id"] = response.get("latest_change_id")
    next_state["last_server_change_unix_ms"] = response.get("latest_change_unix_ms")
    return next_state


def run_sync(sync_command: str, overrides: ProfileOverrides, interactive: bool) -> int:
    options, selected_profile_name = resolve_sync_options(
        sync_command,
        overrides,
        require_local_path=True,
    )
    prepare_local_vault_directory(options.local_path, sync_command)
    state = load_state(options.state_file, options.vault_uid)

    if selected_profile_name:
        info(f"Using profile `{selected_profile_name}`.")

    if sync_command == "full-push":
        state = run_full_push(options, state)
    elif sync_command == "full-pull":
        state = run_full_pull(options, state)
    elif sync_command == "delta-push":
        state = run_delta_push(options, state)
    elif sync_command == "delta-pull":
        state = run_delta_pull(options, state)
    else:
        raise VaultCtlError(f"Unknown sync command: {sync_command}")

    save_state(state, options.state_file)
    if interactive:
        info(f"State saved: {options.state_file}")
    return 0


def run_status(overrides: ProfileOverrides) -> int:
    options, selected_profile_name = resolve_sync_options(
        "status",
        overrides,
        require_local_path=False,
    )
    if selected_profile_name:
        info(f"Using profile `{selected_profile_name}`.")

    since_value = overrides.since_unix_ms
    if since_value is None and options.state_file.exists():
        state = load_state(options.state_file, options.vault_uid)
        since_state = state.get("last_server_change_unix_ms")
        if isinstance(since_state, int):
            since_value = since_state

    response = api_post(
        options.base_url,
        options.api_key,
        "/vaults/sync/status",
        {"vault_uid": options.vault_uid, "since_unix_ms": since_value, "limit": options.limit},
    )

    file_timestamps = response.get("file_timestamps")
    change_log = response.get("change_log")
    file_count = len(file_timestamps) if isinstance(file_timestamps, list) else 0
    change_count = len(change_log) if isinstance(change_log, list) else 0

    print(f"vault_uid: {response.get('vault_uid')}")
    print(f"latest_change_id: {response.get('latest_change_id')}")
    print(f"latest_change_unix_ms: {response.get('latest_change_unix_ms')}")
    print(f"file_timestamps: {file_count}")
    print(f"change_log: {change_count}")
    return 0


def save_profile(name: str, overrides: ProfileOverrides) -> None:
    normalized_name = normalize_profile_name(name)
    config = load_config()
    profiles: Dict[str, Any] = config.get("profiles", {})
    existing = StoredProfile.from_json(profiles.get(normalized_name, {}))

    merged = merge_profile(existing, overrides)

    if merged.base_url is not None:
        merged.base_url = trimmed_non_empty(merged.base_url)
    if merged.api_key is not None:
        merged.api_key = trimmed_non_empty(merged.api_key)
    if merged.vault_uid is not None:
        merged.vault_uid = trimmed_non_empty(merged.vault_uid)
    if merged.local_path is not None:
        merged.local_path = normalize_path_string(merged.local_path)
    if merged.device_id is not None:
        merged.device_id = trimmed_non_empty(merged.device_id)
    if merged.state_file is not None:
        merged.state_file = normalize_path_string(merged.state_file)

    if merged.is_empty():
        raise VaultCtlError("`profile save` needs at least one value to store")

    profiles[normalized_name] = merged.to_json()
    config["profiles"] = profiles
    if config.get("default_profile") is None or overrides.set_default:
        config["default_profile"] = normalized_name
    save_config(config)
    print(f"Saved profile `{normalized_name}`.")


def use_profile(name: str) -> None:
    normalized_name = normalize_profile_name(name)
    config = load_config()
    profiles: Dict[str, Any] = config.get("profiles", {})
    if normalized_name not in profiles:
        raise VaultCtlError(f"Profile not found: {normalized_name}")
    config["default_profile"] = normalized_name
    save_config(config)
    print(f"Default profile set to `{normalized_name}`.")


def remove_profile(name: str) -> None:
    normalized_name = normalize_profile_name(name)
    config = load_config()
    profiles: Dict[str, Any] = config.get("profiles", {})
    if normalized_name not in profiles:
        raise VaultCtlError(f"Profile not found: {normalized_name}")
    profiles.pop(normalized_name, None)
    config["profiles"] = profiles

    if config.get("default_profile") == normalized_name:
        config["default_profile"] = sorted(profiles.keys())[0] if profiles else None
    save_config(config)
    print(f"Removed profile `{normalized_name}`.")


def resolve_profile_name(input_name: Optional[str], config: Dict[str, Any]) -> str:
    if input_name is not None:
        return normalize_profile_name(input_name)
    default_profile = config.get("default_profile")
    if isinstance(default_profile, str):
        return default_profile
    raise VaultCtlError("No profile specified and no default profile is set")


def run_command(command: Dict[str, Any], interactive: bool) -> int:
    command_type = command["type"]

    if command_type == "help":
        print(usage_text())
        return 0
    if command_type == "config-path":
        print(str(config_file_path()))
        return 0
    if command_type == "profile-list":
        print_profile_list(load_config())
        return 0
    if command_type == "profile-show":
        config = load_config()
        name = resolve_profile_name(command.get("name"), config)
        profile_json = config.get("profiles", {}).get(name)
        if profile_json is None:
            raise VaultCtlError(f"Profile not found: {name}")
        profile = StoredProfile.from_json(profile_json)
        print_profile(name, profile, config.get("default_profile") == name)
        return 0
    if command_type == "profile-save":
        save_profile(command["name"], command["overrides"])
        return 0
    if command_type == "profile-use":
        use_profile(command["name"])
        return 0
    if command_type == "profile-remove":
        remove_profile(command["name"])
        return 0
    if command_type == "sync":
        return run_sync(command["command"], command["overrides"], interactive)
    if command_type == "status":
        return run_status(command["overrides"])

    raise VaultCtlError(f"Unknown command type: {command_type}")


def run_interactive_shell() -> None:
    print("VaultCtl interactive shell (Python)")
    print("Type `help` for commands. Type `quit` or `exit` to leave.")

    while True:
        try:
            line = input("vaultctl> ")
        except EOFError:
            print("")
            break
        except KeyboardInterrupt:
            print("")
            continue

        if not line.strip():
            continue
        if line.lstrip().startswith("#"):
            continue

        lowered = line.strip().lower()
        if lowered in {"quit", "exit"}:
            break

        try:
            tokens = shlex.split(line)
            if not tokens:
                continue
            command = parse_command(["vaultctl"] + tokens)
            exit_code = run_command(command, interactive=True)
            if exit_code != 0:
                eprint(f"[vaultctl] Command exited with status {exit_code}")
        except Exception as exc:
            eprint(f"[vaultctl] ERROR: {exc}")


def usage_text() -> str:
    return """
VaultCtl (Python)

Usage:
  ./client/vaultctl.py
  ./client/vaultctl.py shell
  ./client/vaultctl.py profile save <name> [options]
  ./client/vaultctl.py profile list
  ./client/vaultctl.py profile show [name]
  ./client/vaultctl.py profile use <name>
  ./client/vaultctl.py profile remove <name>
  ./client/vaultctl.py config path
  ./client/vaultctl.py sync <full-push|full-pull|delta-push|delta-pull> [options]
  ./client/vaultctl.py <full-push|full-pull|delta-push|delta-pull> [options]
  ./client/vaultctl.py status [options]

Options:
  --profile <name>           Use a saved profile
  --set-default              Make the saved profile the default
  --base-url <url>           Foundation server URL
  --api-key <key>            Foundation API key
  --vault-uid <uid>          Vault identifier
  --local-path <path>        Local vault path
  --device-id <id>           Device identifier
  --limit <n>                Pull/status limit
  --max-upload-bytes <n>     Per-request upload cap
  --state-file <path>        Override sync state file
  --since-unix-ms <n>        Optional status delta cursor

Storage:
  Default config file: ~/.foundation/vaultctl/config.json
  Override with env: FOUNDATION_VAULTCTL_CONFIG=/path/to/config.json

Interactive shell:
  Run with no arguments to enter the shell loop
  Type `quit` or `exit` to leave

Examples:
  ./client/vaultctl.py profile save archive \\
    --base-url https://foundation.kyooni.kr \\
    --api-key host \\
    --vault-uid archive \\
    --local-path ~/Documents/archive \\
    --set-default

  ./client/vaultctl.py profile list
  ./client/vaultctl.py profile show archive
  ./client/vaultctl.py full-push
  ./client/vaultctl.py sync delta-pull --profile archive
  ./client/vaultctl.py status --profile archive
  ./client/vaultctl.py
  vaultctl> profile list
  vaultctl> delta-push
  vaultctl> quit
""".strip(
        "\n"
    )


def main(arguments: List[str]) -> int:
    try:
        if should_start_interactive_shell(arguments):
            run_interactive_shell()
            return 0

        command = parse_command(arguments)
        return run_command(command, interactive=False)
    except KeyboardInterrupt:
        eprint("[vaultctl] ERROR: Interrupted.")
        return 130
    except Exception as exc:
        eprint(f"[vaultctl] ERROR: {exc}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
