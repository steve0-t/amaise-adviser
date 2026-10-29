#!/bin/sh
set -eu
adviser_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec cargo run --locked --quiet --manifest-path "$adviser_dir/Cargo.toml" --bin ameise-adviser -- "$@"
