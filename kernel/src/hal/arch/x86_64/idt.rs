//! Project Zero - Interrupt Descriptor Table (IDT) & CPU Exceptions
//!
//! Provides x86-64 exception handling, register dumps, and fault diagnostics.

use core::arch::asm;
use crate::kprintln;

/// Saved register frame passed from `common_isr_stub` in `boot/isr.asm`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ExceptionFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// 16-byte x86-64 Interrupt Gate Descriptor.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    pub ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

impl IdtEntry {
    pub const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            zero: 0,
        }
    }

    pub fn ist_index(&self) -> u8 {
        self.ist & 0x07
    }

    pub fn is_present(&self) -> bool {
        (self.type_attr & 0x80) != 0
    }


    pub fn set_handler(&mut self, handler: usize, ist_index: u8) {
        self.offset_low = (handler & 0xFFFF) as u16;
        self.selector = super::gdt::KERNEL_CODE_SELECTOR; // Kernel Code Segment Selector
        self.ist = ist_index & 0x07;
        self.type_attr = 0x8E; // Present, DPL 0, 64-bit Interrupt Gate
        self.offset_mid = ((handler >> 16) & 0xFFFF) as u16;
        self.offset_high = ((handler >> 32) & 0xFFFFFFFF) as u32;
        self.zero = 0;
    }

}

/// 10-byte pointer passed to `lidt` instruction.
#[repr(C, packed)]
pub struct IdtrDescriptor {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];

extern "C" {
    fn load_idt(ptr: *const IdtrDescriptor);

    fn isr_0(); fn isr_1(); fn isr_2(); fn isr_3();
    fn isr_4(); fn isr_5(); fn isr_6(); fn isr_7();
    fn isr_8(); fn isr_9(); fn isr_10(); fn isr_11();
    fn isr_12(); fn isr_13(); fn isr_14(); fn isr_15();
    fn isr_16(); fn isr_17(); fn isr_18(); fn isr_19();
    fn isr_20(); fn isr_21(); fn isr_22(); fn isr_23();
    fn isr_24(); fn isr_25(); fn isr_26(); fn isr_27();
    fn isr_28(); fn isr_29(); fn isr_30(); fn isr_31();
    fn isr_32();
    fn isr_251();
    fn isr_252();
    fn isr_253();
    fn isr_255();

    // Controlled test trampolines defined in boot/isr.asm
    pub fn test_trigger_ud();
    pub fn test_resume_ud();
    pub fn test_trigger_de();
    pub fn test_resume_de();
    pub fn test_trigger_pf(addr: u64);
    pub fn test_resume_pf();
    pub fn test_trigger_pf_write(addr: u64);
    pub fn test_resume_pf_write();
    pub fn test_trigger_pf_exec(addr: u64);
    pub fn test_resume_pf_exec();
}

/// Controlled exception testing registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedFault {
    pub vector: u64,
    pub resume_rip: u64,
    pub expected_addr: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultResult {
    pub vector: u64,
    pub error_code: u64,
    pub fault_addr: u64,
    pub fault_rip: u64,
}

static mut EXPECTED_FAULT: Option<ExpectedFault> = None;
static mut LAST_FAULT_RESULT: Option<FaultResult> = None;

pub unsafe fn expect_fault(vector: u64, resume_rip: u64, expected_addr: Option<u64>) {
    EXPECTED_FAULT = Some(ExpectedFault {
        vector,
        resume_rip,
        expected_addr,
    });
    LAST_FAULT_RESULT = None;
}

pub unsafe fn last_fault_result() -> Option<FaultResult> {
    LAST_FAULT_RESULT
}

pub fn get_vector_ist(vector: usize) -> u8 {
    unsafe {
        if vector < 256 {
            IDT[vector].ist_index()
        } else {
            0
        }
    }
}

pub fn get_vector_present(vector: usize) -> bool {
    unsafe {
        if vector < 256 {
            IDT[vector].is_present()
        } else {
            false
        }
    }
}

pub fn get_idt_bounds() -> (u64, u64) {
    unsafe {
        let base = &raw const IDT as u64;
        let size = core::mem::size_of::<[IdtEntry; 256]>() as u64;
        (base, base + size)
    }
}

pub fn init_idt() {

    unsafe {
        let isrs: [unsafe extern "C" fn(); 32] = [
            isr_0, isr_1, isr_2, isr_3, isr_4, isr_5, isr_6, isr_7,
            isr_8, isr_9, isr_10, isr_11, isr_12, isr_13, isr_14, isr_15,
            isr_16, isr_17, isr_18, isr_19, isr_20, isr_21, isr_22, isr_23,
            isr_24, isr_25, isr_26, isr_27, isr_28, isr_29, isr_30, isr_31,
        ];

        for (i, &isr) in isrs.iter().enumerate() {
            // IST 1 is reserved for Double Fault (#DF)
            let ist = if i == 8 { 1 } else { 0 };
            IDT[i].set_handler(isr as usize, ist);
        }

        // Vector 32: Hardware Timer IRQ0
        IDT[32].set_handler(isr_32 as usize, 0);

        // Stage 3N IPI Vectors: 251 (Stop), 252 (Resched), 253 (TLB), 255 (Spurious)
        IDT[251].set_handler(isr_251 as usize, 0);
        IDT[252].set_handler(isr_252 as usize, 0);
        IDT[253].set_handler(isr_253 as usize, 0);
        IDT[255].set_handler(isr_255 as usize, 0);

        load_current_cpu_idt();
    }
}

