#![no_std]
#![cfg(target_arch = "wasm32")]

use core::{
    alloc::{GlobalAlloc, Layout},
    arch::wasm32,
    cell::UnsafeCell,
    ptr::null_mut,
};

pub struct SimpleAllocator {
    #[cfg(feature = "sub-page")]
    used: UnsafeCell<usize>, // bytes
    #[cfg(feature = "sub-page")]
    size: UnsafeCell<usize>, // bytes
}

#[cfg(not(feature = "concurrent"))]
unsafe impl Sync for SimpleAllocator {}

impl SimpleAllocator {
    pub const fn new() -> SimpleAllocator {
        #[cfg(feature = "sub-page")]
        {
            SimpleAllocator {
                used: UnsafeCell::new(0),
                size: UnsafeCell::new(0),
            }
        }
        #[cfg(not(feature = "sub-page"))]
        SimpleAllocator {}
    }
}

/// The WebAssembly page size, in bytes.
pub const PAGE_SIZE: usize = 65536;

unsafe impl GlobalAlloc for SimpleAllocator {
    #[cfg(not(feature = "allocation"))]
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        null_mut()
    }

    #[cfg(all(not(feature = "sub-page"), feature = "allocation"))]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let requested_pages = (layout.size() + PAGE_SIZE - 1) / PAGE_SIZE;
        let previous_page_count = wasm32::memory_grow(0, requested_pages);
        if previous_page_count == usize::max_value() {
            return null_mut();
        }

        let ptr = (previous_page_count * PAGE_SIZE) as *mut u8;
        // This assumes PAGE_SIZE is always a multiple of the required alignment, which should be true for all practical use.
        ptr
    }

    #[cfg(feature = "sub-page")]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size: &mut usize = &mut *self.size.get();
        let used: &mut usize = &mut *self.used.get();
        // This assumes PAGE_SIZE is always a multiple of the required alignment, which should be true for all practical use.
        // If this is not true, this could go past size.
        let alignment = layout.align();
        let offset = *used % alignment;
        if offset != 0 {
            *used += alignment - offset;
        }

        let requested_size = layout.size();
        let new_total = *used + requested_size;
        if new_total > *size {
            // Request enough new space for this allocation, even if we have some space left over from the last one incase they end up non-contiguous.
            // Round up to a number of pages
            let requested_pages = (requested_size + PAGE_SIZE - 1) / PAGE_SIZE;
            let previous_page_count = wasm32::memory_grow(0, requested_pages);
            if previous_page_count == usize::max_value() {
                return null_mut();
            }

            let previous_size = previous_page_count * PAGE_SIZE;
            if previous_size != *size {
                // New memory is not contiguous with old: something else allocated in-between.
                // TODO: is handling this case necessary? Maybe make it optional behind a feature?
                // This assumes PAGE_SIZE is always a multiple of the required alignment, which should be true for all practical use.
                *used = previous_size;
                // TODO: in free mode, have minimum alignment used is rounded up to and is maxed with alignment so we can ensure there is either:
                // 1. no space at the end of the page
                // 2. enough space we can add it to the free list
            }
            *size = previous_size + requested_pages * PAGE_SIZE;
        }

        let start = *used;
        *used += requested_size;
        start as *mut u8
    }

    #[cfg(not(feature = "free"))]
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Leak
    }

    #[cfg(feature = "free")]
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // TODO
    }
}
