#!/bin/sh
set -eu
client_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$client_dir/../.." && pwd)
# The patch tool uses the ASM bundled with the project's JDK 8, only at build time.
build="$client_dir/.build"
mkdir -p "$build/classes" "$build/tools" "$build/empty"
javac -source 8 -target 8 -sourcepath "$build/empty" -cp "$root/Client-engine4/ClientV50.jar" -d "$build/classes" "$client_dir/src/ameise/observer/ObservationHook.java"
javac -XDignore.symbol.file -d "$build/tools" "$client_dir/tools/PatchClient.java"
java -cp "$build/tools" PatchClient "$root/Client-engine4/ClientV50.jar" "$build/ClientV50-observer.jar" "$build/classes"
# Only replace a previous successful patch after all preservation checks pass.
cp "$build/ClientV50-observer.jar" "$root/Client-engine4/ClientV50-observer.jar"
