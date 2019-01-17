//! Heap allocator.
//!
//! A wrapper to `linked_list_allocator` that uses the svcSetHeapSize syscall
//! to expand its memory when needed.

use core::alloc::{GlobalAlloc, Layout};
use spin::{Mutex, MutexGuard};
use core::ptr::NonNull;
use linked_list_allocator::{Heap, align_up};
use crate::syscalls::set_heap_size;
use crate::error::KernelError;

/// The libuser heap allocator.
///
/// A wrapper to `linked_list_allocator` that uses the svcSetHeapSize syscall
/// to expand its memory when needed.
pub struct Allocator(Mutex<Heap>);

impl Allocator {
    /// Safely expands the heap if possible.
    fn expand(heap: &mut MutexGuard<'_, Heap>, by: usize) -> Result<(), KernelError> {
        let total = heap.size() + align_up(by, 0x200_000); // set_heap_size requires this alignment.

        let heap_bottom = unsafe { set_heap_size(total)? };

        if heap.bottom() == 0 {
            unsafe { **heap = Heap::new(heap_bottom, total) };
        } else {
            unsafe { heap.extend(total) };
        }
        Ok(())
    }

    /// Creates an empty heap.
    pub const fn new() -> Allocator {
        Allocator(Mutex::new(Heap::empty()))
    }
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut heap = self.0.lock();
        let allocation_result = heap.allocate_first_fit(layout);
        // If the heap is exhausted, then extend and attempt the allocation once again.
        match allocation_result {
            Err(_) => {
                if let Ok(_) = Self::expand(&mut heap, layout.size()) {
                    heap.allocate_first_fit(layout)
                } else {
                    // Return the original failed allocation if we can't expand.
                    allocation_result
                }
            }
            Ok(_) => allocation_result
        }.ok().map_or(0 as *mut u8, |allocation| allocation.as_ptr())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.lock().deallocate(NonNull::new(ptr).unwrap(), layout)
    }
}