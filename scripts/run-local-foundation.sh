#!/bin/zsh
set -euo pipefail
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
cd "/Users/kyooni18/Code/Foundation"
./scripts/db-tunnel.sh start
exec /opt/homebrew/bin/node dist/index.js
