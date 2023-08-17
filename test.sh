#!/bin/bash
set -eux -o pipefail

cargo test
wasm-pack test --node lol_alloc
wasm-pack build --release example

wc -c example/pkg/lol_alloc_example_bg.wasm