use super::{DefaultGrower, MemoryGrower, PageCount, ERROR_PAGE_COUNT, PAGE_SIZE};
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    ptr::{self, null_mut},
};

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
        // This assumes PAGE_SIZE is always a multiple of the required alignment, which should be true for all practical use.
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
