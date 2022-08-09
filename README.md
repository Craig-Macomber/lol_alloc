# lol_alloc

Like [wee_alloc](https://github.com/rustwasm/wee_alloc), but smaller since I used skinnier letters in the name.

`lol_alloc` is a experimental wasm `global_allocator`.

I'm writing it because `wee_alloc` (seems unmaintained)[https://github.com/rustwasm/wee_alloc/issues/107], [has a leak](https://github.com/rustwasm/wee_alloc/issues/106), and I was unable to understand its implementation to fix it.
After looking at `wee_alloc`'s implementation, I wanted to find out how hard it really is to make a wasm global_allocator, and it seems like providing one could be useful to the rust wasm community.
Thus I am creating `lol_alloc`, which is intended to be a laughably simple wasm global_allocator.

# Status

No support for freeing, and no test coverage.

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
