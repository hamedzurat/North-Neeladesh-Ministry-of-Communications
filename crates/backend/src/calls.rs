use std::time::Instant;

use exchange_protocol::CallPhase;

#[derive(Debug, Clone)]
pub(crate) struct ActiveCall {
    pub(crate) caller: u8,
    pub(crate) callee: u8,
    pub(crate) phase: CallPhase,
    pub(crate) deadline: u64,
    pub(crate) started_elapsed_seconds: u64,
    pub(crate) connected_at: Option<Instant>,
    pub(crate) connected_elapsed_seconds: Option<u64>,
    pub(crate) ring_started_at: Option<u64>,
    pub(crate) ring_ready_at: Option<u64>,
    pub(crate) ring_activated: bool,
    pub(crate) disconnected_at: Option<u64>,
    pub(crate) audio_duration_seconds: u64,
}
