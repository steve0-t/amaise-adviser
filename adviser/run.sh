#!/bin/sh
set -eu
adviser_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec python3 "$adviser_dir/server.py" "$@"
