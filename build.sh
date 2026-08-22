#!/usr/bin/env sh
set -eu

cargo build --locked --release
mkdir -p bin

if [ -f target/release/sr.exe ]; then
    source_binary=target/release/sr.exe
    destination_binary=bin/sr.exe
elif [ -f target/release/sr ]; then
    source_binary=target/release/sr
    destination_binary=bin/sr
else
    echo "Error: release binary was not produced." >&2
    exit 1
fi

cp "$source_binary" "$destination_binary"

if [ ! -s "$destination_binary" ]; then
    echo "Error: copied release binary is missing or empty." >&2
    exit 1
fi

echo "Built $destination_binary"
