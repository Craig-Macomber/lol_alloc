#![cfg(target_arch = "wasm32")]

use lol_alloc::SimpleAllocator;
use wasm_bindgen_test::*;

#[global_allocator]
static ALLOCATOR: SimpleAllocator = SimpleAllocator::new();

#[wasm_bindgen_test]
fn pass() {
    let x = 1;
    assert_eq!(1, x);
}

#[wasm_bindgen_test]
fn fail() {
    assert_eq!(1, 2);
}
