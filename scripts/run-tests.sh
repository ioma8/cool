#!/usr/bin/env bash

set -euo pipefail

mode="${1:-host}"

case "$mode" in
  host)
    exec "$(dirname "$0")/run-tests-host.sh"
    ;;
  embedded-fs)
    exec "$(dirname "$0")/run-tests-embedded-fs.sh"
    ;;
  *)
    echo "Usage: $0 [host|embedded-fs]"
    exit 1
    ;;
esac
