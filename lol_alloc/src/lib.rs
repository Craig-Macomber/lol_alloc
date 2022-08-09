#![no_std]

use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    ptr::null_mut,
};

/// A number of WebAssembly memory pages.
#[derive(Eq, PartialEq)]
struct PageCount(usize);

impl PageCount {
    fn size_in_bytes(self) -> usize {
        self.0 * PAGE_SIZE
    }
}

/// The WebAssembly page size, in bytes.
const PAGE_SIZE: usize = 65536;

/// Invalid number of pages used to indicate out of memory errors.
const ERROR_PAGE_COUNT: PageCount = PageCount(usize::MAX);

/// Wrapper for core::arch::wasm::memory_grow.
/// Adding this level of indirection allows for improved testing,
/// especially on non wasm platforms.
trait MemoryGrower {
    ///
    fn memory_grow(&self, delta: PageCount) -> PageCount;
}

#[derive(Default)]
pub struct DefaultGrower;

impl MemoryGrower for DefaultGrower {
    #[cfg(target_arch = "wasm32")]
    fn memory_grow(&self, delta: PageCount) -> PageCount {
        PageCount(core::arch::wasm::memory_grow(0, delta.0))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn memory_grow(&self, _delta: PageCount) -> PageCount {
        PageCount(usize::MAX)
    }
}

/// Allocator that fails all allocation.
pub struct FailAllocator;

unsafe impl GlobalAlloc for FailAllocator {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

/// Allocator that allocates whole pages for each allocation.
/// Very wasteful for small allocations.
/// Does not free or reuse memory.
pub struct LeakingPageAllocator;

unsafe impl GlobalAlloc for LeakingPageAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // This assumes PAGE_SIZE is always a multiple of the required alignment, which should be true for all practical use.
        debug_assert!(PAGE_SIZE % layout.align() == 0);

        let requested_pages = (layout.size() + PAGE_SIZE - 1) / PAGE_SIZE;
        let previous_page_count = DefaultGrower.memory_grow(PageCount(requested_pages));
        if previous_page_count == ERROR_PAGE_COUNT {
            return null_mut();
        }

        let ptr = previous_page_count.size_in_bytes() as *mut u8;
        debug_assert!(ptr.align_offset(layout.align()) == 0);
        ptr
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

/// A non-concurrency safe bump-pointer allocator.
/// Does not free or reuse memory.
/// Efficient for small allocations.
/// Does tolerate concurrent callers of wasm32::memory_grow,
/// but not concurrent use of this allocator.
pub struct LeakingAllocator<T = DefaultGrower> {
    used: UnsafeCell<usize>, // bytes
    size: UnsafeCell<usize>, // bytes
    grower: T,
}

/// This is an invalid implementation of Sync.
/// SimpleAllocator must not actually be used from multiple threads concurrently.
unsafe impl Sync for LeakingAllocator {}

impl LeakingAllocator<DefaultGrower> {
    pub const fn new() -> Self {
        LeakingAllocator {
            used: UnsafeCell::new(0),
            size: UnsafeCell::new(0),
            grower: DefaultGrower,
        }
    }
}

unsafe impl<T: MemoryGrower> GlobalAlloc for LeakingAllocator<T> {
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
            let previous_page_count = self.grower.memory_grow(PageCount(requested_pages));
            if previous_page_count == ERROR_PAGE_COUNT {
                return null_mut();
            }

            let previous_size = previous_page_count.size_in_bytes();
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

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

mod free_list_allocator;
pub use crate::free_list_allocator::FreeListAllocator;
