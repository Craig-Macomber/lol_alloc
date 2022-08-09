# lol_alloc

A laughably simple wasm global_allocator.

Like [wee_alloc](https://github.com/rustwasm/wee_alloc), but smaller since I used skinnier letters in the name.

`lol_alloc` is a experimental wasm `global_allocator`.

I'm writing `lol_alloc` to learn about allocators (I havent written one before) and because `wee_alloc` (seems unmaintained)[https://github.com/rustwasm/wee_alloc/issues/107] and [has a leak](https://github.com/rustwasm/wee_alloc/issues/106).
After looking at `wee_alloc`'s implementation (which I faield to understand or fix), I wanted to find out how hard it really is to make a wasm global_allocator, and it seems like providing one could be useful to the rust wasm community.

# Plan

I'd like to offer a few minimal allocator implementations targeted specifically as WebAssembly for various levels of requirements.

Initially I'll be providing the most trivial and small code size allocators practical for apps with really minimal requirements,
including one that just errors on allocations (`FailAllocator`), some minimal leaky allocators (`LeakingPageAllocator` and `LeakingAllocator`),
and at least one allocator that actually frees and reuses memory properly (`FreeListAllocator`).

`FreeListAllocator` will get some unit tests which can run outside WebAssembly using a test `MemoryGrower` which should make testing it straight forward.

# Status

Current a few allocators are provided, but they have almost no testing,
and none currently support freeing.

# Testing

https://rustwasm.github.io/wasm-bindgen/wasm-bindgen-test/usage.html

```
wasm-pack test --node lol_alloc
```

build with:

```
cargo build --target wasm32-unknown-unknown
```

Size testing:

```
wasm-pack build --release example && ls -l example/pkg/lol_alloc_example_bg.wasm
```

Sizes of allocators in bytes (including overhead from example):

FailAllocator: 195
LeakingPageAllocator: 230
LeakingAllocator: 356
FreeListAllocator: 500
