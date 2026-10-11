#!/bin/sh
# Full logical backup of the Foundation database before risky migrations (schema upgrades, bulk edits).
# Usage: sh scripts/backup-db.sh [output-file]
# Uses a local pg_dump if present, otherwise the pgvector image's pg_dump over the network.
set -eu
URL="${DATABASE_URL:?DATABASE_URL must be set (see .env)}"
OUT="${1:-foundation-backup-$(date +%Y%m%d-%H%M%S).dump}"
if command -v pg_dump >/dev/null 2>&1; then
  pg_dump --format=custom --file="$OUT" "$URL"
else
  docker run --rm --network host -v "$(pwd):/backup" "${FOUNDATION_TEST_DB_IMAGE:-pgvector/pgvector:pg16}" \
    pg_dump --format=custom --file="/backup/$OUT" "$URL"
fi
echo "Backup written to $OUT (contains private memory data; keep it out of Git)"
