#![cfg(target_arch = "wasm32")]

extern crate alloc;

use lol_alloc::ReusingPageAllocator;

#[global_allocator]
static ALLOCATOR: ReusingPageAllocator = ReusingPageAllocator::new();

use alloc::boxed::Box;

// Box a `u8`!
#[no_mangle]
pub extern "C" fn hello() -> *mut u8 {
    Box::into_raw(Box::new(42))
}

// Free a `Box<u8>` that we allocated earlier!
#[no_mangle]
pub unsafe extern "C" fn goodbye(ptr: *mut u8) {
    let _ = Box::from_raw(ptr);
}
