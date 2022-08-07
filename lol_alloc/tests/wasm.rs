#![cfg(target_arch = "wasm32")]

use lol_alloc::SimpleAllocator;
use wasm_bindgen_test::*;

#[global_allocator]
static ALLOCATOR: SimpleAllocator = SimpleAllocator::new();

#[wasm_bindgen_test]
fn minimal() {
    drop(Box::new(1));
}

#[wasm_bindgen_test]
fn small_allocations() {
    let a = Box::new(1);
    let b = Box::new(2);
    assert_eq!(*a, 1);
    assert_eq!(*b, 2);
}
