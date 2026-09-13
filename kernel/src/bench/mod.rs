//! Project Zero - Performance Instrumentation & Microbenchmarks
//!
//! Measures hardware cycle latencies using the x86 RDTSC timestamp counter.

use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use crate::kprintln;
use crate::mm::pmm::{PMM, PhysFrame};
use crate::mm::vmm::{ActivePageTable, Page, VirtualAddress, PageFlags, MappingDomain};
use crate::mm::heap::ALLOCATOR;
use crate::ipc::{IpcEndpoint, IpcMessage};

/// Reads the current 64-bit Time Stamp Counter (RDTSC).
#[inline(always)]
pub fn rdtsc() -> u64 {
    let rax: u64;
    let rdx: u64;
    unsafe {
        asm!("rdtsc", out("rax") rax, out("rdx") rdx, options(nomem, nostack));
    }
    (rdx << 32) | rax
}

pub struct BenchmarkResults {
    pub pmm_alloc_cycles: u64,
    pub pmm_free_cycles: u64,
    pub vmm_map_cycles: u64,
    pub vmm_unmap_cycles: u64,
    pub heap_alloc_cycles: u64,
    pub heap_dealloc_cycles: u64,
    pub ipc_rendezvous_cycles: u64,
}

pub fn run_benchmarks() -> BenchmarkResults {
    // 1. Physical Memory Manager Latency
    let t0 = rdtsc();
    let frame = unsafe { PMM.allocate_frame().expect("PMM alloc bench failed") };
    let t1 = rdtsc();
    let _ = unsafe { PMM.free_frame(frame) };
    let t2 = rdtsc();

    let pmm_alloc_cycles = t1 - t0;
    let pmm_free_cycles = t2 - t1;

    // 2. Virtual Memory Mapping Latency (Map & Unmap at 0x50000000)
    let test_page = Page::from_address(VirtualAddress::new(0x50000000));
    let test_frame = PhysFrame::from_address(0x02000000).expect("bench frame");
    let mut apt = ActivePageTable::new();

    let t3 = rdtsc();
    unsafe {
        apt.map_page(test_page, test_frame, PageFlags::WRITABLE, MappingDomain::Kernel, &mut PMM)
            .expect("VMM map bench failed");
    }
    let t4 = rdtsc();
    unsafe {
        apt.unmap_page(test_page, &mut PMM).expect("VMM unmap bench failed");
    }
    let t5 = rdtsc();

    let vmm_map_cycles = t4 - t3;
    let vmm_unmap_cycles = t5 - t4;

    // 3. Kernel Heap Allocator Latency
    let layout = Layout::from_size_align(128, 16).unwrap();
    let t6 = rdtsc();
    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    let t7 = rdtsc();
    unsafe { ALLOCATOR.dealloc(ptr, layout); }
    let t8 = rdtsc();

    let heap_alloc_cycles = t7 - t6;
    let heap_dealloc_cycles = t8 - t7;

    // 4. Synchronous IPC Rendezvous Latency
    let mut ep = IpcEndpoint::new();
    let msg = IpcMessage::new(1, 0xAA, 10, 20, 30, 40);

    let t9 = rdtsc();
    ep.send_direct(msg);
    let _received = ep.receive();
    let t10 = rdtsc();

    let ipc_rendezvous_cycles = t10 - t9;

    let results = BenchmarkResults {
        pmm_alloc_cycles,
        pmm_free_cycles,
        vmm_map_cycles,
        vmm_unmap_cycles,
        heap_alloc_cycles,
        heap_dealloc_cycles,
        ipc_rendezvous_cycles,
    };

    kprintln!("\n[Stage 2 Subsystem Microbenchmarks (CPU Cycles)]");
    kprintln!("  PMM 4KiB Frame Alloc:     {} cycles", results.pmm_alloc_cycles);
    kprintln!("  PMM 4KiB Frame Free:      {} cycles", results.pmm_free_cycles);
    kprintln!("  VMM Page Map (with TLB):  {} cycles", results.vmm_map_cycles);
    kprintln!("  VMM Page Unmap:           {} cycles", results.vmm_unmap_cycles);
    kprintln!("  Kernel Heap Alloc (128B): {} cycles", results.heap_alloc_cycles);
    kprintln!("  Kernel Heap Dealloc:      {} cycles", results.heap_dealloc_cycles);
    kprintln!("  IPC Direct Rendezvous:    {} cycles", results.ipc_rendezvous_cycles);

    results
}
