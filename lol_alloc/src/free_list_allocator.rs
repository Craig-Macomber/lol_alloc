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

const NODE_SIZE: usize = core::mem::size_of::<FreeListNode>();

/// This is an invalid implementation of Sync.
/// SimpleAllocator must not actually be used from multiple threads concurrently.
unsafe impl<T: Sync> Sync for FreeListAllocator<T> {}

unsafe impl<T: MemoryGrower> GlobalAlloc for FreeListAllocator<T> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // This assumes PAGE_SIZE is always a multiple of the required alignment, which should be true for all practical use.
        debug_assert!(PAGE_SIZE % layout.align() == 0);

        let size = full_size(layout);
        let alignment = layout.align().max(NODE_SIZE);
        let mut free_list: *mut *mut FreeListNode = self.free_list.get();
        // search freelist
        loop {
            if *free_list == EMPTY_FREE_LIST {
                break;
            }
            // Try to allocate from end of block of free space.
            let size_of_block = (**free_list).size;
            let start_of_block = *free_list as usize;
            let end_of_block = start_of_block + size_of_block;
            let position = multiple_below(end_of_block - size, alignment);
            if position >= start_of_block {
                // Compute if we need a node after used space due to alignment.
                let end_of_used = position + size;
                if end_of_used < end_of_block {
                    // Insert new block
                    let new_block = end_of_used as *mut FreeListNode;
                    (*new_block).next = (**free_list).next;
                    (*new_block).size = end_of_block - end_of_used;
                    free_list = ptr::addr_of_mut!((*new_block).next);
                }
                if position == start_of_block {
                    // Remove current node from free list.
                    *free_list = (**free_list).next;
                } else {
                    // Shrink free block
                    (**free_list).size = position - start_of_block;
                }

                let ptr = position as *mut u8;
                debug_assert!(ptr.align_offset(NODE_SIZE) == 0);
                debug_assert!(ptr.align_offset(layout.align()) == 0);
                return ptr;
            }

            free_list = ptr::addr_of_mut!((**free_list).next);
        }

        // Failed to find space in the free list.
        // So allocate more space, and allocate from that.
        // Simplest way to due that is grow the heap, and "free" the new space then recurse.
        // This should never need to recurse more than once.

        let requested_bytes = round_up(full_size(layout), PAGE_SIZE);
        let previous_page_count = self
            .grower
            .memory_grow(PageCount(requested_bytes / PAGE_SIZE));
        if previous_page_count == ERROR_PAGE_COUNT {
            return null_mut();
        }

        let ptr = previous_page_count.size_in_bytes() as *mut u8;
        self.dealloc(
            ptr,
            Layout::from_size_align_unchecked(requested_bytes, PAGE_SIZE),
        );
        self.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        debug_assert!(ptr.align_offset(NODE_SIZE) == 0);
        let ptr = ptr as *mut FreeListNode;
        let size = full_size(layout);
        let after_new = offset_bytes(ptr, size); // Used to merge with next node if adjacent.

        let mut free_list: *mut *mut FreeListNode = self.free_list.get();
        // Insert into freelist which is stored in order of descending pointers.
        loop {
            if *free_list == EMPTY_FREE_LIST {
                (*ptr).next = EMPTY_FREE_LIST;
                (*ptr).size = size;
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
    let grown = layout.size().max(NODE_SIZE);
    round_up(grown, NODE_SIZE)
}

// From https://github.com/wackywendell/basicalloc/blob/0ad35d6308f70996f5a29b75381917f4cbfd9aef/src/allocators.rs
// Round up value to the nearest multiple of increment
fn round_up(value: usize, increment: usize) -> usize {
    if value == 0 {
        return 0;
    }
    increment * ((value - 1) / increment + 1)
}

fn multiple_below(value: usize, increment: usize) -> usize {
    increment * (value / increment)
}

unsafe fn offset_bytes(ptr: *mut FreeListNode, offset: usize) -> *mut FreeListNode {
    (ptr as *mut u8).offset(offset as isize) as *mut FreeListNode
}

#[cfg(test)]
mod tests {
    use super::{
        multiple_below, FreeListAllocator, MemoryGrower, PageCount, EMPTY_FREE_LIST, NODE_SIZE,
    };
    use crate::{ERROR_PAGE_COUNT, PAGE_SIZE};
    use alloc::{boxed::Box, vec::Vec};
    use core::{
        alloc::{GlobalAlloc, Layout},
        cell::{RefCell, UnsafeCell},
        ptr,
    };

    struct Allocation {
        layout: Layout,
        ptr: *mut u8,
    }

    #[derive(Clone, Copy)]
    #[repr(C, align(65536))] // align does not appear to with with the PAGE_SIZE constant
    struct Page([u8; PAGE_SIZE]);

    struct Slabby {
        /// Test array of paged, sequential in memory.
        /// Note that resizing this Vec will break all pointers into it.
        pages: Box<[Page]>,
        used_pages: usize,
    }

    impl Slabby {
        fn new() -> Self {
            Slabby {
                pages: vec![Page([0; PAGE_SIZE]); 100].into_boxed_slice(),
                used_pages: 0,
            }
        }
    }

    impl MemoryGrower for RefCell<Slabby> {
        fn memory_grow(&self, delta: PageCount) -> PageCount {
            let mut slabby = self.borrow_mut();
            let old_ptr = ptr::addr_of!(slabby.pages[slabby.used_pages]);
            if slabby.used_pages + delta.0 > slabby.pages.len() {
                return ERROR_PAGE_COUNT;
            }
            slabby.used_pages += delta.0;
            debug_assert!(old_ptr.align_offset(PAGE_SIZE) == 0);
            PageCount(old_ptr as usize / PAGE_SIZE)
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct FreeListContent {
        size: usize,
        /// Offset from beginning of Slabby.
        offset: usize,
    }

    fn free_list_content(allocator: &FreeListAllocator<RefCell<Slabby>>) -> Vec<FreeListContent> {
        let mut out = vec![];
        let grower = allocator.grower.borrow();
        let base = grower.pages.as_ptr() as usize;
        unsafe {
            let mut list = *(allocator.free_list.get());
            while list != EMPTY_FREE_LIST {
                assert_eq!(list.align_offset(NODE_SIZE), 0);
                assert!(list as usize >= base);
                assert!(
                    (list as usize)
                        < ptr::addr_of!(grower.pages[grower.used_pages]) as usize + PAGE_SIZE
                );
                out.push(FreeListContent {
                    size: (*list).size,
                    offset: list as usize - base,
                });
                list = (*list).next;
            }
        }
        out
    }

    #[test]
    fn multiple_below_works() {
        assert_eq!(multiple_below(0, 8), 0);
        assert_eq!(multiple_below(7, 8), 0);
        assert_eq!(multiple_below(8, 8), 8);
        assert_eq!(multiple_below(9, 8), 8);
        assert_eq!(multiple_below(15, 8), 8);
        assert_eq!(multiple_below(16, 8), 16);

        assert_eq!(multiple_below(99, 100), 0);
        assert_eq!(multiple_below(100099, 100), 100000);
    }

    #[test]
    fn it_works() {
        let allocator = FreeListAllocator {
            free_list: UnsafeCell::new(EMPTY_FREE_LIST),
            grower: RefCell::new(Slabby::new()),
        };
        assert_eq!(free_list_content(&allocator), []);
        unsafe {
            let alloc = |size: usize, align: usize| {
                let layout = Layout::from_size_align(size, align).unwrap();
                Allocation {
                    layout,
                    ptr: allocator.alloc(layout),
                }
            };
            let free = |alloc: Allocation| allocator.dealloc(alloc.ptr, alloc.layout);
            let alloc1 = alloc(1, 1);
            assert_eq!(allocator.grower.borrow().used_pages, 1);
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: PAGE_SIZE - NODE_SIZE,
                    offset: 0, // Expect allocation at the end of first page.
                }]
            );
            free(alloc1);
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: PAGE_SIZE,
                    offset: 0,
                }]
            );
        }
    }
}
