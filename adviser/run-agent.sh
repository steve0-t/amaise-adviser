#!/bin/sh
set -eu
adviser_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$adviser_dir"
if [ "${1:-}" != "--help" ] && [ "${1:-}" != "-h" ]; then
    : "${OPENAI_API_KEY:?Set OPENAI_API_KEY to use the OpenAI adapter}"
    : "${OPENAI_MODEL:?Set OPENAI_MODEL to a Responses API model available to your account}"
fi
cargo build --locked --quiet --bin adviser-runner --bin openai-adapter
target_dir=${CARGO_TARGET_DIR:-target}
exec "$target_dir/debug/adviser-runner" "$@" -- "$target_dir/debug/openai-adapter"
