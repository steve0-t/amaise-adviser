#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ ! -f "$root/Client-engine4/ClientV50-observer.jar" ]; then
    "$root/adviser/client/build.sh"
fi
cd "$root"
exec java "-Dameise.observer.enabled=${AMEISE_OBSERVER_ENABLED:-true}" "-Dameise.observer.url=${AMEISE_OBSERVER_URL:-http://127.0.0.1:8765/api/v2/events}" -jar "$root/Client-engine4/ClientV50-observer.jar" "$@"
