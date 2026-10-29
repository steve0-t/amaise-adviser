#!/bin/sh
set -eu
client_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$client_dir/../.." && pwd)
"$client_dir/build.sh"
javac -sourcepath "$client_dir/.build/empty" -cp "$root/Client-engine4/ClientV50-observer.jar" -d "$client_dir/.build/tools" "$client_dir/tools/ObserverTest.java"
java -Xverify:all -cp "$client_dir/.build/tools:$root/Client-engine4/ClientV50-observer.jar" ObserverTest "$client_dir/.build/fixture-events.jsonl"
java -Xverify:all -cp "$client_dir/.build/tools:$root/Client-engine4/ClientV50-observer.jar" ObserverTest disabled
cargo test --locked --manifest-path "$root/adviser/Cargo.toml" --test store generated_java_packets_are_valid -- --ignored
