#![cfg(target_arch = "wasm32")]

use std::mem::swap;

use lol_alloc::{FreeListAllocator, LockedAllocator};
use wasm_bindgen_test::*;

#[global_allocator]
static ALLOCATOR: LockedAllocator<FreeListAllocator> =
    LockedAllocator::new(FreeListAllocator::new());

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

#[wasm_bindgen_test]
fn many_allocations() {
    let mut v = vec![];
    for i in 0..10000 {
        v.push(Box::new(i));
    }
    for b in &mut v {
        swap(b, &mut Box::new(0));
    }
    v.reserve(1000000);
    drop(v);
}
