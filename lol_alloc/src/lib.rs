#![no_std]

#[macro_use]
extern crate alloc;

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
    /// See core::arch::wasm::memory_grow for semantics.
    fn memory_grow(&self, delta: PageCount) -> PageCount;
}

pub struct DefaultGrower;

impl MemoryGrower for DefaultGrower {
    #[cfg(target_arch = "wasm32")]
    fn memory_grow(&self, delta: PageCount) -> PageCount {
        PageCount(core::arch::wasm::memory_grow(0, delta.0))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn memory_grow(&self, _delta: PageCount) -> PageCount {
        // This MemoryGrower is not actually supported on non-wasm targets.
        // Just return an out of memory error:
        ERROR_PAGE_COUNT
    }
}

mod free_list_allocator;
mod trivial_allocators;
pub use crate::free_list_allocator::FreeListAllocator;
pub use crate::trivial_allocators::{FailAllocator, LeakingAllocator, LeakingPageAllocator};
