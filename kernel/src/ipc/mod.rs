//! Project Zero - Inter-Process Communication (IPC) & Kernel Object Subsystem (Stage 3G)
//!
//! Authoritative Contract: Stage 3G Architecture Rev10 (Approved & Frozen).

pub mod types;
pub mod object;
pub mod handle;
pub mod channel;
pub mod shm;
pub mod tests;

pub use types::{rights, signals, IpcError, IpcMessage, ObjectId};
pub use object::{
    allocate_object_id, free_object_slot_locked, KernelObjectHeader, KernelObjectSlot,
    KernelObjectType, ObjectLifecycleState, KERNEL_OBJECT_TABLE, KERNEL_OBJECT_TABLE_LOCK,
    MAX_KERNEL_OBJECTS, NEXT_OBJECT_ID,
};
pub use handle::{
    allocate_handle_entry_locked, close_handle_locked, resolve_current_process_slot,
    validate_handle_locked, Handle, HandleEntry, HandleTable, MAX_HANDLES_PER_PROCESS,
    PROCESS_HANDLE_TABLES,
};
pub use channel::{
    cancel_channel_waiters_for_process_locked, channel_close, channel_create,
    channel_receive, channel_send, check_and_reclaim_channel_locked,
    release_in_flight_pin_locked, Channel, ChannelEndpointData, ChannelRing,
    ThreadIpcState, CHANNEL_RING_CAPACITY, CHANNEL_TABLE, MAX_CHANNELS, THREAD_IPC_STATE,
};
pub use shm::{
    check_and_reclaim_shm_locked, cleanup_process_shm_and_handles_locked, shm_close,
    shm_create, shm_map, shm_unmap, ShmMapping, ShmObject, MAX_SHM_MAPPINGS,
    MAX_SHM_OBJECTS, MAX_SHM_PAGES, SHM_MAPPING_TABLE, SHM_TABLE,
};
pub use tests::run_stage3g_verification;

