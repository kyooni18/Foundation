#!/bin/sh
set -eu
umask 077
if [ "$1" = init ]; then
  # Refuse partial provisioning rather than generating replacements on startup.
  if [ -e /credentials/registry.json ]; then
    test -s /credentials/user.token && test -s /credentials/agent.token
  else
    primeradiant credential-issue --credentials /credentials/registry.json --token-file /credentials/user.token --kind user
    primeradiant credential-issue --credentials /credentials/registry.json --token-file /credentials/agent.token --kind agent
  fi
  exec primeradiant --db /data/radiant.db --embedding off init
fi
if [ "$1" = serve ]; then
  case "${EMBEDDING_MODE:-off}" in
    off|local) ;;
    *) echo 'EMBEDDING_MODE must be off or local; no cloud embedding fallback is configured.' >&2; exit 1 ;;
  esac
  test -s /credentials/registry.json || { echo 'Run compose run --rm radiant init first' >&2; exit 1; }
  set -- serve --credentials /credentials/registry.json
  if [ "${REMOTE_MCP:-off}" = on ]; then
    test -s /credentials/remote.json || { echo 'Provision /credentials/remote.json (0600) first' >&2; exit 1; }
    python3 -c 'import json,os; c=json.load(open("/credentials/remote.json")); assert c["bind"].rsplit(":",1)[-1] == os.environ.get("MCP_HTTP_PORT","8787"), "remote MCP port mismatch"'
    set -- "$@" --remote-config /credentials/remote.json
  fi

  if [ -n "${EXTRACTION_MODEL:-}" ] || [ -n "${EXTRACTION_REVIEWER:-}" ]; then
    test -n "${EXTRACTION_MODEL:-}" && test -n "${EXTRACTION_REVIEWER:-}"
    test -s /credentials/openrouter-key && test -s /credentials/budget.db
    set -- "$@" --extraction-model "$EXTRACTION_MODEL" --extraction-reviewer "$EXTRACTION_REVIEWER"
    if [ -n "${EXTRACTION_PROMPT_VERSION:-}" ]; then
      set -- "$@" --extraction-prompt-version "$EXTRACTION_PROMPT_VERSION"
    fi
  else
    echo 'Extraction unavailable (not configured); queued Sources are preserved, not published.' >&2
  fi
  echo "Embedding mode: ${EMBEDDING_MODE:-off} (off = explicit lexical/structured degraded retrieval)" >&2
  exec primeradiant --db /data/radiant.db --embedding "${EMBEDDING_MODE:-off}" --embedding-model-dir /model "$@"
fi
exec primeradiant --db /data/radiant.db --embedding off "$@"
