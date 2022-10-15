use crate::FreeListAllocator;

use core::alloc::{GlobalAlloc, Layout};

/// A thread safe allocator created by wrapping a (possible not thread-safe) allocator in a spin-lock.
pub struct LockedAllocator<T = FreeListAllocator> {
    spin: spin::Mutex<T>,
}

impl<T> LockedAllocator<T> {
    pub const fn new(t: T) -> Self {
        LockedAllocator {
            spin: spin::Mutex::new(t),
        }
    }
}

unsafe impl<T: GlobalAlloc> GlobalAlloc for LockedAllocator<T> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.spin.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.spin.lock().dealloc(ptr, layout);
    }
}
