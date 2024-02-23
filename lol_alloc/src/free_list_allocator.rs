use super::{DefaultGrower, MemoryGrower, PageCount, ERROR_PAGE_COUNT, PAGE_SIZE};
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    ptr::{self, null_mut},
};

/// A non-thread safe allocator that uses a free list.
/// Allocations and frees have runtime O(length of free list).
///
/// The free list is kept sorted by address, and adjacent blocks of memory are coalesced when inserting new blocks.
pub struct FreeListAllocator<T = DefaultGrower> {
    free_list: UnsafeCell<*mut FreeListNode>,
    grower: T,
}

#[cfg(target_arch = "wasm32")]
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
struct FreeListNode {
    next: *mut FreeListNode,
    size: usize,
}

const NODE_SIZE: usize = core::mem::size_of::<FreeListNode>();

// Safety: No one besides us has the raw pointer, so we can safely transfer the
// FreeListAllocator to another thread.
unsafe impl<T> Send for FreeListAllocator<T> {}

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
            if size < end_of_block {
                let position = multiple_below(end_of_block - size, alignment);
                if position >= start_of_block {
                    // Compute if we need a node after used space due to alignment.
                    let end_of_used = position + size;
                    if end_of_used < end_of_block {
                        // Insert new block
                        let new_block = end_of_used as *mut FreeListNode;
                        (*new_block).next = *free_list;
                        (*new_block).size = end_of_block - end_of_used;
                        *free_list = new_block;
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
            }

            free_list = ptr::addr_of_mut!((**free_list).next);
        }

        // Failed to find space in the free list.
        // So allocate more space, and allocate from that.
        // Simplest way to due that is grow the heap, and "free" the new space then recurse.
        // This should never need to recurse more than once.

        let requested_bytes = round_up(size, PAGE_SIZE);
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
                let next = (**free_list).next;
                if next != EMPTY_FREE_LIST && offset_bytes(next, (*next).size) == ptr {
                    // Merge into node before this one, as well as after it.
                    (*next).size += new_size;
                    // Sine we are combining 2 existing nodes (with the new one in-between)
                    // remove one from the list.
                    *free_list = next;
                    return;
                }
                // Edit node in free list, moving its location and updating its size.
                *free_list = ptr;
                (*ptr).size = new_size;
                (*ptr).next = next;
                return;
            }

            if *free_list < ptr {
                // Merge onto end of current if adjacent
                if offset_bytes(*free_list, (**free_list).size) == ptr {
                    // Merge into node before this one, as well as after it.
                    (**free_list).size += size;
                    // Sine we are combining the new node into the end of an existing node, no pointer updates, just a size change.
                    return;
                }
                // Create a new free list node
                (*ptr).next = *free_list;
                (*ptr).size = size;
                *free_list = ptr;
                return;
            }
            free_list = ptr::addr_of_mut!((**free_list).next);
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
    debug_assert!(increment.is_power_of_two());
    (value + (increment - 1)) & increment.wrapping_neg()
}

fn multiple_below(value: usize, increment: usize) -> usize {
    debug_assert!(increment.is_power_of_two());
    value & increment.wrapping_neg()
}

