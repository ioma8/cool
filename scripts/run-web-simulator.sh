#!/usr/bin/env bash
set -euo pipefail

bash scripts/build-web-simulator.sh

cd dist
python3 -m http.server 8000 &
SERVER_PID=$!

trap "kill $SERVER_PID 2>/dev/null" EXIT

sleep 1
open "http://localhost:8000"

wait $SERVER_PID