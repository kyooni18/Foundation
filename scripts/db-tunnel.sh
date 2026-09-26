#!/bin/sh
set -eu

ACTION="${1:-start}"
SSH_TARGET="${FOUNDATION_DOCKER_SSH_TARGET:-dockerd-machine}"
DOCKER_MACHINE="${FOUNDATION_DOCKER_MACHINE:-docker}"
LOCAL_PORT="${FOUNDATION_DB_TUNNEL_PORT:-55432}"
REMOTE_PORT="${FOUNDATION_DB_VM_PORT:-5432}"
SOCKET="${FOUNDATION_DB_TUNNEL_SOCKET:-/tmp/foundation-db-tunnel-$(id -u).sock}"
WAIT_SECONDS="${FOUNDATION_DOCKER_WAIT_SECONDS:-20}"

ssh_direct() {
  ssh \
    -o ControlMaster=no \
    -o ControlPath=none \
    -o BatchMode=yes \
    -o ConnectTimeout=3 \
    "$SSH_TARGET" "$@"
}

remote_docker_ready() {
  ssh_direct 'docker info >/dev/null 2>&1' >/dev/null 2>&1
}

ensure_runtime() {
  if remote_docker_ready; then
    return 0
  fi

  if command -v container >/dev/null 2>&1; then
    if ! container system status >/dev/null 2>&1; then
      echo "Starting Apple Container system..."
      container system start >/dev/null
    fi

    echo "Ensuring Apple Container machine '$DOCKER_MACHINE' is running..."
    container machine run -n "$DOCKER_MACHINE" -- true >/dev/null 2>&1 || {
      echo "Failed to start Apple Container machine '$DOCKER_MACHINE'" >&2
      return 1
    }
  fi

  waited=0
  while ! remote_docker_ready; do
    if [ "$waited" -ge "$WAIT_SECONDS" ]; then
      echo "Remote Docker host '$SSH_TARGET' did not become ready within ${WAIT_SECONDS}s" >&2
      return 1
    fi
    sleep 1
    waited=$((waited + 1))
  done
}

run_compose() {
  ensure_runtime
  export DOCKER_HOST="ssh://$SSH_TARGET"

  if command -v docker-compose >/dev/null 2>&1; then
    docker-compose "$@"
    return
  fi

  if docker compose version >/dev/null 2>&1; then
    docker compose "$@"
    return
  fi

  echo "Docker Compose is not installed (tried docker-compose and docker compose)" >&2
  return 1
}

database_up() {
  run_compose up -d --wait db
}

is_running() {
  ssh -S "$SOCKET" -O check "$SSH_TARGET" >/dev/null 2>&1
}

start_tunnel() {
  database_up

  if is_running; then
    echo "Foundation DB tunnel already running on 127.0.0.1:$LOCAL_PORT"
    return 0
  fi

  rm -f "$SOCKET"
  ssh \
    -M -S "$SOCKET" \
    -fNT \
    -o ExitOnForwardFailure=yes \
    -o ServerAliveInterval=30 \
    -o ServerAliveCountMax=3 \
    -L "127.0.0.1:$LOCAL_PORT:127.0.0.1:$REMOTE_PORT" \
    "$SSH_TARGET"

  if ! is_running; then
    echo "Failed to start Foundation DB tunnel" >&2
    return 1
  fi

  echo "Foundation DB tunnel: 127.0.0.1:$LOCAL_PORT -> $SSH_TARGET:127.0.0.1:$REMOTE_PORT"
}

stop_tunnel() {
  if ! is_running; then
    rm -f "$SOCKET"
    echo "Foundation DB tunnel is not running"
    return 0
  fi

  ssh -S "$SOCKET" -O exit "$SSH_TARGET" >/dev/null
  rm -f "$SOCKET"
  echo "Foundation DB tunnel stopped"
}

case "$ACTION" in
  ensure)
    ensure_runtime
    echo "Foundation Docker runtime is ready at ssh://$SSH_TARGET"
    ;;
  up)
    database_up
    ;;
  down)
    stop_tunnel
    run_compose down
    ;;
  start)
    start_tunnel
    ;;
  stop)
    stop_tunnel
    ;;
  status)
    if remote_docker_ready; then
      echo "Foundation Docker runtime is reachable at ssh://$SSH_TARGET"
    else
      echo "Foundation Docker runtime is not reachable at ssh://$SSH_TARGET"
    fi

    if is_running; then
      echo "Foundation DB tunnel is running on 127.0.0.1:$LOCAL_PORT"
      exit 0
    fi
    echo "Foundation DB tunnel is not running"
    exit 1
    ;;
  compose)
    shift
    if [ "$#" -eq 0 ]; then
      echo "usage: $0 compose <docker-compose arguments...>" >&2
      exit 2
    fi
    run_compose "$@"
    ;;
  *)
    echo "usage: $0 {ensure|up|down|start|stop|status|compose ...}" >&2
    exit 2
    ;;
esac
