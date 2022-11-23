use core::alloc::{GlobalAlloc, Layout};

/// A non thread safe allocator created by wrapping an allocator in `Sync` implementation that assumes all use is from the same thread.
/// Using this (and thus defeating Rust's thread safety checking) is useful due to global allocators having to be stored in statics,
/// which requires `Sync` even in single threaded applications.
pub struct AssumeSingleThreaded<T> {
    inner: T,
}

impl<T> AssumeSingleThreaded<T> {
    /// Converts a potentially non-`Sync` allocator into a `Sync` one by assuming it will only be used by one thread.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the returned value is only accessed by a single thread.
    pub const unsafe fn new(t: T) -> Self {
        AssumeSingleThreaded { inner: t }
    }
}

/// This is an invalid implementation of Sync.
/// AssumeSingleThreaded must not actually be used from multiple threads concurrently.
unsafe impl<T> Sync for AssumeSingleThreaded<T> {}

unsafe impl<T: GlobalAlloc> GlobalAlloc for AssumeSingleThreaded<T> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.inner.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.inner.dealloc(ptr, layout);
    }
}
