//! Project Zero - Kernel Heap Allocator
//!
//! Provides a deterministic block allocator implementing `core::alloc::GlobalAlloc`
//! with memory coalescing, alignment guarantees, and runtime telemetry.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub const KERNEL_HEAP_SIZE: usize = 512 * 1024; // 512 KiB Kernel Heap

#[repr(C, align(16))]
struct BlockHeader {
    size: usize,          // Size of usable memory block (excluding header)
    is_free: bool,        // True if available for allocation
    next: *mut BlockHeader,
}

pub struct KernelHeapAllocator {
    lock: AtomicBool,
    heap_start: AtomicUsize,
    heap_size: usize,
    total_allocated: AtomicUsize,
    alloc_count: AtomicUsize,
    dealloc_count: AtomicUsize,
}

unsafe impl Sync for KernelHeapAllocator {}

static mut HEAP_MEMORY: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

#[global_allocator]
pub static ALLOCATOR: KernelHeapAllocator = KernelHeapAllocator::new();

impl KernelHeapAllocator {
    pub const fn new() -> Self {
        Self {
            lock: AtomicBool::new(false),
            heap_start: AtomicUsize::new(0),
            heap_size: KERNEL_HEAP_SIZE,
            total_allocated: AtomicUsize::new(0),
            alloc_count: AtomicUsize::new(0),
            dealloc_count: AtomicUsize::new(0),
        }
    }

    fn acquire_lock(&self) {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
    }

    fn release_lock(&self) {
        self.lock.store(false, Ordering::Release);
    }

    pub fn init(&self) {
        let ptr = (&raw mut HEAP_MEMORY) as *mut u8 as usize;
        self.heap_start.store(ptr, Ordering::SeqCst);

        // Initialize the single master free block
        let header = ptr as *mut BlockHeader;
        let header_size = core::mem::size_of::<BlockHeader>();
        unsafe {
            (*header).size = KERNEL_HEAP_SIZE - header_size;
            (*header).is_free = true;
            (*header).next = core::ptr::null_mut();
        }
    }

    pub fn stats(&self) -> HeapStats {
        let used = self.total_allocated.load(Ordering::Relaxed);
        HeapStats {
            total_bytes: self.heap_size,
            used_bytes: used,
            free_bytes: self.heap_size.saturating_sub(used),
            allocations: self.alloc_count.load(Ordering::Relaxed),
            deallocations: self.dealloc_count.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HeapStats {
    pub total_bytes: usize,
    pub used_bytes: usize,
    pub free_bytes: usize,
    pub allocations: usize,
    pub deallocations: usize,
}

unsafe impl GlobalAlloc for KernelHeapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.acquire_lock();

        let heap_base = self.heap_start.load(Ordering::Relaxed);
        if heap_base == 0 {
            self.release_lock();
            return core::ptr::null_mut();
        }

        let header_size = core::mem::size_of::<BlockHeader>();
        let align = layout.align().max(16);
        let requested_size = (layout.size() + align - 1) & !(align - 1);

        let mut curr = heap_base as *mut BlockHeader;

        while !curr.is_null() {
            let block = &mut *curr;
            if block.is_free && block.size >= requested_size {
                // Determine if block can be split
                let remainder = block.size.saturating_sub(requested_size + header_size);
                if remainder >= 32 {
                    let next_block_ptr = ((curr as usize) + header_size + requested_size) as *mut BlockHeader;
                    (*next_block_ptr).size = remainder;
                    (*next_block_ptr).is_free = true;
                    (*next_block_ptr).next = block.next;

                    block.size = requested_size;
                    block.next = next_block_ptr;
                }

                block.is_free = false;
                self.total_allocated.fetch_add(block.size, Ordering::Relaxed);
                self.alloc_count.fetch_add(1, Ordering::Relaxed);
                self.release_lock();

                let data_ptr = (curr as usize + header_size) as *mut u8;
                return data_ptr;
            }
            curr = block.next;
        }

        self.release_lock();
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }

        self.acquire_lock();

        let heap_base = self.heap_start.load(Ordering::Relaxed);
        if heap_base == 0 {
            self.release_lock();
            return;
        }

        let header_size = core::mem::size_of::<BlockHeader>();
        let header_ptr = (ptr as usize - header_size) as *mut BlockHeader;
        let block = &mut *header_ptr;

        block.is_free = true;
        self.total_allocated.fetch_sub(block.size, Ordering::Relaxed);
        self.dealloc_count.fetch_add(1, Ordering::Relaxed);

        // Coalesce adjacent free blocks to minimize fragmentation
        let mut curr = heap_base as *mut BlockHeader;
        while !curr.is_null() {
            let curr_block = &mut *curr;
            if curr_block.is_free && !curr_block.next.is_null() {
                let next_block = &mut *curr_block.next;
                if next_block.is_free {
                    curr_block.size += header_size + next_block.size;
                    curr_block.next = next_block.next;
                    continue; // Re-check with the newly merged successor
                }
            }
            curr = curr_block.next;
        }

        self.release_lock();
    }
}
