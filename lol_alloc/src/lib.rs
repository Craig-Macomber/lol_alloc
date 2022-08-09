#![no_std]

use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    ptr::{self, null_mut},
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
    fn memory_grow(&self, delta: PageCount) -> PageCount {
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
        let requested_pages = (layout.size() + PAGE_SIZE - 1) / PAGE_SIZE;
        let previous_page_count = DefaultGrower.memory_grow(PageCount(requested_pages));
        if previous_page_count == ERROR_PAGE_COUNT {
            return null_mut();
        }

        let ptr = previous_page_count.size_in_bytes() as *mut u8;
        // This assumes PAGE_SIZE is always a multiple of the required alignment, which should be true for all practical use.
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

/// A non-concurrency safe allocator that allocates whole pages for each allocation.
/// Very wasteful for small allocations.
pub struct FreeListAllocator<T = DefaultGrower> {
    free_list: UnsafeCell<*mut FreeListNode>,
    grower: T,
}

impl FreeListAllocator<DefaultGrower> {
    pub const fn new() -> Self {
        FreeListAllocator {
            // Use a special value for empty, which is never valid otherwise.
            free_list: UnsafeCell::new(EMPTY_FREE_LIST),
            grower: DefaultGrower,
        }
    }
}

const EMPTY_FREE_LIST: *mut FreeListNode = usize::MAX as *mut FreeListNode;

/// Stored at the beginning of each free segment.
/// Note: It would be possible to fit this in 1 word (use the low bit to flag that case,
/// then only use a second word if the allocation has size greater than 1 word)
#[repr(C, align(4))]
struct FreeListNode {
    next: *mut FreeListNode,
    size: usize,
}

/// This is an invalid implementation of Sync.
/// SimpleAllocator must not actually be used from multiple threads concurrently.
unsafe impl<T: Sync> Sync for FreeListAllocator<T> {}

unsafe impl<T: MemoryGrower> GlobalAlloc for FreeListAllocator<T> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        debug_assert!(PAGE_SIZE % layout.align() == 0);

        let size = full_size(layout);
        let mut free_list: *mut *mut FreeListNode = self.free_list.get();
        // search freelist
        loop {
            if *free_list == EMPTY_FREE_LIST {
                break;
            }
            // if (**free_list).size > size {
            //     (*ptr).next = *free_list;
            //     *free_list = ptr;
            //     // TODO: Merge with next and/or previous if possible
            //     return;
            // }
            // if (**free_list).size > size {
            //     (*ptr).next = *free_list;
            //     *free_list = ptr;
            //     // TODO: Merge with next and/or previous if possible
            //     return;
            // }

            // free_list = ptr::addr_of_mut!((**free_list).next);
            // if *free_list < ptr {
            //     (*ptr).next = *free_list;
            //     *free_list = ptr;
            //     // TODO: Merge with next and/or previous if possible
            //     return;
            // }
        }

        let requested_pages = (full_size(layout) + PAGE_SIZE - 1) / PAGE_SIZE;
        let previous_page_count = self.grower.memory_grow(PageCount(requested_pages));
        if previous_page_count == ERROR_PAGE_COUNT {
            return null_mut();
        }

        let ptr = previous_page_count.size_in_bytes() as *mut u8;
        debug_assert!(ptr.align_offset(core::mem::size_of::<FreeListNode>()) == 0);
        debug_assert!(ptr.align_offset(layout.align()) == 0);
        // This assumes PAGE_SIZE is always a multiple of the required alignment, which should be true for all practical use.
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        debug_assert!(ptr.align_offset(core::mem::size_of::<FreeListNode>()) == 0);
        let ptr = ptr as *mut FreeListNode;
        let size = full_size(layout);
        let after_new = offset_bytes(ptr, size); // Used to merge with next node if adjacent.

        let mut free_list: *mut *mut FreeListNode = self.free_list.get();
        // Insert into freelist which is stored in order of descending pointers.
        loop {
            if *free_list == EMPTY_FREE_LIST {
                (*ptr).next = EMPTY_FREE_LIST;
                *free_list = ptr;
                return;
            }

            if *free_list == after_new {
                // Merge new node into node after this one.

                let new_size = size + (**free_list).size;
                if offset_bytes((*ptr).next, (*(*ptr).next).size) == ptr {
                    // Merge into node before this one, as well as after it.
                    (*(*ptr).next).size += new_size;
                    // Sine we are combining 2 existing nodes (with the new one in-between)
                    // remove one from the list.
                    *free_list = (**free_list).next;
                    return;
                }

                *free_list = ptr;
                (*ptr).size = new_size;
                (*ptr).next = (**free_list).next;
                return;
            }

            let next_free_list = ptr::addr_of_mut!((**free_list).next);
            if *next_free_list < ptr {
                // TODO: Merge with next and/or previous if possible
                if offset_bytes(*next_free_list, (*(*next_free_list)).size) == ptr {
                    // Merge into node before this one, as well as after it.
                    (*(*ptr).next).size += size;
                    // Sine we are combining the new node into the end of an existing node, no pointer updates, just a size change.
                    return;
                }
                // Create a new free list node
                (*ptr).next = *next_free_list;
                (*ptr).size = size;
                *next_free_list = ptr;
                return;
            }
            free_list = next_free_list;
        }
    }
}

fn full_size(layout: Layout) -> usize {
    let grown = layout.size().max(core::mem::size_of::<FreeListNode>());
    round_up(grown, core::mem::size_of::<FreeListNode>())
}

// From https://github.com/wackywendell/basicalloc/blob/0ad35d6308f70996f5a29b75381917f4cbfd9aef/src/allocators.rs
// Round up value to the nearest multiple of increment
fn round_up(value: usize, increment: usize) -> usize {
    if value == 0 {
        return 0;
    }
    increment * ((value - 1) / increment + 1)
}

unsafe fn offset_bytes(ptr: *mut FreeListNode, offset: usize) -> *mut FreeListNode {
    (ptr as *mut u8).offset(offset as isize) as *mut FreeListNode
}
