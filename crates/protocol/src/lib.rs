use std::io::{self, Read, Write};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned, de::Visitor};
use std::fmt;
use thiserror::Error;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_SIZE: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortId {
    Subscriber(u8),
    Operator,
    RingGenerator,
    Tap(u8),
}

impl Serialize for PortId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = match self {
            Self::Subscriber(index) if *index < 16 => format!("subscriber_{index}"),
            Self::Operator => "operator".to_string(),
            Self::RingGenerator => "ring_generator".to_string(),
            Self::Tap(index) if (1..=4).contains(index) => format!("tap_{index}"),
            Self::Subscriber(_) => {
                return Err(serde::ser::Error::custom("invalid subscriber port"));
            }
            Self::Tap(_) => return Err(serde::ser::Error::custom("invalid tap port")),
        };
        serializer.serialize_str(&value)
    }
}

impl<'de> Deserialize<'de> for PortId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(PortIdVisitor)
    }
}

struct PortIdVisitor;

impl<'de> Visitor<'de> for PortIdVisitor {
    type Value = PortId;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "a subscriber_0..subscriber_15, operator, ring_generator, or tap_1..tap_4 port string",
        )
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match value {
            "operator" => Ok(PortId::Operator),
            "ring_generator" => Ok(PortId::RingGenerator),
            _ if value.starts_with("subscriber_") => parse_index(value, "subscriber_")
                .filter(|index| *index < 16)
                .map(PortId::Subscriber)
                .ok_or_else(|| {
                    E::custom("subscriber port must be subscriber_0 through subscriber_15")
                }),
            _ if value.starts_with("tap_") => parse_index(value, "tap_")
                .filter(|index| (1..=4).contains(index))
                .map(PortId::Tap)
                .ok_or_else(|| E::custom("tap port must be tap_1 through tap_4")),
            _ => Err(E::custom("invalid port string")),
        }
    }
}

fn parse_index(value: &str, prefix: &str) -> Option<u8> {
    let digits = value.strip_prefix(prefix)?;
    if digits.is_empty() || (digits.len() > 1 && digits.starts_with('0')) {
        return None;
    }
    digits.parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CordConnection {
    pub first: PortId,
    pub second: PortId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HeldControls {
    pub ptt: bool,
    pub police: bool,
    pub ems: bool,
    pub fire: bool,
    pub tap_1: bool,
    pub tap_2: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TuningState {
    pub coarse: u16,
    pub fine: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct InputDebug {
    pub firmware_version: Option<String>,
    pub transport_connected: bool,
    pub device_faults: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct InputState {
    pub cord_topology: Vec<CordConnection>,
    pub held_controls: HeldControls,
    pub directory_digits: [u8; 4],
    pub crank_rotation_timestamps: [u64; 4],
    pub tuning: TuningState,
    pub debug: InputDebug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputMessage {
    pub protocol_version: u16,
    pub input_sequence: u64,
    pub expected_state_revision: u64,
    pub input: InputState,
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
#[serde(deny_unknown_fields)]
pub struct ClockState {
    pub shift: u8,
    pub elapsed_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryPage {
    pub page_number: u8,
    pub heading: String,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ShiftStatus {
    pub number: u8,
    pub phase: ShiftPhase,
    pub active_call_count: u8,
    pub completed_routings: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrinterEntry {
    pub entry_id: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputDebug {
    pub messages: Vec<BackendDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateOutput {
    pub line_lamps: [bool; 16],
    pub game_phase: GamePhase,
    pub clock: ClockState,
    pub speaker_active: bool,
    pub tuning: TuningState,
    pub directory_pages: Vec<DirectoryPage>,
    pub printer_output: Vec<PrinterEntry>,
    pub call: Option<CallStatus>,
    pub shift: ShiftStatus,
    pub debug: OutputDebug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateMessage {
    pub protocol_version: u16,
    pub input_sequence: u64,
    pub accepted: bool,
    pub error: Option<ProtocolError>,
    pub state_revision: u64,
    pub output: StateOutput,
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

    use super::{FrameError, MAX_FRAME_SIZE, PortId, read_frame, write_frame};

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

    #[test]
    fn ports_are_single_wire_strings() {
        for port in [
            PortId::Subscriber(0),
            PortId::Subscriber(15),
            PortId::Operator,
            PortId::RingGenerator,
            PortId::Tap(1),
            PortId::Tap(2),
            PortId::Tap(3),
            PortId::Tap(4),
        ] {
            let bytes = serde_cbor::to_vec(&port).unwrap();
            let decoded: PortId = serde_cbor::from_slice(&bytes).unwrap();
            assert_eq!(decoded, port);
        }

        assert_eq!(
            serde_cbor::to_vec(&PortId::Subscriber(3)).unwrap(),
            serde_cbor::to_vec(&"subscriber_3").unwrap()
        );
        assert!(serde_cbor::from_slice::<PortId>(&serde_cbor::to_vec(&"tap_5").unwrap()).is_err());
    }
}