unsafe fn offset_bytes(ptr: *mut FreeListNode, offset: usize) -> *mut FreeListNode {
    (ptr as *mut u8).add(offset) as *mut FreeListNode
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
    #[repr(C, align(65536))] // align does not appear to work with the PAGE_SIZE constant
    struct Page([u8; PAGE_SIZE]);

    struct Slabby {
        /// Test array of pages, sequential in memory.
        pages: Box<[Page]>,
        used_pages: usize,
    }

    impl Slabby {
        fn new() -> Self {
            Slabby {
                pages: vec![Page([0; PAGE_SIZE]); 1000].into_boxed_slice(),
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

    /// Enumerate and validate free list content
    fn free_list_content(allocator: &FreeListAllocator<RefCell<Slabby>>) -> Vec<FreeListContent> {
        let mut out: Vec<FreeListContent> = vec![];
        let grower = allocator.grower.borrow();
        let base = grower.pages.as_ptr() as usize;
        unsafe {
            let mut list = *(allocator.free_list.get());
            while list != EMPTY_FREE_LIST {
                debug_assert_eq!(list.align_offset(NODE_SIZE), 0);
                debug_assert!(list as usize >= base);
                debug_assert!(
                    (list as usize)
                        < ptr::addr_of!(grower.pages[grower.used_pages]) as usize + PAGE_SIZE
                );
                let offset = list as usize - base;
                let size = (*list).size;
                debug_assert!(offset + size <= grower.used_pages * PAGE_SIZE);
                debug_assert!(size >= NODE_SIZE);
                match out.last() {
                    Some(previous) => {
                        debug_assert!(
                            previous.offset > offset + size,
                            "Free list nodes should not overlap or be adjacent"
                        );
                    }
                    None => {}
                }
                out.push(FreeListContent { size, offset });
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

        assert_eq!(multiple_below(127, 128), 0);
        assert_eq!(multiple_below(100223, 128), 100096);
    }

    /// Test performing frees populates the free list, correctly coalescing adjacent pages.
    #[test]
    fn populates_free_list() {
        let allocator = FreeListAllocator {
            free_list: UnsafeCell::new(EMPTY_FREE_LIST),
            grower: RefCell::new(Slabby::new()),
        };
        allocator.grower.borrow_mut().used_pages = 1; // Fake used pages large enough to we don't fail free list validation.
        assert_eq!(free_list_content(&allocator), []);
        unsafe {
            let free = |alloc: FreeListContent| {
                allocator.dealloc(
                    (allocator.grower.borrow().pages.as_ptr() as *mut u8).add(alloc.offset),
                    Layout::from_size_align(alloc.size, 1).unwrap(),
                )
            };
            assert_eq!(free_list_content(&allocator), []);

            free(FreeListContent {
                size: NODE_SIZE,
                offset: NODE_SIZE * 3,
            });
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: NODE_SIZE,
                    offset: NODE_SIZE * 3,
                }]
            );

            // Free before, not contiguous
            free(FreeListContent {
                size: NODE_SIZE,
                offset: NODE_SIZE,
            });
            assert_eq!(
                free_list_content(&allocator),
                [
                    FreeListContent {
                        size: NODE_SIZE,
                        offset: NODE_SIZE * 3,
                    },
                    FreeListContent {
                        size: NODE_SIZE,
                        offset: NODE_SIZE,
                    }
                ]
            );

            // Free before, contiguous
            free(FreeListContent {
                size: NODE_SIZE,
                offset: 0,
            });
            assert_eq!(
                free_list_content(&allocator),
                [
                    FreeListContent {
                        size: NODE_SIZE,
                        offset: NODE_SIZE * 3,
                    },
                    FreeListContent {
                        size: NODE_SIZE * 2,
                        offset: 0,
                    }
                ]
            );

            // Free between, contiguous
            free(FreeListContent {
                size: NODE_SIZE,
                offset: NODE_SIZE * 2,
            });
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: NODE_SIZE * 4,
                    offset: 0,
                },]
            );

            // Free after, contiguous
            free(FreeListContent {
                size: NODE_SIZE,
                offset: NODE_SIZE * 4,
            });
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: NODE_SIZE * 5,
                    offset: 0,
                },]
            );

            // Free after, not contiguous
            free(FreeListContent {
                size: NODE_SIZE,
                offset: NODE_SIZE * 6,
            });
            assert_eq!(
                free_list_content(&allocator),
                [
                    FreeListContent {
                        size: NODE_SIZE,
                        offset: NODE_SIZE * 6,
                    },
                    FreeListContent {
                        size: NODE_SIZE * 5,
                        offset: 0,
                    }
                ]
            );
        }
    }

    #[test]
    fn it_works() {
        let allocator = FreeListAllocator {
            free_list: UnsafeCell::new(EMPTY_FREE_LIST),
            grower: RefCell::new(Slabby::new()),
        };
        assert_eq!(free_list_content(&allocator), []);
        unsafe {
            let allocate = |size: usize, align: usize| {
                let layout = Layout::from_size_align(size, align).unwrap();
                Allocation {
                    layout,
                    ptr: allocator.alloc(layout),
                }
            };
            let free = |alloc: Allocation| allocator.dealloc(alloc.ptr, alloc.layout);
            let alloc = allocate(1, 1);
            assert_eq!(allocator.grower.borrow().used_pages, 1);
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: PAGE_SIZE - NODE_SIZE,
                    offset: 0, // Expect allocation at the end of first page.
                }]
            );
            // Merge into end of existing chunk
            free(alloc);
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: PAGE_SIZE,
                    offset: 0,
                }]
            );

            // Allocate small value to impact alignment
            let alloc = allocate(1, 1);
            // Allocate larger aligned value to cause a hole after it
            let alloc_big = allocate(NODE_SIZE * 2, NODE_SIZE * 2);
            assert_eq!(
                free_list_content(&allocator),
                [
                    FreeListContent {
                        size: NODE_SIZE,
                        offset: PAGE_SIZE - NODE_SIZE * 2,
                    },
                    FreeListContent {
                        size: PAGE_SIZE - NODE_SIZE * 4,
                        offset: 0,
                    },
                ]
            );

            // Free second allocation, causing 3 way join
            free(alloc_big);
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: PAGE_SIZE - NODE_SIZE,
                    offset: 0,
                }]
            );

            // Multi-page allocation
            assert_eq!(allocator.grower.borrow().used_pages, 1);
            let multi_page = allocate(PAGE_SIZE + 1, 1);
            assert_eq!(allocator.grower.borrow().used_pages, 3);
            assert_eq!(
                free_list_content(&allocator),
                [
                    FreeListContent {
                        size: PAGE_SIZE - NODE_SIZE,
                        offset: PAGE_SIZE,
                    },
                    FreeListContent {
                        size: PAGE_SIZE - NODE_SIZE,
                        offset: 0,
                    }
                ]
            );

            // Free everything
            free(alloc);
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: PAGE_SIZE * 2 - NODE_SIZE,
                    offset: 0,
                }]
            );
            free(multi_page);
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: PAGE_SIZE * 3,
                    offset: 0,
                }]
            );
        }
    }

    #[test]
    fn fuzz() {
        use rand::Rng;
        use rand_core::SeedableRng;
        use rand_pcg::Pcg32;

        let mut rng = Pcg32::seed_from_u64(0);

        for _ in 0..100 {
            let allocator = FreeListAllocator {
                free_list: UnsafeCell::new(EMPTY_FREE_LIST),
                grower: RefCell::new(Slabby::new()),
            };

            let allocate = |size: usize, align: usize| {
                let layout = Layout::from_size_align(size, align).unwrap();
                let ptr = unsafe { allocator.alloc(layout) };
                assert!(!ptr.is_null(), "Slab Full");
                Allocation { layout, ptr }
            };
            let free = |alloc: Allocation| unsafe { allocator.dealloc(alloc.ptr, alloc.layout) };

            let mut allocations = vec![];
            for _ in 0..5000 {
                // Randomly free some allocations.
                while !allocations.is_empty() {
                    if rng.gen_bool(0.45) {
                        let alloc = allocations.swap_remove(rng.gen_range(0..allocations.len()));
                        free(alloc);
                    } else {
                        break;
                    }
                }
                // Do a random small allocation
                let size = rng.gen_range(1..100);
                allocations.push(allocate(size, 1 << rng.gen_range(0..7)));
                if rng.gen_bool(0.05) {
                    // Do a random large allocation
                    let size = rng.gen_range(1..(PAGE_SIZE * 10));
                    allocations.push(allocate(size, 1 << rng.gen_range(0..16)));
                }
            }
            free_list_content(&allocator);
            while !allocations.is_empty() {
                let alloc = allocations.swap_remove(rng.gen_range(0..allocations.len()));
                free(alloc);
            }
            assert_eq!(
                free_list_content(&allocator),
                [FreeListContent {
                    size: allocator.grower.borrow().used_pages * PAGE_SIZE,
                    offset: 0,
                }]
            );
        }
    }
}