/// Loads the architectural IDT descriptor on the currently executing CPU.
pub unsafe fn load_current_cpu_idt() {
    let idtr = IdtrDescriptor {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: (&raw const IDT) as u64,
    };
    load_idt(&idtr);
}

pub fn get_exception_name(vector: u64) -> &'static str {
    match vector {
        0 => "Divide Error (#DE)",
        1 => "Debug Exception (#DB)",
        2 => "Non-Maskable Interrupt (NMI)",
        3 => "Breakpoint (#BP)",
        4 => "Overflow (#OF)",
        5 => "Bound Range Exceeded (#BR)",
        6 => "Invalid Opcode (#UD)",
        7 => "Device Not Available (#NM)",
        8 => "Double Fault (#DF)",
        10 => "Invalid TSS (#TS)",
        11 => "Segment Not Present (#NP)",
        12 => "Stack Fault (#SS)",
        13 => "General Protection Fault (#GP)",
        14 => "Page Fault (#PF)",
        16 => "x87 Floating-Point Exception (#MF)",
        17 => "Alignment Check (#AC)",
        18 => "Machine Check (#MC)",
        19 => "SIMD Floating-Point Exception (#XM)",
        20 => "Virtualization Exception (#VE)",
        21 => "Control Protection Exception (#CP)",
        28 => "Hypervisor Injection Exception (#HV)",
        29 => "VMM Communication Exception (#VC)",
        30 => "Security Exception (#SX)",
        _ => "Reserved / Unknown Exception",
    }
}

