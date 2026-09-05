use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_SIZE: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendKind {
    Odin,
    Cabinet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontendIdentity {
    pub kind: FrontendKind,
    pub instance_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortId {
    Subscriber(u8),
    Operator,
    RingGenerator,
    Tap(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CordConnection {
    pub first: PortId,
    pub second: PortId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HeldControls {
    pub ptt: bool,
    pub police: bool,
    pub ems: bool,
    pub fire: bool,
    pub tap_listen: [bool; 4],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CrankState {
    pub rotation_count: u32,
    pub speed: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TuningState {
    pub coarse: u16,
    pub fine: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FrontendDiagnostics {
    pub firmware_version: Option<String>,
    pub transport_connected: bool,
    pub device_faults: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputSnapshot {
    pub frontend: FrontendIdentity,
    pub input_sequence: u64,
    pub cord_topology: Vec<CordConnection>,
    pub held_controls: HeldControls,
    pub directory_digits: [u8; 4],
    pub crank: CrankState,
    pub tuning: TuningState,
    pub reset: bool,
    pub diagnostics: FrontendDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    InputSnapshot,
    StateSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputMessage {
    pub protocol_version: u16,
    pub message_kind: MessageKind,
    pub session_id: String,
    pub message_id: u64,
    pub expected_state_revision: u64,
    pub input: InputSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamePhase {
    Ready,
    Shift,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShiftPhase {
    Ready,
    Active,
    Settled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockState {
    pub shift: u8,
    pub elapsed_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryPage {
    pub page_number: u8,
    pub heading: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallStatus {
    pub caller_line: u8,
    pub requested_callee_line: u8,
    pub phase: CallPhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallPhase {
    Waiting,
    OperatorSession,
    AwaitingRouting,
    Held,
    Ringing,
    Connected,
    Completed,
    Missed,
    Misrouted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShiftStatus {
    pub number: u8,
    pub phase: ShiftPhase,
    pub active_call_count: u8,
    pub completed_routings: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrinterEntry {
    pub entry_id: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticState {
    pub frontend: FrontendDiagnostics,
    pub messages: Vec<BackendDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub protocol_version: u16,
    pub frontend: FrontendIdentity,
    pub session_id: String,
    pub input_sequence: u64,
    pub state_revision: u64,
    pub cord_topology: Vec<CordConnection>,
    pub held_controls: HeldControls,
    pub directory_digits: [u8; 4],
    pub crank: CrankState,
    pub tuning: TuningState,
    pub reset_applied: bool,
    pub line_lamps: [bool; 16],
    pub game_phase: GamePhase,
    pub clock: ClockState,
    pub directory_pages: Vec<DirectoryPage>,
    pub printer_output: Vec<PrinterEntry>,
    pub call: Option<CallStatus>,
    pub shift: ShiftStatus,
    pub diagnostics: DiagnosticState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateMessage {
    pub protocol_version: u16,
    pub message_kind: MessageKind,
    pub message_id: u64,
    pub accepted: bool,
    pub error: Option<ProtocolError>,
    pub state: StateSnapshot,
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("I/O error while reading or writing frame: {0}")]
    Io(#[from] io::Error),
    #[error("frame length {0} exceeds the maximum of {MAX_FRAME_SIZE} bytes")]
    Oversized(usize),
    #[error("CBOR error: {0}")]
    Cbor(#[from] serde_cbor::Error),
}

pub fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), FrameError>
where
    W: Write,
    T: Serialize,
{
    let payload = serde_cbor::to_vec(value)?;
    if payload.len() > MAX_FRAME_SIZE {
        return Err(FrameError::Oversized(payload.len()));
    }

    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame<R, T>(reader: &mut R) -> Result<T, FrameError>
where
    R: Read,
    T: DeserializeOwned,
{
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_FRAME_SIZE {
        return Err(FrameError::Oversized(length));
    }

    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    Ok(serde_cbor::from_slice(&payload)?)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{FrameError, MAX_FRAME_SIZE, read_frame, write_frame};

    #[test]
    fn frames_round_trip_with_a_big_endian_length_prefix() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &"complete snapshot").unwrap();

        assert_eq!(&bytes[..4], &[0, 0, 0, 18]);
        let decoded: String = read_frame(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(decoded, "complete snapshot");
    }

    #[test]
    fn oversized_frames_are_rejected_before_writing() {
        let mut bytes = Vec::new();
        let error = write_frame(&mut bytes, &vec![0_u8; MAX_FRAME_SIZE + 1]).unwrap_err();

        assert!(matches!(error, FrameError::Oversized(size) if size > MAX_FRAME_SIZE));
        assert!(bytes.is_empty());
    }
}
