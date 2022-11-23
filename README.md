# lol_alloc

A laughably simple wasm global_allocator.

Like [wee_alloc](https://github.com/rustwasm/wee_alloc), but smaller since I used skinnier letters in the name.

`lol_alloc` is a collection of simple wasm `global_allocator`s.

I wrote `lol_alloc` to learn about allocators (I hadn't written one before) and because `wee_alloc` [seems unmaintained](https://github.com/rustwasm/wee_alloc/issues/107) and [has a leak](https://github.com/rustwasm/wee_alloc/issues/106).
After looking at `wee_alloc`'s implementation (which I failed to understand or fix), I wanted to find out how hard it really is to make a wasm global_allocator, and it seemed like providing one could be useful to the rust wasm community.

# Usage

You can replace the `global_allocator` in `LockedAllocator<FreeListAllocator>` in `wasm32` with builds using:

```rust
#[cfg(target_arch = "wasm32")]
use lol_alloc::{FreeListAllocator, LockedAllocator};

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOCATOR: LockedAllocator<FreeListAllocator> = LockedAllocator::new(FreeListAllocator::new());
```

# Soundness

## Use of Pointers

Soundness of the pointer manipulation in this library is currently unclear.
Since [wasm32::memory_grow](https://doc.rust-lang.org/core/arch/wasm32/fn.memory_grow.html)
does not return a pointer there is no "original pointer" so the [Strict Provenance](https://doc.rust-lang.org/std/ptr/index.html#provenance) rules can not be followed.
Attempting to determine if this library's use of pointes at least meets the requirements for being dereferenceable when it dereferences them is similarly challenging as that [is defined as](https://doc.rust-lang.org/std/ptr/index.html#safety):

> dereferenceable: the memory range of the given size starting at the pointer must all be within the bounds of a single allocated object.

The definition of "allocated object" is not clear here.
If the growable wasm heap counts as a single allocated object, then all these allocators are likely ok (in this aspect at least).
However if each call to `wasm32::memory_grow` is considered to create a new allocated object,
then the free list coalescing in `FreeListAllocator` in unsound and could result in undefined behavior.

## Thread Safety

`LeakingAllocator` and `FreeListAllocator` are NOT thread-safe and use unsound implementations of Sync to allow their use as the global allocator. Multithreading is possible in wasm these days: applications that have multiple threads should not use these allocators without wrapping them in `LockedAllocator`.

`FailAllocator`, `LeakingPageAllocator`, and `LockedAllocator` are thread-safe.

# Status

A few projects have apparently used this library, and there have been no reported issues (none reported success either, so use at your own risk).

FreeListAllocator has pretty good test suite, and the rest of the allocators are trivial, and had at least minimal testing.

If you use it, please report any bugs.
If it actually works for you, also let me know (you can post an issue with your report).

Sizes of allocators include overhead from example (compiled with rustc 1.64.0 and wasm-pack 0.10.3):

- `FailAllocator`: 195 bytes: errors on allocations. Operations are O(1).
- `LeakingPageAllocator`: 230 bytes: Allocates pages for each allocation. Operations are O(1).
- `LeakingAllocator`: 356 bytes: Bump pointer allocator, growing the heap as needed and does not reuse/free memory. Operations are O(1).
- `FreeListAllocator`: 656 bytes: Free list based allocator. Operations (both allocation and freeing) are O(length of free list), but it does coalesce adjacent free list nodes.
- `LockedAllocator<FreeListAllocator>`: 777 bytes: FreeListAllocator in a spin lock.

`LeakingAllocator` should have no allocating space overhead other than for alignment.

`FreeListAllocator` rounds allocations up to at least 2 words in size, but otherwise should use all the space. Even gaps from high alignment allocations end up in its free list for use by smaller allocations.

Builtin rust allocator is 5053 bytes in this same test.
If you can afford the extra code size, use it: its a much better allocator.

Supports only `wasm32`: other targets may build, but the allocators will not work on them (except: `FailAllocator`, it errors on all platforms just fine).

# Testing

There are some normal rust unit tests (run with `cargo test`),
which use a test implementation of `MemoryGrower`.

There are also some [wasm-pack tests](https://rustwasm.github.io/wasm-bindgen/wasm-bindgen-test/usage.html) (run with `wasm-pack test --node lol_alloc`)

Size testing:

```bash
wasm-pack build --release example && wc -c example/pkg/lol_alloc_example_bg.wasm
```

# Change log

- 0.2.0: Add `LockedAllocator`.
