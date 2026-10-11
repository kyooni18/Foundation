#!/bin/sh
# Disposable pgvector Postgres for integration tests and benchmarks. Never touches real Foundation data.
set -eu
NAME="${FOUNDATION_TEST_DB_CONTAINER:-foundation-test-pg}"
PORT="${FOUNDATION_TEST_DB_PORT:-55433}"
IMAGE="${FOUNDATION_TEST_DB_IMAGE:-pgvector/pgvector:pg16}"
case "${1:-up}" in
  up)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker run -d --name "$NAME" -e POSTGRES_PASSWORD=test -p "$PORT:5432" "$IMAGE" >/dev/null
    until docker exec "$NAME" pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done
    echo "postgresql://postgres:test@127.0.0.1:$PORT/postgres" ;;
  down) docker rm -f "$NAME" >/dev/null 2>&1 || true ;;
  *) echo "usage: test-db.sh up|down" >&2; exit 2 ;;
esac
