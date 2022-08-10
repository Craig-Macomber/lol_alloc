extern crate alloc;

#[cfg(target_arch = "wasm32")]
use lol_alloc::FreeListAllocator;

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOCATOR: FreeListAllocator = FreeListAllocator::new();

use alloc::boxed::Box;

// Box a `u8`!
#[no_mangle]
pub extern "C" fn hello() -> *mut u8 {
    Box::into_raw(Box::new(42))
}

/// Free a `Box<u8>` that we allocated earlier!
/// # Safety
/// `ptr` must be a pointer from `hello` which is used exactly once.
#[no_mangle]
pub unsafe extern "C" fn goodbye(ptr: *mut u8) {
    let _ = Box::from_raw(ptr);
}
