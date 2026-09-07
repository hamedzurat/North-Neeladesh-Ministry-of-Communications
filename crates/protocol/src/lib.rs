use std::io::{self, Read, Write};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned, de::Visitor};
use std::fmt;
use thiserror::Error;

pub const PROTOCOL_VERSION: u16 = 1;
pub const DEBUG_PROTOCOL_VERSION: u16 = 2;
pub const MAX_FRAME_SIZE: usize = 4 * 1_048_576;
pub const VOICE_PROTOCOL_VERSION: u16 = 2;
pub const VOICE_INPUT_SAMPLE_RATE: u32 = 16_000;
pub const VOICE_INPUT_AUDIO_PACKET_SAMPLES: usize = 320;
pub const VOICE_AUDIO_SAMPLE_RATE: u32 = 24_000;
pub const VOICE_AUDIO_PAYLOAD_TYPE: u8 = 96;
pub const VOICE_AUDIO_PACKET_SAMPLES: usize = 480;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Police,
    Ems,
    Fire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceCallPhase {
    Active,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceCallStatus {
    pub service: ServiceKind,
    pub phase: ServiceCallPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceErrorKind {
    MissedRequiredServiceCall,
    MisroutedCall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceErrorCount {
    pub kind: ServiceErrorKind,
    pub count: u32,
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
    pub required_service_calls: u32,
    pub completed_service_calls: u32,
    pub service_errors: u32,
    pub service_error_counts: Vec<ServiceErrorCount>,
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
    pub run_generation: u64,
    pub clock: ClockState,
    pub speaker_active: bool,
    pub interference_level: u8,
    pub tap_bridge_audio_active: bool,
    pub tuning: TuningState,
    pub directory_pages: Vec<DirectoryPage>,
    pub printer_output: Vec<PrinterEntry>,
    pub call: Option<CallStatus>,
    pub calls: Vec<CallStatus>,
    pub service_call: Option<ServiceCallStatus>,
    pub tap_bridge_monitoring: Option<u8>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebugCommand {
    Snapshot,
    ResetRun,
    AdvanceTime {
        seconds: u32,
    },
    InjectCall {
        caller_line: u8,
        callee_line: u8,
    },
    ForceStoryEvent {
        event_id: String,
    },
    SelectStoryPath {
        node_id: String,
    },
    SetGodmode {
        enabled: bool,
    },
    SetBypassRestrictions {
        enabled: bool,
    },
    GetVoiceAudio {
        conversation_id: u64,
        kind: DebugAudioKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugAudioKind {
    Capture,
    Tts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugRequest {
    pub protocol_version: u16,
    pub command: DebugCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugResponse {
    pub protocol_version: u16,
    pub accepted: bool,
    pub error: Option<ProtocolError>,
    pub snapshot: DebugSnapshot,
    pub audio: Option<DebugAudio>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugSnapshot {
    pub run: DebugRunState,
    pub shift: ShiftStatus,
    pub calls: Vec<CallStatus>,
    pub subscribers: Vec<DebugSubscriberState>,
    pub story: DebugStoryState,
    pub counters: DebugCounters,
    pub voice: DebugVoiceState,
    pub frontend: DebugFrontendState,
    pub recent_errors: Vec<BackendDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugRunState {
    pub number: u32,
    pub state_revision: u64,
    pub elapsed_seconds: u32,
    pub game_phase: GamePhase,
    pub godmode: bool,
    pub bypass_restrictions: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugSubscriberState {
    pub id: String,
    pub name: String,
    pub line: Option<u8>,
    pub status: String,
    pub availability: String,
    pub pressure: u32,
    pub current_goal: Option<String>,
    pub status_flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugStoryState {
    pub current_node_id: String,
    pub frontier: Vec<String>,
    pub current_story_beat: Option<String>,
    pub interference_reduced: bool,
    pub interference_level: u8,
    pub operator_knowledge: Vec<String>,
    pub graph: Vec<DebugStoryNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugStoryNode {
    pub id: String,
    pub kind: String,
    pub outgoing: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugCounters {
    pub completed_routings: u32,
    pub completed_service_calls: u32,
    pub required_service_calls: u32,
    pub service_errors: u32,
    pub active_call_count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugVoiceState {
    pub status: Option<VoiceStatus>,
    pub speaker_active: bool,
    pub session_id: Option<u64>,
    pub turn_id: Option<u64>,
    pub transcript: Option<String>,
    pub response_text: Option<String>,
    pub conversations: Vec<DebugVoiceConversation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugVoiceConversation {
    pub id: u64,
    pub session_id: u64,
    pub turn_id: u64,
    pub state_revision: u64,
    pub status: Option<VoiceStatus>,
    pub started_elapsed_seconds: u32,
    pub finished_elapsed_seconds: Option<u32>,
    pub captured_samples: u32,
    pub tts_samples: u32,
    pub transcript: Option<String>,
    pub response_text: Option<String>,
    pub error: Option<ProtocolError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugAudio {
    pub sample_rate: u32,
    pub channels: u8,
    pub samples: Vec<i16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugFrontendState {
    pub firmware_version: Option<String>,
    pub transport_connected: bool,
    pub device_faults: Vec<String>,
    pub last_input_json: Option<String>,
    pub last_output_json: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceStatus {
    Ready,
    Listening,
    Transcribing,
    GeneratingResponse,
    Synthesizing,
    Playing,
    Completed,
    Failed,
    Cancelled,
}

impl VoiceStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceStatusMessage {
    pub protocol_version: u16,
    pub session_id: u64,
    pub turn_id: u64,
    pub state_revision: u64,
    pub status: VoiceStatus,
    pub transcript: Option<String>,
    pub response_text: Option<String>,
    pub error: Option<ProtocolError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceControl {
    StartPtt,
    ReleasePtt,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceControlMessage {
    pub protocol_version: u16,
    pub session_id: u64,
    pub turn_id: u64,
    pub state_revision: u64,
    pub voice_id: String,
    pub control: VoiceControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpL16Packet {
    pub marker: bool,
    pub sequence: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub samples: Vec<i16>,
}

#[derive(Debug, Error)]
pub enum VoiceDatagramError {
    #[error("voice datagram is empty")]
    Empty,
    #[error("unsupported voice datagram tag {0}")]
    UnknownTag(u8),
    #[error("voice status CBOR error: {0}")]
    Cbor(#[source] serde_cbor::Error),
    #[error("RTP packet is too short")]
    RtpTooShort,
    #[error("RTP packet has unsupported version {0}")]
    RtpVersion(u8),
    #[error("RTP packet has unsupported payload type {0}")]
    RtpPayloadType(u8),
    #[error("RTP packet has an invalid CSRC or extension layout")]
    RtpLayout,
    #[error("RTP L16 payload must contain an even number of bytes")]
    RtpOddPayload,
    #[error("voice input audio chunk is too large")]
    VoiceInputAudioTooLarge,
}

pub const VOICE_STATUS_TAG: u8 = 0x01;
pub const VOICE_CONTROL_TAG: u8 = 0x02;
pub const VOICE_INPUT_AUDIO_TAG: u8 = 0x03;

pub fn encode_voice_status(message: &VoiceStatusMessage) -> Result<Vec<u8>, serde_cbor::Error> {
    let mut datagram = vec![VOICE_STATUS_TAG];
    datagram.extend(serde_cbor::to_vec(message)?);
    Ok(datagram)
}

pub fn decode_voice_status(datagram: &[u8]) -> Result<VoiceStatusMessage, VoiceDatagramError> {
    if datagram.is_empty() {
        return Err(VoiceDatagramError::Empty);
    }
    if datagram[0] != VOICE_STATUS_TAG {
        return Err(VoiceDatagramError::UnknownTag(datagram[0]));
    }
    serde_cbor::from_slice(&datagram[1..]).map_err(VoiceDatagramError::Cbor)
}

pub fn encode_voice_control(message: &VoiceControlMessage) -> Result<Vec<u8>, serde_cbor::Error> {
    let mut datagram = vec![VOICE_CONTROL_TAG];
    datagram.extend(serde_cbor::to_vec(message)?);
    Ok(datagram)
}

pub fn decode_voice_control(datagram: &[u8]) -> Result<VoiceControlMessage, VoiceDatagramError> {
    if datagram.is_empty() {
        return Err(VoiceDatagramError::Empty);
    }
    if datagram[0] != VOICE_CONTROL_TAG {
        return Err(VoiceDatagramError::UnknownTag(datagram[0]));
    }
    serde_cbor::from_slice(&datagram[1..]).map_err(VoiceDatagramError::Cbor)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceInputAudioMessage {
    pub protocol_version: u16,
    pub session_id: u64,
    pub turn_id: u64,
    pub state_revision: u64,
    pub chunk_index: u32,
    pub complete: bool,
    pub samples: Vec<i16>,
}

pub fn encode_voice_input_audio(
    message: &VoiceInputAudioMessage,
) -> Result<Vec<u8>, serde_cbor::Error> {
    let mut datagram = vec![VOICE_INPUT_AUDIO_TAG];
    datagram.extend(serde_cbor::to_vec(message)?);
    Ok(datagram)
}

pub fn decode_voice_input_audio(
    datagram: &[u8],
) -> Result<VoiceInputAudioMessage, VoiceDatagramError> {
    if datagram.is_empty() {
        return Err(VoiceDatagramError::Empty);
    }
    if datagram[0] != VOICE_INPUT_AUDIO_TAG {
        return Err(VoiceDatagramError::UnknownTag(datagram[0]));
    }
    let message: VoiceInputAudioMessage =
        serde_cbor::from_slice(&datagram[1..]).map_err(VoiceDatagramError::Cbor)?;
    if message.samples.len() > VOICE_INPUT_AUDIO_PACKET_SAMPLES {
        return Err(VoiceDatagramError::VoiceInputAudioTooLarge);
    }
    Ok(message)
}

impl RtpL16Packet {
    pub fn encode(&self) -> Vec<u8> {
        let mut packet = Vec::with_capacity(12 + self.samples.len() * 2);
        packet.push(0x80);
        packet.push((u8::from(self.marker) << 7) | VOICE_AUDIO_PAYLOAD_TYPE);
        packet.extend(self.sequence.to_be_bytes());
        packet.extend(self.timestamp.to_be_bytes());
        packet.extend(self.ssrc.to_be_bytes());
        for sample in &self.samples {
            packet.extend(sample.to_be_bytes());
        }
        packet
    }

    pub fn decode(packet: &[u8]) -> Result<Self, VoiceDatagramError> {
        if packet.len() < 12 {
            return Err(VoiceDatagramError::RtpTooShort);
        }
        let version = packet[0] >> 6;
        if version != 2 {
            return Err(VoiceDatagramError::RtpVersion(version));
        }
        let csrc_count = usize::from(packet[0] & 0x0f);
        if packet[1] & 0x7f != VOICE_AUDIO_PAYLOAD_TYPE {
            return Err(VoiceDatagramError::RtpPayloadType(packet[1] & 0x7f));
        }
        let header_len = 12 + csrc_count * 4;
        if packet.len() < header_len {
            return Err(VoiceDatagramError::RtpLayout);
        }
        let mut payload_offset = header_len;
        if packet[0] & 0x10 != 0 {
            if packet.len() < payload_offset + 4 {
                return Err(VoiceDatagramError::RtpLayout);
            }
            let extension_words = usize::from(u16::from_be_bytes([
                packet[payload_offset + 2],
                packet[payload_offset + 3],
            ]));
            payload_offset += 4 + extension_words * 4;
            if packet.len() < payload_offset {
                return Err(VoiceDatagramError::RtpLayout);
            }
        }
        let mut payload = &packet[payload_offset..];
        if packet[0] & 0x20 != 0 {
            let padding = usize::from(*payload.last().ok_or(VoiceDatagramError::RtpLayout)?);
            if padding == 0 || padding > payload.len() {
                return Err(VoiceDatagramError::RtpLayout);
            }
            payload = &payload[..payload.len() - padding];
        }
        if !payload.len().is_multiple_of(2) {
            return Err(VoiceDatagramError::RtpOddPayload);
        }
        let samples = payload
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| i16::from_be_bytes(*bytes))
            .collect();
        Ok(Self {
            marker: packet[1] & 0x80 != 0,
            sequence: u16::from_be_bytes([packet[2], packet[3]]),
            timestamp: u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]),
            ssrc: u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]),
            samples,
        })
    }
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

    use super::{
        FrameError, MAX_FRAME_SIZE, PortId, RtpL16Packet, VOICE_AUDIO_PAYLOAD_TYPE,
        VOICE_PROTOCOL_VERSION, VoiceControl, VoiceControlMessage, VoiceInputAudioMessage,
        VoiceStatus, VoiceStatusMessage, decode_voice_control, decode_voice_input_audio,
        decode_voice_status, encode_voice_control, encode_voice_input_audio, encode_voice_status,
        read_frame, write_frame,
    };

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

    #[test]
    fn voice_status_is_a_tagged_cbor_datagram() {
        let message = VoiceStatusMessage {
            protocol_version: VOICE_PROTOCOL_VERSION,
            session_id: 12,
            turn_id: 3,
            state_revision: 8,
            status: VoiceStatus::Completed,
            transcript: Some("send the report".to_string()),
            response_text: Some("I will connect you now.".to_string()),
            error: None,
        };

        let encoded = encode_voice_status(&message).unwrap();
        assert_eq!(encoded[0], 1);
        assert_eq!(decode_voice_status(&encoded).unwrap(), message);
    }

    #[test]
    fn voice_control_is_a_separate_tagged_cbor_datagram() {
        let message = VoiceControlMessage {
            protocol_version: VOICE_PROTOCOL_VERSION,
            session_id: 12,
            turn_id: 3,
            state_revision: 8,
            voice_id: "Ryan".to_string(),
            control: VoiceControl::ReleasePtt,
        };

        let encoded = encode_voice_control(&message).unwrap();
        assert_eq!(encoded[0], 2);
        assert_eq!(decode_voice_control(&encoded).unwrap(), message);
    }

    #[test]
    fn voice_input_audio_is_chunked_with_an_explicit_completion_flag() {
        let message = VoiceInputAudioMessage {
            protocol_version: VOICE_PROTOCOL_VERSION,
            session_id: 12,
            turn_id: 3,
            state_revision: 8,
            chunk_index: 0,
            complete: true,
            samples: vec![-2, 0x1234, i16::MAX],
        };

        let encoded = encode_voice_input_audio(&message).unwrap();
        assert_eq!(encoded[0], 3);
        assert_eq!(decode_voice_input_audio(&encoded).unwrap(), message);
    }

    #[test]
    fn rtp_l16_uses_network_order_and_the_project_audio_payload_type() {
        let packet = RtpL16Packet {
            marker: true,
            sequence: 41,
            timestamp: 960,
            ssrc: 0x1234_5678,
            samples: vec![-2, 0x1234, i16::MAX],
        };

        let encoded = packet.encode();
        assert_eq!(encoded[0], 0x80);
        assert_eq!(encoded[1], 0x80 | VOICE_AUDIO_PAYLOAD_TYPE);
        assert_eq!(
            &encoded[12..],
            &[
                (-2_i16).to_be_bytes(),
                0x1234_i16.to_be_bytes(),
                i16::MAX.to_be_bytes()
            ]
            .concat()
        );
        assert_eq!(RtpL16Packet::decode(&encoded).unwrap(), packet);
    }
}
