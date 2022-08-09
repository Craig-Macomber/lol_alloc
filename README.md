# lol_alloc

A laughably simple wasm global_allocator.

Like [wee_alloc](https://github.com/rustwasm/wee_alloc), but smaller since I used skinnier letters in the name.

`lol_alloc` is a experimental wasm `global_allocator`.

I'm writing `lol_alloc` to learn about allocators (I haven't written one before) and because `wee_alloc` [seems unmaintained](https://github.com/rustwasm/wee_alloc/issues/107) and [has a leak](https://github.com/rustwasm/wee_alloc/issues/106).
After looking at `wee_alloc`'s implementation (which I failed to understand or fix), I wanted to find out how hard it really is to make a wasm global_allocator, and it seems like providing one could be useful to the rust wasm community.

# Plan

I'd like to offer a few minimal allocator implementations targeted specifically as WebAssembly optimized for code size for various levels of requirements.

# Status

Not production ready.

Current a few allocators are provided with minimal testing.
If you use it, please report any bugs: I expect there are some.

Currently none are concurrency safe.

Sizes of allocators include overhead from example:

- `FailAllocator`: 195 bytes: errors on allocations. Operations are O(1),
- `LeakingPageAllocator`: 230 bytes: Allocates pages for each allocation. Operations are O(1).
- `LeakingAllocator`: 356 bytes: Bump pointer allocator, growing the heap as needed. Operations are O(1).
- `FreeListAllocator`: 656 bytes: Free list based allocator. Operations are O(size of free list).

Supports only `wasm32`: other targets may build, but the allocators will not work on them (except: `FailAllocator`, it errors on all platforms just fine).

# Usage

You can replace the `global_allocator` in `wasm32` with `FreeListAllocator` builds using:

```
#[cfg(target_arch = "wasm32")]
use lol_alloc::FreeListAllocator;

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOCATOR: FreeListAllocator = FreeListAllocator::new();
```

# Testing

There are some normal rust unit tests (run with `cargo run test`),
which use a test implementation of `MemoryGrower`.

There are also some [wasm-pack tests](https://rustwasm.github.io/wasm-bindgen/wasm-bindgen-test/usage.html) (run with `wasm-pack test --node lol_alloc`)

Size testing:

```
wasm-pack build --release example && ls -l example/pkg/lol_alloc_example_bg.wasm
```
