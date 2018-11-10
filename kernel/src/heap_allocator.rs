//! Virtual heap allocator.
//!
//! A simple wrapper around linked_list_allocator. We catch the OomError, and
//! try to expand the heap with more pages in that case.
use core::alloc::{GlobalAlloc, Layout, AllocErr};
use sync::{SpinLock, Once};
use core::ops::Deref;
use core::ptr::NonNull;
use linked_list_allocator::{Heap, align_up};
use paging::lands::KernelLand;
use paging::{PAGE_SIZE, MappingFlags, kernel_memory::get_kernel_memory};
use frame_allocator::{FrameAllocator, FrameAllocatorTrait};
use mem::VirtualAddress;

pub struct Allocator(Once<SpinLock<Heap>>);

// 512MB. Should be a multiple of PAGE_SIZE.
const RESERVED_HEAP_SIZE : usize = 512 * 1024 * 1024;

impl Allocator {
    /// Safely expands the heap if possible.
    fn expand(&self, by: usize) {
        let heap = self.0.call_once(Self::init);
        let heap_top = heap.lock().top();
        let heap_bottom = heap.lock().bottom();
        let new_heap_top = align_up(by, PAGE_SIZE) + heap_top; // TODO: Checked add

        assert!(new_heap_top - heap_bottom < RESERVED_HEAP_SIZE, "New heap grows over reserved heap size");

        debug!("EXTEND {:#010x}", new_heap_top);

        for new_page in (heap_top..new_heap_top).step_by(PAGE_SIZE) {
            let frame = FrameAllocator::allocate_frame()
                .expect("Cannot allocate physical memory for heap expansion");
            let mut active_pages = get_kernel_memory();
            active_pages.unmap(VirtualAddress(new_page), PAGE_SIZE);
            active_pages.map_phys_region_to(frame, VirtualAddress(new_page), MappingFlags::WRITABLE);
        }
        unsafe {
            // Safety: We just allocated the area.
            heap.lock().extend(align_up(by, PAGE_SIZE));
        }
    }

    fn init() -> SpinLock<Heap> {
        let mut active_pages = get_kernel_memory();
        // Reserve 512MB of virtual memory for heap space. Don't actually allocate it.
        let heap_space = active_pages.find_virtual_space(RESERVED_HEAP_SIZE)
            .expect("Kernel should have 512MB of virtual memory");
        // map only the first page
        let frame = FrameAllocator::allocate_frame()
            .expect("Cannot allocate first frame of heap");
        active_pages.map_phys_region_to(frame, heap_space, MappingFlags::WRITABLE);
        // guard the rest
        active_pages.guard(heap_space + PAGE_SIZE, RESERVED_HEAP_SIZE - PAGE_SIZE);
        info!("Reserving {} pages at {:#010x}", RESERVED_HEAP_SIZE / PAGE_SIZE - 1, heap_space.addr() + PAGE_SIZE);
        unsafe {
            // Safety: Size is of 0, and the address is freshly guard-paged.
            SpinLock::new(Heap::new(heap_space.addr(), PAGE_SIZE))
        }
    }

    /// Creates a new heap based off of loader settings.
    pub const fn new() -> Allocator {
        Allocator(Once::new())
    }
}

impl Deref for Allocator {
    type Target = SpinLock<Heap>;

    fn deref(&self) -> &SpinLock<Heap> {
        &self.0.call_once(Self::init)
    }
}

unsafe impl<'a> GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // TODO: Race conditions.
        let allocation = self.0.call_once(Self::init).lock().allocate_first_fit(layout);
        let size = layout.size();
        // If the heap is exhausted, then extend and attempt the allocation another time.
        let alloc = match allocation {
            Err(AllocErr) => {
                self.expand(size); // TODO: how much should I *really* expand by?
                self.0.call_once(Self::init).lock().allocate_first_fit(layout)
            }
            _ => allocation
        }.ok().map_or(0 as *mut u8, |allocation| allocation.as_ptr());

        debug!("ALLOC  {:#010x?}, size {:#x}", alloc, layout.size());
        alloc
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        debug!("FREE   {:#010x?}, size {:#x}", ptr, layout.size());
        if cfg!(debug_assertions) {
            let p = ptr as usize;
            for i in p..(p + layout.size()) {
                *(i as *mut u8) = 0x7F;
            }
        }
        self.0.call_once(Self::init).lock().deallocate(NonNull::new(ptr).unwrap(), layout)
    }
}

// required: define how Out Of Memory (OOM) conditions should be handled
// *if* no other crate has already defined `oom`
#[lang = "oom"]
#[no_mangle]
pub fn rust_oom(_: Layout) -> ! {
    panic!("OOM")
}