/// Dispatches CPU hardware exceptions and formats diagnostics.
#[no_mangle]
pub extern "C" fn exception_dispatch(frame_ptr: *mut ExceptionFrame) {
    let frame = unsafe { &mut *frame_ptr };

    // 1. Breakpoint (#BP, Vector 3) is a non-fatal debugging trap
    if frame.vector == 3 {
        kprintln!("[CPU EXCEPTION] Breakpoint (#BP) trapped successfully at RIP: 0x{:016X}", frame.rip);
        kprintln!("  RSP: 0x{:016X}, RFLAGS: 0x{:016X}", frame.rsp, frame.rflags);
        return;
    }

    // 2. Read CR2 for Page Faults
    let mut cr2_val = 0u64;
    if frame.vector == 14 {
        unsafe {
            asm!("mov {}, cr2", out(reg) cr2_val, options(nomem, nostack, preserves_flags));
        }
    }

    // 3. Controlled Test Interception
    unsafe {
        if let Some(expected) = EXPECTED_FAULT {
            if expected.vector == frame.vector {
                let addr_matches = match expected.expected_addr {
                    Some(addr) => (cr2_val & !0xFFF) == (addr & !0xFFF),
                    None => true,
                };

                if addr_matches {
                    kprintln!("[CONTROLLED EXCEPTION TRAP] Vector {} ({}) trapped at RIP: 0x{:016X}", 
                        frame.vector, get_exception_name(frame.vector), frame.rip);
                    if frame.vector == 14 {
                        kprintln!("  CR2 Fault Address: 0x{:016X}", cr2_val);
                        kprintln!("  Error Code:        0x{:016X} ({}{}{})", 
                            frame.error_code,
                            if (frame.error_code & 1) == 0 { "Non-present " } else { "Protection " },
                            if (frame.error_code & 2) != 0 { "Write " } else { "Read " },
                            if (frame.error_code & 4) != 0 { "User" } else { "Kernel" }
                        );
                    }
                    kprintln!("  Safely diverting execution to recovery point at RIP: 0x{:016X}", expected.resume_rip);

                    LAST_FAULT_RESULT = Some(FaultResult {
                        vector: frame.vector,
                        error_code: frame.error_code,
                        fault_addr: cr2_val,
                        fault_rip: frame.rip,
                    });
                    EXPECTED_FAULT = None;

                    // Divert instruction pointer to safe recovery label
                    frame.rip = expected.resume_rip;
                    return;
                }
            }
        }
    }

    // 4. Fatal Hardware Exception (Deterministic Termination)
    kprintln!("\n!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    kprintln!("CPU HARDWARE EXCEPTION TRAP (FATAL)");
    kprintln!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
    kprintln!("  Exception:   {} (Vector {})", get_exception_name(frame.vector), frame.vector);
    kprintln!("  Error Code:  0x{:016X} ({})", frame.error_code, frame.error_code);
    kprintln!("  RIP:         0x{:016X}", frame.rip);
    kprintln!("  CS:          0x{:04X}", frame.cs);
    kprintln!("  RSP:         0x{:016X}", frame.rsp);
    kprintln!("  SS:          0x{:04X}", frame.ss);
    kprintln!("  RFLAGS:      0x{:016X}", frame.rflags);

    if frame.vector == 14 {
        kprintln!("  Fault Address (CR2): 0x{:016X}", cr2_val);
        kprintln!("  Cause:               {}{}{}{}{}",
            if (frame.error_code & 1) == 0 { "Page Not Present; " } else { "Protection Violation; " },
            if (frame.error_code & 2) != 0 { "Write Access; " } else { "Read Access; " },
            if (frame.error_code & 4) != 0 { "User Mode; " } else { "Kernel Mode; " },
            if (frame.error_code & 8) != 0 { "Reserved Bit Overwrite; " } else { "" },
            if (frame.error_code & 16) != 0 { "Instruction Fetch (NX Violation); " } else { "" },
        );
    }

    kprintln!("\n[Register Dump]");
    kprintln!("  RAX: 0x{:016X}  RBX: 0x{:016X}  RCX: 0x{:016X}  RDX: 0x{:016X}", frame.rax, frame.rbx, frame.rcx, frame.rdx);
    kprintln!("  RSI: 0x{:016X}  RDI: 0x{:016X}  RBP: 0x{:016X}  R8:  0x{:016X}", frame.rsi, frame.rdi, frame.rbp, frame.r8);
    kprintln!("  R9:  0x{:016X}  R10: 0x{:016X}  R11: 0x{:016X}  R12: 0x{:016X}", frame.r9, frame.r10, frame.r11, frame.r12);
    kprintln!("  R13: 0x{:016X}  R14: 0x{:016X}  R15: 0x{:016X}", frame.r13, frame.r14, frame.r15);

    kprintln!("\nKernel halted on fatal hardware exception.");
    loop {
        super::cpu::hlt();
    }
}

// ====================================================================
// Stage 2A Controlled Exception Verification API
// ====================================================================

/// Verifies Breakpoint (#BP, Vector 3) trap and recovery.
pub fn verify_breakpoint() -> bool {
    kprintln!("  [Test 1/4] Triggering deliberate Breakpoint (#BP, Vector 3)...");
    unsafe {
        asm!("int3", options(nomem, nostack, preserves_flags));
    }
    kprintln!("  [x] Breakpoint trap handled; execution resumed smoothly.");
    true
}

/// Verifies Invalid Opcode (#UD, Vector 6) controlled interception and recovery.
pub fn verify_invalid_opcode() -> bool {
    kprintln!("  [Test 2/4] Triggering deliberate Invalid Opcode (#UD, Vector 6)...");
    unsafe {
        expect_fault(6, test_resume_ud as usize as u64, None);
        test_trigger_ud();
        let res = last_fault_result();
        match res {
            Some(fault) if fault.vector == 6 => {
                kprintln!("  [x] Invalid Opcode (#UD) intercepted and recovered successfully.");
                true
            }
            _ => {
                kprintln!("  [FAILED] Expected #UD was not captured!");
                false
            }
        }
    }
}

/// Verifies Divide Error (#DE, Vector 0) controlled interception and recovery.
pub fn verify_divide_error() -> bool {
    kprintln!("  [Test 3/4] Triggering deliberate Divide Error (#DE, Vector 0)...");
    unsafe {
        expect_fault(0, test_resume_de as usize as u64, None);
        test_trigger_de();
        let res = last_fault_result();
        match res {
            Some(fault) if fault.vector == 0 => {
                kprintln!("  [x] Divide Error (#DE) intercepted and recovered successfully.");
                true
            }
            _ => {
                kprintln!("  [FAILED] Expected #DE was not captured!");
                false
            }
        }
    }
}

/// Verifies Page Fault (#PF, Vector 14) controlled interception, CR2 validation, and recovery.
pub fn verify_controlled_page_fault(unmapped_addr: u64) -> bool {
    kprintln!("  [Test 4/4] Triggering controlled Page Fault (#PF, Vector 14) at 0x{:016X}...", unmapped_addr);
    unsafe {
        expect_fault(14, test_resume_pf as usize as u64, Some(unmapped_addr));
        test_trigger_pf(unmapped_addr);
        let res = last_fault_result();
        match res {
            Some(fault) if fault.vector == 14 && (fault.fault_addr & !0xFFF) == (unmapped_addr & !0xFFF) => {
                kprintln!("  [x] Page Fault (#PF) trapped with correct CR2; execution resumed smoothly.");
                true
            }
            _ => {
                kprintln!("  [FAILED] Expected #PF for 0x{:016X} was not captured properly!", unmapped_addr);
                false
            }
        }
    }
}

