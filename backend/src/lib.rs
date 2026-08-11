//! Deliberately small, offline authority for the Cabinet Frontend MVP.
//! The newline-delimited JSON protocol is disposable: it exists only to make
//! the Odin/Rust boundary inspectable during the MVP demonstration.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc::{self, Receiver},
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SHIFT_START_MINUTES: u16 = 9 * 60;
const SHIFT_END_MINUTES: u16 = 17 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    IncomingCaller,
    ConnectedToOperator,
    OperatorResponding,
    RecoverableSystemFailure,
    AwaitingRouting,
    CalleeRinging,
    CircuitConnected { tapped: bool },
    DemonstrationComplete,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::IncomingCaller => "INCOMING CALLER",
            Self::ConnectedToOperator => "CONNECTED TO OPERATOR",
            Self::OperatorResponding => "OPERATOR RESPONSE IN PROGRESS",
            Self::RecoverableSystemFailure => "RECOVERABLE SYSTEM FAILURE",
            Self::AwaitingRouting => "AWAITING ROUTING",
            Self::CalleeRinging => "CALLEE RINGING",
            Self::CircuitConnected { tapped: false } => "DIRECT CIRCUIT CONNECTED",
            Self::CircuitConnected { tapped: true } => "TAP BRIDGE CIRCUIT CONNECTED",
            Self::DemonstrationComplete => "DEMONSTRATION COMPLETE",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cord(pub usize, pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CabinetSnapshot {
    pub sequence: u64,
    pub cords: Vec<Cord>,
    pub active_action: i32,
    pub crank_complete: bool,
    pub directory_id: u16,
    pub speaker_enabled: bool,
    pub reset: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CabinetOutput {
    pub sequence: u64,
    pub clock_minutes: u16,
    pub phase: Phase,
    pub reset_status: &'static str,
    pub routing_status: &'static str,
    pub line_lamps: [bool; 16],
    pub directory: Vec<String>,
    pub printer: Vec<String>,
    pub monitor_active: bool,
    pub speaker_active: bool,
    pub microphone_level: u8,
}

/// Typed consequential changes available to the MVP's hardcoded Subscribers.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum SubscriberAction {
    RequestKharadClinicRouting,
    AssessIncomingHouseholdCall,
    RequestSteelWorksRouting,
    ConfirmSteelWorksFreightStatus,
}

/// Authored MVP data owned by the Rust core. The Directory Terminal presents
/// the concise listing fields while dialogue and speech stages consume the
/// remaining profile fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubscriberProfile {
    id: u16,
    line: usize,
    pub identity: &'static str,
    pub line_listing: &'static str,
    pub occupation_or_role: &'static str,
    pub identifying_note: &'static str,
    pub personality: &'static str,
    pub speaking_style: &'static str,
    pub immediate_goal: &'static str,
    pub authoritative_callee: Option<&'static str>,
    pub initial_perspective: &'static str,
    pub paired_relationship: &'static str,
    pub permitted_actions: &'static [SubscriberAction],
    pub local_voice_configuration: &'static str,
}

impl SubscriberProfile {
    fn operator_session_prompt(&self, recent_dialogue: &[String], transcript: &str) -> String {
        format!(
            "You are {identity}, {role}. Personality: {personality} Speaking style: {style} Immediate goal: {goal} {routing_instruction} Relationship: {relationship} Recent conversation: {recent_dialogue} The Exchange Operator said: {transcript} Reply in character in one or two sentences, maximum 35 words. Do not invent facts or actions.",
            identity = self.identity,
            role = self.occupation_or_role,
            personality = self.personality,
            style = self.speaking_style,
            goal = self.immediate_goal,
            routing_instruction = self.authoritative_callee.map_or_else(
                || "".into(),
                |callee| format!("If requesting routing, explicitly name {callee}."),
            ),
            recent_dialogue = recent_dialogue.join(" "),
            relationship = self.paired_relationship,
        )
    }
}

/// A completed Operator Session ready for the Cabinet's existing speaker
/// and thermal-printer presentation surfaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperatorSession {
    pub transcript: String,
    pub response: String,
}

/// The local stage that stopped an Operator Session. The Cabinet presents these
/// errors as retryable instead of advancing the active Call Attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceStage {
    Capture,
    Stt,
    Dialogue,
    Tts,
}

impl VoiceStage {
    fn label(self) -> &'static str {
        match self {
            Self::Capture => "CAPTURE",
            Self::Stt => "STT",
            Self::Dialogue => "DIALOGUE",
            Self::Tts => "TTS",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoicePipelineError {
    stage: VoiceStage,
    message: String,
}

impl VoicePipelineError {
    pub fn new(stage: VoiceStage, message: impl Into<String>) -> Self {
        Self {
            stage,
            message: message.into(),
        }
    }

    fn presentation(&self) -> String {
        format!("{} FAILED: {}", self.stage.label(), self.message)
    }
}

/// Disposable MVP boundary around the local STT → dialogue → TTS pipeline.
/// Tests use a deterministic implementation; the runtime implementation uses
/// only the fixed local loopback workers.
pub trait VoicePipeline: Send {
    fn begin_capture(&mut self) -> Result<(), VoicePipelineError>;
    fn finish_operator_session(
        &mut self,
        profile: &SubscriberProfile,
        speaker_enabled: bool,
    ) -> Result<OperatorSession, VoicePipelineError>;

    fn cancel_capture(&mut self) {}

    fn microphone_level(&self) -> Arc<AtomicU32> {
        Arc::new(AtomicU32::new(0))
    }
}

struct FinishedOperatorSession {
    pipeline: Box<dyn VoicePipeline>,
    profile: SubscriberProfile,
    caller: SubscriberProfile,
    result: Result<OperatorSession, VoicePipelineError>,
}

struct ActiveCapture {
    path: PathBuf,
    session_id: u128,
}

struct ContinuousRecorder {
    _stream: cpal::Stream,
    sample_rate: u32,
    samples: Arc<Mutex<Vec<i16>>>,
    recording: Arc<AtomicBool>,
    stream_error: Arc<Mutex<Option<String>>>,
}

struct LocalVoicePipeline {
    microphone_level: Arc<AtomicU32>,
    recorder: Option<ContinuousRecorder>,
    recorder_failure: Option<VoicePipelineError>,
    capture: Option<ActiveCapture>,
    recent_dialogue: BTreeMap<u16, Vec<String>>,
}

impl Default for LocalVoicePipeline {
    fn default() -> Self {
        let microphone_level = Arc::new(AtomicU32::new(0));
        match Self::start_continuous_recorder(microphone_level.clone()) {
            Ok(recorder) => Self {
                microphone_level,
                recorder: Some(recorder),
                recorder_failure: None,
                capture: None,
                recent_dialogue: BTreeMap::new(),
            },
            Err(error) => Self {
                microphone_level,
                recorder: None,
                recorder_failure: Some(error),
                capture: None,
                recent_dialogue: BTreeMap::new(),
            },
        }
    }
}

impl LocalVoicePipeline {
    fn recorder(&self) -> Result<&ContinuousRecorder, VoicePipelineError> {
        self.recorder.as_ref().ok_or_else(|| {
            self.recorder_failure.clone().unwrap_or_else(|| {
                VoicePipelineError::new(VoiceStage::Capture, "PIPEWIRE MICROPHONE IS UNAVAILABLE")
            })
        })
    }

    fn voice_artifact_path(
        stage: VoiceStage,
        filename: &str,
    ) -> Result<PathBuf, VoicePipelineError> {
        let directory = PathBuf::from("runtime/voice");
        fs::create_dir_all(&directory).map_err(|error| {
            VoicePipelineError::new(stage, format!("COULD NOT CREATE VOICE ARTIFACTS: {error}"))
        })?;
        Ok(directory.join(filename))
    }

    fn next_session_id() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    }

    fn captured_audio_has_frames(audio_path: &PathBuf) -> Result<(), VoicePipelineError> {
        let metadata = fs::metadata(audio_path).map_err(|error| {
            VoicePipelineError::new(
                VoiceStage::Capture,
                format!("CAPTURED AUDIO IS UNAVAILABLE: {error}"),
            )
        })?;
        if metadata.len() <= 44 {
            return Err(VoicePipelineError::new(
                VoiceStage::Capture,
                "MICROPHONE RECORDED NO AUDIO",
            ));
        }
        Ok(())
    }

    fn captured_audio_has_signal(samples: &[i16]) -> Result<(), VoicePipelineError> {
        const SILENCE_PEAK_LIMIT: i16 = 16;

        let peak = samples
            .iter()
            .map(|sample| i32::from(*sample).unsigned_abs())
            .max()
            .unwrap_or_default();
        if peak <= u32::from(SILENCE_PEAK_LIMIT.unsigned_abs()) {
            return Err(VoicePipelineError::new(
                VoiceStage::Capture,
                "MICROPHONE CAPTURED SILENCE: SPEAK INTO THE ACTIVE PIPEWIRE INPUT",
            ));
        }
        Ok(())
    }

    fn capture_peak(samples: &[i16]) -> u32 {
        samples
            .iter()
            .map(|sample| i32::from(*sample).unsigned_abs())
            .max()
            .unwrap_or_default()
    }

    fn append_captured_samples<T>(
        input: &[T],
        samples: &Arc<Mutex<Vec<i16>>>,
        recording: &Arc<AtomicBool>,
        microphone_level: &Arc<AtomicU32>,
        channels: usize,
    ) where
        T: cpal::Sample,
        i16: cpal::FromSample<T>,
    {
        use cpal::Sample;

        let peak = input
            .iter()
            .map(|sample| i32::from(i16::from_sample(*sample)).unsigned_abs())
            .max()
            .unwrap_or_default();
        microphone_level.store(peak, Ordering::Release);
        if !recording.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut samples) = samples.lock() else {
            return;
        };
        for frame in input.chunks(channels) {
            if !frame.is_empty() {
                let sum = frame
                    .iter()
                    .map(|sample| i64::from(i16::from_sample(*sample)))
                    .sum::<i64>();
                samples.push((sum / frame.len() as i64) as i16);
            }
        }
    }

    fn build_capture_stream(
        device: &cpal::Device,
        config: &cpal::SupportedStreamConfig,
        samples: Arc<Mutex<Vec<i16>>>,
        recording: Arc<AtomicBool>,
        microphone_level: Arc<AtomicU32>,
        stream_error: Arc<Mutex<Option<String>>>,
    ) -> Result<cpal::Stream, VoicePipelineError> {
        use cpal::traits::DeviceTrait;

        let stream_config: cpal::StreamConfig = config.clone().into();
        let channels = usize::from(stream_config.channels);
        let stage = VoiceStage::Capture;
        match config.sample_format() {
            cpal::SampleFormat::I8 => {
                let stream_error = stream_error.clone();
                let recording = recording.clone();
                let microphone_level = microphone_level.clone();
                device.build_input_stream(
                    stream_config.clone(),
                    move |data: &[i8], _| {
                        Self::append_captured_samples(
                            data,
                            &samples,
                            &recording,
                            &microphone_level,
                            channels,
                        )
                    },
                    move |error: cpal::Error| {
                        if let Ok(mut saved_error) = stream_error.lock() {
                            *saved_error = Some(error.to_string());
                        }
                    },
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let stream_error = stream_error.clone();
                let recording = recording.clone();
                let microphone_level = microphone_level.clone();
                device.build_input_stream(
                    stream_config.clone(),
                    move |data: &[i16], _| {
                        Self::append_captured_samples(
                            data,
                            &samples,
                            &recording,
                            &microphone_level,
                            channels,
                        )
                    },
                    move |error: cpal::Error| {
                        if let Ok(mut saved_error) = stream_error.lock() {
                            *saved_error = Some(error.to_string());
                        }
                    },
                    None,
                )
            }
            cpal::SampleFormat::I32 => {
                let stream_error = stream_error.clone();
                let recording = recording.clone();
                let microphone_level = microphone_level.clone();
                device.build_input_stream(
                    stream_config.clone(),
                    move |data: &[i32], _| {
                        Self::append_captured_samples(
                            data,
                            &samples,
                            &recording,
                            &microphone_level,
                            channels,
                        )
                    },
                    move |error: cpal::Error| {
                        if let Ok(mut saved_error) = stream_error.lock() {
                            *saved_error = Some(error.to_string());
                        }
                    },
                    None,
                )
            }
            cpal::SampleFormat::F32 => {
                let stream_error = stream_error.clone();
                let microphone_level = microphone_level.clone();
                device.build_input_stream(
                    stream_config,
                    move |data: &[f32], _| {
                        Self::append_captured_samples(
                            data,
                            &samples,
                            &recording,
                            &microphone_level,
                            channels,
                        )
                    },
                    move |error: cpal::Error| {
                        if let Ok(mut saved_error) = stream_error.lock() {
                            *saved_error = Some(error.to_string());
                        }
                    },
                    None,
                )
            }
            format => {
                return Err(VoicePipelineError::new(
                    stage,
                    format!("UNSUPPORTED PIPEWIRE SAMPLE FORMAT: {format}"),
                ));
            }
        }
        .map_err(|error| {
            VoicePipelineError::new(stage, format!("COULD NOT START PIPEWIRE CAPTURE: {error}"))
        })
    }

    fn start_continuous_recorder(
        microphone_level: Arc<AtomicU32>,
    ) -> Result<ContinuousRecorder, VoicePipelineError> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        let host = cpal::host_from_id(cpal::HostId::PipeWire).map_err(|error| {
            VoicePipelineError::new(
                VoiceStage::Capture,
                format!("PIPEWIRE HOST UNAVAILABLE: {error}"),
            )
        })?;
        let device = host.default_input_device().ok_or_else(|| {
            VoicePipelineError::new(VoiceStage::Capture, "PIPEWIRE HAS NO DEFAULT MICROPHONE")
        })?;
        let device_name = device.to_string();
        let config = device.default_input_config().map_err(|error| {
            VoicePipelineError::new(
                VoiceStage::Capture,
                format!("MICROPHONE CONFIGURATION UNAVAILABLE: {error}"),
            )
        })?;
        let samples = Arc::new(Mutex::new(Vec::new()));
        let recording = Arc::new(AtomicBool::new(false));
        let stream_error = Arc::new(Mutex::new(None));
        let stream = Self::build_capture_stream(
            &device,
            &config,
            samples.clone(),
            recording.clone(),
            microphone_level,
            stream_error.clone(),
        )?;
        stream.play().map_err(|error| {
            VoicePipelineError::new(
                VoiceStage::Capture,
                format!("PIPEWIRE MICROPHONE COULD NOT START: {error}"),
            )
        })?;
        eprintln!("[voice] CAPTURE READY: PIPEWIRE INPUT {device_name}");
        Ok(ContinuousRecorder {
            _stream: stream,
            sample_rate: config.sample_rate(),
            samples,
            recording,
            stream_error,
        })
    }

    fn write_captured_audio(
        path: &PathBuf,
        sample_rate: u32,
        samples: &[i16],
    ) -> Result<(), VoicePipelineError> {
        let sample_bytes = u32::try_from(samples.len().saturating_mul(2)).map_err(|_| {
            VoicePipelineError::new(VoiceStage::Capture, "CAPTURE IS TOO LARGE TO WRITE")
        })?;
        let mut file = fs::File::create(path).map_err(|error| {
            VoicePipelineError::new(
                VoiceStage::Capture,
                format!("COULD NOT WRITE CAPTURED AUDIO: {error}"),
            )
        })?;
        file.write_all(b"RIFF")
            .and_then(|_| file.write_all(&(36_u32.saturating_add(sample_bytes)).to_le_bytes()))
            .and_then(|_| file.write_all(b"WAVEfmt "))
            .and_then(|_| file.write_all(&16_u32.to_le_bytes()))
            .and_then(|_| file.write_all(&1_u16.to_le_bytes()))
            .and_then(|_| file.write_all(&1_u16.to_le_bytes()))
            .and_then(|_| file.write_all(&sample_rate.to_le_bytes()))
            .and_then(|_| file.write_all(&sample_rate.saturating_mul(2).to_le_bytes()))
            .and_then(|_| file.write_all(&2_u16.to_le_bytes()))
            .and_then(|_| file.write_all(&16_u16.to_le_bytes()))
            .and_then(|_| file.write_all(b"data"))
            .and_then(|_| file.write_all(&sample_bytes.to_le_bytes()))
            .map_err(|error| {
                VoicePipelineError::new(
                    VoiceStage::Capture,
                    format!("COULD NOT WRITE CAPTURE HEADER: {error}"),
                )
            })?;
        for sample in samples {
            file.write_all(&sample.to_le_bytes()).map_err(|error| {
                VoicePipelineError::new(
                    VoiceStage::Capture,
                    format!("COULD NOT WRITE CAPTURE FRAMES: {error}"),
                )
            })?;
        }
        Ok(())
    }

    fn worker_port(variable: &str, stage: VoiceStage) -> Result<String, VoicePipelineError> {
        std::env::var(variable).map_err(|_| {
            VoicePipelineError::new(stage, "LOCAL WORKER PORT IS MISSING: USE just backend")
        })
    }

    fn post_local_worker(
        stage: VoiceStage,
        port: &str,
        path: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<Vec<u8>, VoicePipelineError> {
        let address = format!("127.0.0.1:{port}");
        let mut stream = TcpStream::connect(&address).map_err(|error| {
            VoicePipelineError::new(stage, format!("LOCAL WORKER UNAVAILABLE: {error}"))
        })?;
        let timeout = Some(Duration::from_secs(8));
        stream.set_read_timeout(timeout).map_err(|error| {
            VoicePipelineError::new(stage, format!("LOCAL WORKER TIMEOUT UNAVAILABLE: {error}"))
        })?;
        stream.set_write_timeout(timeout).map_err(|error| {
            VoicePipelineError::new(stage, format!("LOCAL WORKER TIMEOUT UNAVAILABLE: {error}"))
        })?;
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(head.as_bytes())
            .and_then(|_| stream.write_all(body))
            .map_err(|error| {
                VoicePipelineError::new(stage, format!("LOCAL WORKER REQUEST FAILED: {error}"))
            })?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).map_err(|error| {
            VoicePipelineError::new(stage, format!("LOCAL WORKER RESPONSE FAILED: {error}"))
        })?;
        Self::http_response_body(stage, &response)
    }

    fn http_response_body(
        stage: VoiceStage,
        response: &[u8],
    ) -> Result<Vec<u8>, VoicePipelineError> {
        let divider = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .ok_or_else(|| {
                VoicePipelineError::new(stage, "LOCAL WORKER SENT AN INVALID HTTP RESPONSE")
            })?;
        let head = std::str::from_utf8(&response[..divider]).map_err(|_| {
            VoicePipelineError::new(stage, "LOCAL WORKER SENT A NON-TEXT HTTP RESPONSE")
        })?;
        let status = head.split_whitespace().nth(1).unwrap_or("UNKNOWN");
        if !status.starts_with('2') {
            return Err(VoicePipelineError::new(
                stage,
                format!("LOCAL WORKER RETURNED HTTP {status}"),
            ));
        }
        let body = &response[divider + 4..];
        let chunked = head.lines().any(|line| {
            let Some((name, value)) = line.split_once(':') else {
                return false;
            };
            name.eq_ignore_ascii_case("transfer-encoding")
                && value
                    .split(',')
                    .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
        });
        if chunked {
            Self::decode_chunked_body(stage, body)
        } else {
            Ok(body.to_vec())
        }
    }

    fn decode_chunked_body(
        stage: VoiceStage,
        encoded: &[u8],
    ) -> Result<Vec<u8>, VoicePipelineError> {
        let mut cursor = 0;
        let mut decoded = Vec::new();
        loop {
            let line_end = encoded[cursor..]
                .windows(2)
                .position(|part| part == b"\r\n")
                .map(|offset| cursor + offset)
                .ok_or_else(|| {
                    VoicePipelineError::new(stage, "LOCAL WORKER SENT AN INVALID CHUNK")
                })?;
            let size = std::str::from_utf8(&encoded[cursor..line_end])
                .ok()
                .and_then(|size| size.split(';').next())
                .and_then(|size| usize::from_str_radix(size.trim(), 16).ok())
                .ok_or_else(|| {
                    VoicePipelineError::new(stage, "LOCAL WORKER SENT AN INVALID CHUNK SIZE")
                })?;
            cursor = line_end + 2;
            if size == 0 {
                return Ok(decoded);
            }
            let end = cursor
                .checked_add(size)
                .filter(|end| *end + 2 <= encoded.len())
                .ok_or_else(|| VoicePipelineError::new(stage, "LOCAL WORKER TRUNCATED A CHUNK"))?;
            decoded.extend_from_slice(&encoded[cursor..end]);
            if &encoded[end..end + 2] != b"\r\n" {
                return Err(VoicePipelineError::new(
                    stage,
                    "LOCAL WORKER SENT AN INVALID CHUNK TERMINATOR",
                ));
            }
            cursor = end + 2;
        }
    }

    fn append_form_field(body: &mut Vec<u8>, boundary: &str, name: &str, value: &[u8]) {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n")
                .as_bytes(),
        );
        body.extend_from_slice(value);
        body.extend_from_slice(b"\r\n");
    }

    fn append_audio_file(body: &mut Vec<u8>, boundary: &str, audio: &[u8]) {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"capture.wav\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes());
        body.extend_from_slice(audio);
        body.extend_from_slice(b"\r\n");
    }

    fn pocket_tts_voice(profile: &SubscriberProfile) -> Result<String, VoicePipelineError> {
        let voice = profile
            .local_voice_configuration
            .strip_prefix("pocket-tts:")
            .filter(|voice| !voice.is_empty())
            .ok_or_else(|| {
                VoicePipelineError::new(
                    VoiceStage::Tts,
                    format!(
                        "INVALID POCKET TTS VOICE PROFILE: {}",
                        profile.local_voice_configuration
                    ),
                )
            })?;
        Ok(voice.to_owned())
    }

    fn transcribe(&self, audio_path: &PathBuf) -> Result<String, VoicePipelineError> {
        let stage = VoiceStage::Stt;
        let audio = fs::read(audio_path).map_err(|error| {
            VoicePipelineError::new(stage, format!("CAPTURED AUDIO IS UNAVAILABLE: {error}"))
        })?;
        let boundary = "north-neeladesh-mvp-stt";
        let mut body = Vec::new();
        Self::append_audio_file(&mut body, boundary, &audio);
        Self::append_form_field(&mut body, boundary, "temperature", b"0.0");
        Self::append_form_field(&mut body, boundary, "response_format", b"json");
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let port = Self::worker_port("NN_MVP_STT_PORT", stage)?;
        let response = Self::post_local_worker(
            stage,
            &port,
            "/inference",
            &format!("multipart/form-data; boundary={boundary}"),
            &body,
        )?;
        let response: serde_json::Value = serde_json::from_slice(&response).map_err(|error| {
            VoicePipelineError::new(stage, format!("LOCAL WORKER SENT INVALID JSON: {error}"))
        })?;
        response
            .get("text")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| VoicePipelineError::new(stage, "LOCAL WORKER OMITTED THE TRANSCRIPT"))
    }

    fn request_llm(&self, prompt: &str) -> Result<String, VoicePipelineError> {
        let stage = VoiceStage::Dialogue;
        let body = serde_json::json!({
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0.2,
            "max_tokens": 48,
            "stream": false,
        })
        .to_string();
        let port = Self::worker_port("NN_MVP_LLM_PORT", stage)?;
        let response = Self::post_local_worker(
            stage,
            &port,
            "/v1/chat/completions",
            "application/json",
            body.as_bytes(),
        )?;
        let response: serde_json::Value = serde_json::from_slice(&response).map_err(|error| {
            VoicePipelineError::new(stage, format!("LOCAL WORKER SENT INVALID JSON: {error}"))
        })?;
        response
            .pointer("/choices/0/message/content")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| VoicePipelineError::new(stage, "LOCAL WORKER OMITTED THE REPLY"))
    }

    fn synthesize(
        &self,
        profile: &SubscriberProfile,
        response: &str,
        output_path: &PathBuf,
    ) -> Result<(), VoicePipelineError> {
        let stage = VoiceStage::Tts;
        let boundary = "north-neeladesh-mvp-tts";
        let mut body = Vec::new();
        Self::append_form_field(&mut body, boundary, "text", response.as_bytes());
        Self::append_form_field(
            &mut body,
            boundary,
            "voice_url",
            Self::pocket_tts_voice(profile)?.as_bytes(),
        );
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        let port = Self::worker_port("NN_MVP_TTS_PORT", stage)?;
        let wave = Self::post_local_worker(
            stage,
            &port,
            "/tts",
            &format!("multipart/form-data; boundary={boundary}"),
            &body,
        )?;
        if !wave.starts_with(b"RIFF") {
            return Err(VoicePipelineError::new(
                stage,
                "POCKET TTS RETURNED NON-WAV AUDIO",
            ));
        }
        fs::write(output_path, wave).map_err(|error| {
            VoicePipelineError::new(stage, format!("COULD NOT WRITE POCKET TTS AUDIO: {error}"))
        })
    }

    fn bounded_text(value: &str) -> String {
        value
            .split_whitespace()
            .take(35)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn response_is_on_goal(profile: &SubscriberProfile, response: &str) -> bool {
        profile
            .authoritative_callee
            .is_none_or(|callee| response.to_lowercase().contains(&callee.to_lowercase()))
    }

    fn fallback_response(profile: &SubscriberProfile) -> String {
        Self::bounded_text(&format!(
            "Operator, please connect me. {}",
            profile.immediate_goal
        ))
    }

    fn remember_dialogue(&mut self, profile: &SubscriberProfile, transcript: &str, response: &str) {
        const RECENT_DIALOGUE_LINES: usize = 4;

        let history = self.recent_dialogue.entry(profile.id).or_default();
        history.push(format!("OPERATOR: {transcript}"));
        history.push(format!("{}: {response}", profile.identity));
        if history.len() > RECENT_DIALOGUE_LINES {
            history.drain(..history.len() - RECENT_DIALOGUE_LINES);
        }
    }
}

impl VoicePipeline for LocalVoicePipeline {
    fn microphone_level(&self) -> Arc<AtomicU32> {
        self.microphone_level.clone()
    }

    fn begin_capture(&mut self) -> Result<(), VoicePipelineError> {
        if self.capture.is_some() {
            return Err(VoicePipelineError::new(
                VoiceStage::Capture,
                "PTT CAPTURE IS ALREADY ACTIVE",
            ));
        }
        let recorder = self.recorder()?;
        if let Some(error) = recorder
            .stream_error
            .lock()
            .ok()
            .and_then(|mut error| error.take())
        {
            return Err(VoicePipelineError::new(
                VoiceStage::Capture,
                format!("PIPEWIRE MICROPHONE STOPPED: {error}"),
            ));
        }
        let session_id = Self::next_session_id();
        let path =
            Self::voice_artifact_path(VoiceStage::Capture, &format!("{session_id}-operator.wav"))?;
        let mut samples = recorder.samples.lock().map_err(|_| {
            VoicePipelineError::new(
                VoiceStage::Capture,
                "PIPEWIRE CAPTURE BUFFER IS UNAVAILABLE",
            )
        })?;
        samples.clear();
        recorder.recording.store(true, Ordering::Release);
        drop(samples);

        self.capture = Some(ActiveCapture { path, session_id });
        Ok(())
    }

    fn finish_operator_session(
        &mut self,
        profile: &SubscriberProfile,
        speaker_enabled: bool,
    ) -> Result<OperatorSession, VoicePipelineError> {
        let ActiveCapture { path, session_id } = self.capture.take().ok_or_else(|| {
            VoicePipelineError::new(VoiceStage::Capture, "NO PTT RECORDING WAS STARTED")
        })?;
        let recorder = self.recorder()?;
        recorder.recording.store(false, Ordering::Release);
        if let Some(error) = recorder
            .stream_error
            .lock()
            .ok()
            .and_then(|mut error| error.take())
        {
            return Err(VoicePipelineError::new(
                VoiceStage::Capture,
                format!("PIPEWIRE MICROPHONE STOPPED: {error}"),
            ));
        }
        let captured_samples = recorder
            .samples
            .lock()
            .map_err(|_| {
                VoicePipelineError::new(
                    VoiceStage::Capture,
                    "PIPEWIRE CAPTURE BUFFER IS UNAVAILABLE",
                )
            })?
            .clone();
        Self::write_captured_audio(&path, recorder.sample_rate, &captured_samples)?;
        let capture_milliseconds = captured_samples
            .len()
            .saturating_mul(1_000)
            .checked_div(recorder.sample_rate as usize)
            .unwrap_or_default();
        eprintln!(
            "[voice] PTT CAPTURE: path={} frames={} duration_ms={} peak={}",
            path.display(),
            captured_samples.len(),
            capture_milliseconds,
            Self::capture_peak(&captured_samples),
        );

        let result = (|| {
            Self::captured_audio_has_frames(&path)?;
            Self::captured_audio_has_signal(&captured_samples)?;
            let transcript = Self::bounded_text(&self.transcribe(&path)?);
            if transcript.is_empty() {
                return Err(VoicePipelineError::new(
                    VoiceStage::Stt,
                    "NO SPEECH WAS TRANSCRIBED",
                ));
            }

            let recent_dialogue = self
                .recent_dialogue
                .get(&profile.id)
                .cloned()
                .unwrap_or_default();
            let prompt = profile.operator_session_prompt(&recent_dialogue, &transcript);
            let first_response = Self::bounded_text(&self.request_llm(&prompt)?);
            let response = if first_response.is_empty()
                || !Self::response_is_on_goal(profile, &first_response)
            {
                let retry = Self::bounded_text(&self.request_llm(&prompt)?);
                if retry.is_empty() || !Self::response_is_on_goal(profile, &retry) {
                    Self::fallback_response(profile)
                } else {
                    retry
                }
            } else {
                first_response
            };

            let speech_path = Self::voice_artifact_path(
                VoiceStage::Tts,
                &format!("{session_id}-subscriber.wav"),
            )?;
            let synthesis = self.synthesize(profile, &response, &speech_path);
            if let Err(error) = synthesis {
                return Err(error);
            }
            let playback = if speaker_enabled {
                let player =
                    std::env::var("NN_MVP_PLAY_COMMAND").unwrap_or_else(|_| "paplay".into());
                Command::new(player)
                    .arg(&speech_path)
                    .spawn()
                    .map_err(|error| {
                        VoicePipelineError::new(
                            VoiceStage::Tts,
                            format!("SPEAKER UNAVAILABLE: {error}"),
                        )
                    })
                    .map(|_| ())
            } else {
                Ok(())
            };
            playback?;
            self.remember_dialogue(profile, &transcript, &response);
            Ok(OperatorSession {
                transcript,
                response,
            })
        })();
        result
    }

    fn cancel_capture(&mut self) {
        if self.capture.take().is_some() {
            if let Some(recorder) = self.recorder.as_ref() {
                recorder.recording.store(false, Ordering::Release);
            }
        }
    }
}

impl Drop for LocalVoicePipeline {
    fn drop(&mut self) {
        self.cancel_capture();
    }
}

const SUBSCRIBERS: [SubscriberProfile; 4] = [
    SubscriberProfile {
        id: 4101,
        line: 4,
        identity: "NILA DAS",
        line_listing: "FOUNDRY APARTMENTS",
        occupation_or_role: "Foundry Apartments resident",
        identifying_note: "Keeps a sick household together on a foundry wage.",
        personality: "Worried, informal, impatient under stress, and fiercely protective of her household.",
        speaking_style: "Plainspoken and quick; asks directly when frightened.",
        immediate_goal: "Reach Kharad Clinic to arrange urgent care for her parent.",
        authoritative_callee: Some("Kharad Clinic"),
        initial_perspective: "The clinic may be the only safe place left for her household tonight.",
        paired_relationship: "Seeking practical help from Dr. Sorin Vale at Kharad Clinic.",
        permitted_actions: &[SubscriberAction::RequestKharadClinicRouting],
        local_voice_configuration: "pocket-tts:anna",
    },
    SubscriberProfile {
        id: 4102,
        line: 1,
        identity: "DR. SORIN VALE",
        line_listing: "KHARAD CLINIC",
        occupation_or_role: "Clinic intake worker",
        identifying_note: "Intake desk worker trusted to make scarce clinic time count.",
        personality: "Calm, concise, and empathetic; focused on facts that let the clinic help.",
        speaking_style: "Even-paced, precise questions followed by a brief reassurance.",
        immediate_goal: "Assess Nila Das's household emergency and secure the next safe step.",
        authoritative_callee: None,
        initial_perspective: "Care is scarce, but a clear account can still secure the right response.",
        paired_relationship: "Clinic contact for Nila Das, whose household needs urgent help.",
        permitted_actions: &[SubscriberAction::AssessIncomingHouseholdCall],
        local_voice_configuration: "pocket-tts:alba",
    },
    SubscriberProfile {
        id: 4103,
        line: 0,
        identity: "ARUN MEREK",
        line_listing: "RAILWAY DISPATCH",
        occupation_or_role: "Railway Dispatch clerk",
        identifying_note: "Dispatch ledger clerk responsible for an urgent freight interruption.",
        personality: "Brisk, procedural, and time-conscious; hates leaving an operational risk unlogged.",
        speaking_style: "Uses dispatch terms, short clauses, and numbered facts.",
        immediate_goal: "Reach Steel Works to resolve an urgent freight movement problem.",
        authoritative_callee: Some("Steel Works"),
        initial_perspective: "A missed rail window becomes a citywide delay unless Steel Works decides now.",
        paired_relationship: "Needs a decision from Leela Voss at Steel Works before the rail window closes.",
        permitted_actions: &[SubscriberAction::RequestSteelWorksRouting],
        local_voice_configuration: "pocket-tts:charles",
    },
    SubscriberProfile {
        id: 4104,
        line: 11,
        identity: "LEELA VOSS",
        line_listing: "STEEL WORKS",
        occupation_or_role: "Steel Works manager",
        identifying_note: "Manager whose plant schedule can disrupt the city's freight plans.",
        personality: "Measured, guarded, and status-conscious; reluctant to disclose more than necessary.",
        speaking_style: "Formal and deliberate, answering only the question she considers necessary.",
        immediate_goal: "Protect Steel Works' schedule while resolving Railway Dispatch's urgent problem.",
        authoritative_callee: None,
        initial_perspective: "The plant's commitments matter, but an unmanaged freight problem could expose her authority.",
        paired_relationship: "The Steel Works decision-maker sought by Arun Merek at Railway Dispatch.",
        permitted_actions: &[SubscriberAction::ConfirmSteelWorksFreightStatus],
        local_voice_configuration: "pocket-tts:vera",
    },
];

/// Returns the core-owned profile selected by a four-digit Directory ID.
/// Unknown IDs intentionally resolve to no profile and therefore no record.
pub fn subscriber_profile(id: u16) -> Option<&'static SubscriberProfile> {
    SUBSCRIBERS.iter().find(|subscriber| subscriber.id == id)
}

const OPERATOR_JACK: usize = 16;
const RING_JACK: usize = 17;
const TAP_ONE: (usize, usize) = (18, 19);
const TAP_TWO: (usize, usize) = (20, 21);

pub struct MvpCore {
    call_index: usize,
    clock_started_at: Instant,
    phase: Phase,
    receipts: Vec<String>,
    routing_status: &'static str,
    voice_pipeline: Option<Box<dyn VoicePipeline>>,
    voice_completion: Option<Receiver<FinishedOperatorSession>>,
    microphone_level: Arc<AtomicU32>,
    ptt_held: bool,
    capture_active: bool,
    operator_session_profile: Option<SubscriberProfile>,
    caller_ready_to_route: bool,
    active_tap_action: i32,
}

impl Default for MvpCore {
    fn default() -> Self {
        Self::new()
    }
}

impl MvpCore {
    pub fn new() -> Self {
        Self::with_voice_pipeline(LocalVoicePipeline::default())
    }

    pub fn with_voice_pipeline(pipeline: impl VoicePipeline + 'static) -> Self {
        let microphone_level = pipeline.microphone_level();
        Self {
            call_index: 0,
            clock_started_at: Instant::now(),
            phase: Phase::IncomingCaller,
            receipts: vec![
                "MINISTRY OF COMMUNICATIONS".into(),
                "KHARAD PROVINCIAL EXCHANGE".into(),
                "MVP CORE ONLINE — OFFLINE AUTHORITY READY".into(),
                "--------------------------------".into(),
            ],
            routing_status: "CALLER OFF-HOOK: CONNECT TO OPERATOR",
            voice_pipeline: Some(Box::new(pipeline)),
            voice_completion: None,
            microphone_level,
            ptt_held: false,
            capture_active: false,
            operator_session_profile: None,
            caller_ready_to_route: false,
            active_tap_action: -1,
        }
    }

    pub fn apply(&mut self, input: CabinetSnapshot) -> CabinetOutput {
        if input.reset {
            if let Some(pipeline) = self.voice_pipeline.as_mut() {
                pipeline.cancel_capture();
            }
            *self = Self::new();
            return self.output(
                input.sequence,
                "RESET COMPLETE",
                input.directory_id,
                false,
                input.speaker_enabled,
            );
        }

        self.collect_finished_operator_session();

        let (caller, callee) = self.current_pair();
        let connected_profile = operator_profile(&input.cords);
        let connected_to_operator = connected_profile.is_some();
        let ringing_attempt =
            has_pair(&input.cords, callee.line, RING_JACK) || input.crank_complete;
        let holding_ring_connection =
            input.cords.len() == 1 && has_pair(&input.cords, callee.line, RING_JACK);
        let valid_ringing_callee = holding_ring_connection && input.crank_complete;
        let direct = has_pair(&input.cords, caller.line, callee.line);
        let valid_direct = input.cords.len() == 1 && direct;
        let tap_action = tapped_bridge(&input.cords, caller.line, callee.line);
        let tapped = tap_action.is_some();

        if matches!(self.phase, Phase::ConnectedToOperator)
            && !connected_to_operator
            && self.caller_ready_to_route
        {
            self.phase = Phase::AwaitingRouting;
            self.routing_status = "AWAITING ROUTING: RING CALLEE";
        }

        if matches!(self.phase, Phase::AwaitingRouting) && direct {
            self.reject_routing("DIRECT CIRCUIT REJECTED: RING CALLEE FIRST");
            return self.output(
                input.sequence,
                "READY",
                input.directory_id,
                false,
                input.speaker_enabled,
            );
        }

        let ptt_held = input.active_action == 0;
        let ptt_pressed = ptt_held && !self.ptt_held;
        let ptt_released = !ptt_held && self.ptt_held;
        self.ptt_held = ptt_held;
        match self.phase {
            Phase::IncomingCaller if connected_to_operator => {
                self.phase = Phase::ConnectedToOperator;
                self.operator_session_profile = connected_profile;
                self.routing_status = "OPERATOR CONNECTED: HOLD PTT";
            }
            Phase::ConnectedToOperator | Phase::RecoverableSystemFailure
                if connected_to_operator && ptt_pressed =>
            {
                self.operator_session_profile = connected_profile;
                self.start_capture();
            }
            Phase::ConnectedToOperator | Phase::RecoverableSystemFailure
                if ptt_released && self.capture_active =>
            {
                self.finish_operator_session(
                    self.operator_session_profile.unwrap_or(caller),
                    caller,
                    input.speaker_enabled,
                );
            }
            Phase::AwaitingRouting if valid_ringing_callee => {
                self.phase = Phase::CalleeRinging;
                self.routing_status = "CALLEE RINGING: MAKE DIRECT CIRCUIT";
            }
            Phase::AwaitingRouting if ringing_attempt => {
                self.reject_routing("RINGING REJECTED: INVALID CORD TOPOLOGY");
            }
            Phase::CalleeRinging
                if (direct && !valid_direct)
                    || (!input.cords.is_empty()
                        && !holding_ring_connection
                        && !valid_direct
                        && !tapped) =>
            {
                self.reject_routing("DIRECT CIRCUIT REJECTED: INVALID CORD TOPOLOGY");
            }
            Phase::CalleeRinging if valid_direct || tapped => {
                self.complete_call(caller, callee, tap_action)
            }
            Phase::CircuitConnected { .. } if !direct && !tapped => {
                self.phase = Phase::IncomingCaller;
                self.routing_status = "NEXT CALLER OFF-HOOK: CONNECT TO OPERATOR";
            }
            _ => {}
        }

        let monitor_active = matches!(self.phase, Phase::CircuitConnected { tapped: true })
            && input.active_action == self.active_tap_action;
        self.output(
            input.sequence,
            "READY",
            input.directory_id,
            monitor_active,
            input.speaker_enabled,
        )
    }

    fn complete_call(
        &mut self,
        caller: SubscriberProfile,
        callee: SubscriberProfile,
        tap_action: Option<i32>,
    ) {
        let tapped = tap_action.is_some();
        let route = if tapped { "TAP BRIDGE" } else { "DIRECT" };
        self.receipts.push(format!(
            "ROUTING RECEIPT: {} → {}",
            caller.line_listing, callee.line_listing
        ));
        self.receipts.push(format!("CIRCUIT: {route} — SUCCESS"));
        self.receipts
            .push("--------------------------------".into());
        self.capture_active = false;
        self.active_tap_action = tap_action.unwrap_or(-1);
        self.call_index += 1;
        self.routing_status = "ROUTING COMPLETE: CLEAR CIRCUIT";
        self.phase = if self.call_index == 2 {
            self.receipts
                .push("MVP SUMMARY: BOTH CALLS COMPLETE".into());
            self.receipts.push("ALL FOUR SUBSCRIBERS EXERCISED".into());
            self.routing_status = "DEMONSTRATION COMPLETE";
            Phase::DemonstrationComplete
        } else {
            Phase::CircuitConnected { tapped }
        };
    }

    fn reject_routing(&mut self, reason: &'static str) {
        if self.routing_status != reason {
            self.receipts.push(reason.into());
        }
        self.routing_status = reason;
    }

    fn start_capture(&mut self) {
        let Some(pipeline) = self.voice_pipeline.as_mut() else {
            self.record_voice_error(VoicePipelineError::new(
                VoiceStage::Dialogue,
                "VOICE PIPELINE IS STILL PROCESSING",
            ));
            return;
        };
        match pipeline.begin_capture() {
            Ok(()) => {
                self.capture_active = true;
                self.phase = Phase::ConnectedToOperator;
                self.routing_status = "PTT RECORDING: RELEASE TO SEND";
            }
            Err(error) => self.record_voice_error(error),
        }
    }

    fn finish_operator_session(
        &mut self,
        profile: SubscriberProfile,
        caller: SubscriberProfile,
        speaker_enabled: bool,
    ) {
        self.capture_active = false;
        let Some(mut pipeline) = self.voice_pipeline.take() else {
            self.record_voice_error(VoicePipelineError::new(
                VoiceStage::Dialogue,
                "VOICE PIPELINE IS STILL PROCESSING",
            ));
            return;
        };
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = pipeline.finish_operator_session(&profile, speaker_enabled);
            let _ = sender.send(FinishedOperatorSession {
                pipeline,
                profile,
                caller,
                result,
            });
        });
        self.voice_completion = Some(receiver);
        self.phase = Phase::OperatorResponding;
        self.routing_status = "VOICE PROCESSING: CABINET REMAINS RESPONSIVE";
    }

    fn collect_finished_operator_session(&mut self) {
        let Some(receiver) = self.voice_completion.as_ref() else {
            return;
        };
        let Ok(finished) = receiver.try_recv() else {
            return;
        };
        self.voice_completion = None;
        self.voice_pipeline = Some(finished.pipeline);
        match finished.result {
            Ok(exchange) => {
                self.receipts
                    .push(format!("OPERATOR: {}", exchange.transcript));
                self.receipts.push(format!(
                    "{}: {}",
                    finished.profile.identity, exchange.response
                ));
                self.receipts.push(format!(
                    "VOICE: {} — LOCAL STT / DIALOGUE / TTS COMPLETE",
                    finished.profile.local_voice_configuration
                ));
                self.receipts
                    .push("--------------------------------".into());
                self.caller_ready_to_route = finished.profile.line == finished.caller.line;
                self.phase = Phase::ConnectedToOperator;
                self.routing_status = if self.caller_ready_to_route {
                    "OPERATOR SESSION ACTIVE: HOLD PTT TO CONTINUE OR CLEAR TO ROUTE"
                } else {
                    "OPERATOR SESSION ACTIVE: HOLD PTT TO CONTINUE"
                };
            }
            Err(error) => self.record_voice_error(error),
        }
    }

    fn record_voice_error(&mut self, error: VoicePipelineError) {
        let presentation = error.presentation();
        self.receipts
            .push(format!("OPERATOR SESSION FAILED: {presentation}"));
        self.receipts
            .push("HOLD PTT TO RETRY OR PRESS R TO RESET".into());
        self.phase = Phase::RecoverableSystemFailure;
        self.routing_status = "SYSTEM FAILURE: HOLD PTT TO RETRY";
    }

    fn current_pair(&self) -> (SubscriberProfile, SubscriberProfile) {
        if self.call_index == 0 {
            (SUBSCRIBERS[0], SUBSCRIBERS[1])
        } else {
            (SUBSCRIBERS[2], SUBSCRIBERS[3])
        }
    }

    fn clock_minutes(&self) -> u16 {
        let elapsed_minutes =
            self.clock_started_at
                .elapsed()
                .as_secs()
                .min(u64::from(SHIFT_END_MINUTES - SHIFT_START_MINUTES)) as u16;
        SHIFT_START_MINUTES + elapsed_minutes
    }

    fn output(
        &self,
        sequence: u64,
        reset_status: &'static str,
        directory_id: u16,
        monitor_active: bool,
        speaker_active: bool,
    ) -> CabinetOutput {
        let mut lamps = [false; 16];
        if !matches!(self.phase, Phase::DemonstrationComplete) {
            // The MVP authors all four demonstrable Subscribers as off-hook so
            // the Exchange Operator can select any profile for a session.
            for profile in SUBSCRIBERS {
                lamps[profile.line] = true;
            }
        }
        CabinetOutput {
            sequence,
            clock_minutes: self.clock_minutes(),
            phase: self.phase,
            reset_status,
            routing_status: self.routing_status,
            line_lamps: lamps,
            directory: directory_page(directory_id),
            printer: self.receipts.clone(),
            monitor_active,
            speaker_active,
            microphone_level: Self::normalize_microphone_level(
                self.microphone_level.load(Ordering::Acquire),
            ),
        }
    }

    fn normalize_microphone_level(peak: u32) -> u8 {
        let normalized = peak.saturating_mul(255) / u32::from(i16::MAX as u16);
        normalized.min(255) as u8
    }
}

fn has_pair(cords: &[Cord], left: usize, right: usize) -> bool {
    cords
        .iter()
        .any(|Cord(a, b)| (*a == left && *b == right) || (*a == right && *b == left))
}

fn operator_profile(cords: &[Cord]) -> Option<SubscriberProfile> {
    SUBSCRIBERS
        .iter()
        .copied()
        .find(|profile| has_pair(cords, profile.line, OPERATOR_JACK))
}

fn tapped_bridge(cords: &[Cord], caller: usize, callee: usize) -> Option<i32> {
    [(TAP_ONE.0, TAP_ONE.1), (TAP_TWO.0, TAP_TWO.1)]
        .into_iter()
        .enumerate()
        .find_map(|(index, (left, right))| {
            ((has_pair(cords, caller, left) && has_pair(cords, callee, right))
                || (has_pair(cords, caller, right) && has_pair(cords, callee, left)))
            .then_some(4 + index as i32)
        })
}

fn directory_page(id: u16) -> Vec<String> {
    match subscriber_profile(id) {
        Some(subscriber) => vec![
            subscriber.identity.into(),
            format!("{} · {}", subscriber.id, subscriber.line_listing),
            subscriber.occupation_or_role.into(),
            subscriber.identifying_note.into(),
        ],
        None => vec!["NO RECORD".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    struct SuccessfulVoicePipeline {
        started: bool,
        profile_identity: Option<&'static str>,
        speaker_enabled: bool,
    }

    impl VoicePipeline for SuccessfulVoicePipeline {
        fn begin_capture(&mut self) -> Result<(), VoicePipelineError> {
            self.started = true;
            Ok(())
        }

        fn finish_operator_session(
            &mut self,
            profile: &SubscriberProfile,
            speaker_enabled: bool,
        ) -> Result<OperatorSession, VoicePipelineError> {
            self.profile_identity = Some(profile.identity);
            self.speaker_enabled = speaker_enabled;
            Ok(OperatorSession {
                transcript: "My parent needs Kharad Clinic tonight.".into(),
                response: "Stay calm. Tell me their fever and I will prepare the intake desk."
                    .into(),
            })
        }
    }

    struct RetriableVoicePipeline {
        exchanges: usize,
    }

    struct CaptureCancellationPipeline(Arc<AtomicBool>);

    impl VoicePipeline for CaptureCancellationPipeline {
        fn begin_capture(&mut self) -> Result<(), VoicePipelineError> {
            Ok(())
        }

        fn finish_operator_session(
            &mut self,
            _: &SubscriberProfile,
            _: bool,
        ) -> Result<OperatorSession, VoicePipelineError> {
            unreachable!("reset cancels the active capture before PTT release")
        }

        fn cancel_capture(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    impl VoicePipeline for RetriableVoicePipeline {
        fn begin_capture(&mut self) -> Result<(), VoicePipelineError> {
            Ok(())
        }

        fn finish_operator_session(
            &mut self,
            _: &SubscriberProfile,
            _: bool,
        ) -> Result<OperatorSession, VoicePipelineError> {
            self.exchanges += 1;
            if self.exchanges == 1 {
                Err(VoicePipelineError::new(
                    VoiceStage::Stt,
                    "MICROPHONE INPUT WAS EMPTY",
                ))
            } else {
                Ok(OperatorSession {
                    transcript: "Please connect me now.".into(),
                    response: "I am ready at the intake desk.".into(),
                })
            }
        }
    }

    fn snapshot(
        cords: &[(usize, usize)],
        active_action: i32,
        crank_complete: bool,
    ) -> CabinetSnapshot {
        CabinetSnapshot {
            sequence: 1,
            cords: cords.iter().map(|&(a, b)| Cord(a, b)).collect(),
            active_action,
            crank_complete,
            directory_id: 4101,
            speaker_enabled: true,
            reset: false,
        }
    }

    fn test_core() -> MvpCore {
        MvpCore::with_voice_pipeline(SuccessfulVoicePipeline {
            started: false,
            profile_identity: None,
            speaker_enabled: false,
        })
    }

    fn complete_operator_session(core: &mut MvpCore, cords: &[(usize, usize)]) -> CabinetOutput {
        for _ in 0..100 {
            let output = core.apply(snapshot(cords, -1, false));
            if output.phase != Phase::OperatorResponding {
                return output;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("test voice pipeline did not finish")
    }

    #[test]
    fn fixed_call_transitions_from_incoming_to_direct_routing_receipt() {
        let mut core = test_core();
        let incoming = core.apply(snapshot(&[], -1, false));
        assert_eq!(incoming.phase, Phase::IncomingCaller);
        assert_eq!(incoming.clock_minutes, 9 * 60);
        assert!(incoming.line_lamps[4]);
        assert!(incoming.line_lamps[1]);
        assert_eq!(
            core.apply(snapshot(&[(4, 16)], -1, false)).phase,
            Phase::ConnectedToOperator
        );
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        assert_eq!(
            complete_operator_session(&mut core, &[(4, 16)]).phase,
            Phase::ConnectedToOperator
        );
        assert_eq!(
            core.apply(snapshot(&[(1, 17)], -1, true)).phase,
            Phase::CalleeRinging
        );
        let output = core.apply(snapshot(&[(4, 1)], -1, false));
        assert_eq!(output.phase, Phase::CircuitConnected { tapped: false });
        assert!(
            output
                .printer
                .iter()
                .any(|line| line.contains("FOUNDRY APARTMENTS"))
        );
        assert!(
            output
                .printer
                .iter()
                .any(|line| line == "CIRCUIT: DIRECT — SUCCESS")
        );
    }

    #[test]
    fn direct_circuit_requires_the_callee_to_be_rung_first() {
        let mut core = test_core();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        complete_operator_session(&mut core, &[(4, 16)]);

        let output = core.apply(snapshot(&[(4, 1)], -1, false));

        assert_eq!(output.phase, Phase::AwaitingRouting);
        assert_eq!(
            output.routing_status,
            "DIRECT CIRCUIT REJECTED: RING CALLEE FIRST"
        );
        assert!(
            output
                .printer
                .iter()
                .any(|line| line == "DIRECT CIRCUIT REJECTED: RING CALLEE FIRST")
        );
    }

    #[test]
    fn ringing_rejects_extra_or_unrelated_cords() {
        let mut core = test_core();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        complete_operator_session(&mut core, &[(4, 16)]);

        let output = core.apply(snapshot(&[(1, 17), (0, 2)], -1, true));

        assert_eq!(output.phase, Phase::AwaitingRouting);
        assert_eq!(
            output.routing_status,
            "RINGING REJECTED: INVALID CORD TOPOLOGY"
        );
        assert!(
            output
                .printer
                .iter()
                .any(|line| line == "RINGING REJECTED: INVALID CORD TOPOLOGY")
        );
    }

    #[test]
    fn direct_circuit_rejects_extra_or_unrelated_cords() {
        let mut core = test_core();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        complete_operator_session(&mut core, &[(4, 16)]);
        core.apply(snapshot(&[(1, 17)], -1, true));

        let output = core.apply(snapshot(&[(4, 1), (0, 2)], -1, false));

        assert_eq!(output.phase, Phase::CalleeRinging);
        assert_eq!(
            output.routing_status,
            "DIRECT CIRCUIT REJECTED: INVALID CORD TOPOLOGY"
        );
        assert!(
            output
                .printer
                .iter()
                .any(|line| line == "DIRECT CIRCUIT REJECTED: INVALID CORD TOPOLOGY")
        );
    }

    #[test]
    fn direct_circuit_rejects_the_caller_connected_to_the_wrong_line() {
        let mut core = test_core();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        complete_operator_session(&mut core, &[(4, 16)]);
        core.apply(snapshot(&[(1, 17)], -1, true));

        let output = core.apply(snapshot(&[(4, 2)], -1, false));

        assert_eq!(output.phase, Phase::CalleeRinging);
        assert_eq!(
            output.routing_status,
            "DIRECT CIRCUIT REJECTED: INVALID CORD TOPOLOGY"
        );
    }

    #[test]
    fn tap_bridge_requires_both_bridge_ports_and_only_monitors_when_held() {
        let mut core = test_core();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        complete_operator_session(&mut core, &[(4, 16)]);
        core.apply(snapshot(&[(1, 17)], -1, true));
        let output = core.apply(snapshot(&[(4, 18), (1, 19)], 4, false));
        assert_eq!(output.phase, Phase::CircuitConnected { tapped: true });
        assert!(output.monitor_active);
        assert!(
            !core
                .apply(snapshot(&[(4, 18), (1, 19)], -1, false))
                .monitor_active
        );
    }

    #[test]
    fn unknown_directory_id_and_reset_are_backend_owned() {
        let mut core = test_core();
        let mut unknown = snapshot(&[], -1, false);
        unknown.directory_id = 9999;
        assert_eq!(core.apply(unknown).directory, vec!["NO RECORD"]);
        let mut reset = snapshot(&[], -1, false);
        reset.reset = true;
        let output = core.apply(reset);
        assert_eq!(output.reset_status, "RESET COMPLETE");
        assert_eq!(output.phase, Phase::IncomingCaller);
    }

    #[test]
    fn reset_discards_completed_routing_and_is_repeatable() {
        let mut core = test_core();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        complete_operator_session(&mut core, &[(4, 16)]);
        core.apply(snapshot(&[(1, 17)], -1, true));
        let completed = core.apply(snapshot(&[(4, 1)], -1, false));
        assert!(
            completed
                .printer
                .iter()
                .any(|line| line.starts_with("ROUTING RECEIPT:"))
        );

        let mut reset = snapshot(&[], -1, false);
        reset.reset = true;
        let first_reset = core.apply(reset.clone());
        let second_reset = core.apply(reset);

        assert_eq!(first_reset.phase, Phase::IncomingCaller);
        assert_eq!(
            first_reset.routing_status,
            "CALLER OFF-HOOK: CONNECT TO OPERATOR"
        );
        assert!(first_reset.line_lamps[4]);
        assert_eq!(first_reset.printer, second_reset.printer);
        assert!(
            !first_reset
                .printer
                .iter()
                .any(|line| line.starts_with("ROUTING RECEIPT:"))
        );
    }

    #[test]
    fn directory_lookup_returns_each_complete_backend_owned_subscriber_profile() {
        let expected_records = [
            (
                4101,
                "NILA DAS",
                "FOUNDRY APARTMENTS",
                "Foundry Apartments resident",
                "pocket-tts:anna",
            ),
            (
                4102,
                "DR. SORIN VALE",
                "KHARAD CLINIC",
                "Clinic intake worker",
                "pocket-tts:alba",
            ),
            (
                4103,
                "ARUN MEREK",
                "RAILWAY DISPATCH",
                "Railway Dispatch clerk",
                "pocket-tts:charles",
            ),
            (
                4104,
                "LEELA VOSS",
                "STEEL WORKS",
                "Steel Works manager",
                "pocket-tts:vera",
            ),
        ];

        let mut core = test_core();
        for (id, identity, listing, role, voice) in expected_records {
            let profile = subscriber_profile(id).expect("known directory ID has a profile");
            assert_eq!(profile.identity, identity);
            assert_eq!(profile.line_listing, listing);
            assert_eq!(profile.occupation_or_role, role);
            assert_eq!(profile.local_voice_configuration, voice);
            assert!(!profile.identifying_note.is_empty());
            assert!(!profile.personality.is_empty());
            assert!(!profile.speaking_style.is_empty());
            assert!(!profile.immediate_goal.is_empty());
            assert!(!profile.initial_perspective.is_empty());
            assert!(!profile.paired_relationship.is_empty());
            assert!(!profile.permitted_actions.is_empty());

            let mut input = snapshot(&[], -1, false);
            input.directory_id = id;
            assert_eq!(
                core.apply(input).directory,
                vec![
                    identity.into(),
                    format!("{id} · {listing}"),
                    role.into(),
                    profile.identifying_note.into(),
                ]
            );
        }

        assert!(subscriber_profile(9999).is_none());

        let profiles = [4101, 4102, 4103, 4104]
            .map(|id| subscriber_profile(id).expect("known directory ID has a profile"));
        for field_values in [
            profiles.map(|profile| profile.identity),
            profiles.map(|profile| profile.line_listing),
            profiles.map(|profile| profile.occupation_or_role),
            profiles.map(|profile| profile.identifying_note),
            profiles.map(|profile| profile.personality),
            profiles.map(|profile| profile.speaking_style),
            profiles.map(|profile| profile.immediate_goal),
            profiles.map(|profile| profile.initial_perspective),
            profiles.map(|profile| profile.paired_relationship),
            profiles.map(|profile| profile.local_voice_configuration),
        ] {
            assert_eq!(field_values.into_iter().collect::<HashSet<_>>().len(), 4);
        }
        assert_eq!(
            profiles
                .map(|profile| profile.permitted_actions[0])
                .into_iter()
                .collect::<HashSet<_>>()
                .len(),
            4
        );
    }

    #[test]
    fn every_subscriber_selects_a_distinct_pocket_tts_voice_profile() {
        let voice_urls = SUBSCRIBERS.map(|profile| {
            LocalVoicePipeline::pocket_tts_voice(&profile)
                .expect("subscriber has a usable Pocket TTS voice profile")
        });

        assert_eq!(
            SUBSCRIBERS.map(|profile| profile.local_voice_configuration),
            [
                "pocket-tts:anna",
                "pocket-tts:alba",
                "pocket-tts:charles",
                "pocket-tts:vera",
            ]
        );
        assert_eq!(voice_urls, ["anna", "alba", "charles", "vera"]);
        assert_eq!(voice_urls.iter().collect::<HashSet<_>>().len(), 4);
    }

    #[test]
    fn fixed_callers_only_accept_a_request_that_names_their_authoritative_callee() {
        assert!(LocalVoicePipeline::response_is_on_goal(
            &SUBSCRIBERS[0],
            "Operator, please put me through to Kharad Clinic right away."
        ));
        assert!(!LocalVoicePipeline::response_is_on_goal(
            &SUBSCRIBERS[0],
            "Operator, I need help with my parent."
        ));
        assert!(LocalVoicePipeline::response_is_on_goal(
            &SUBSCRIBERS[2],
            "Connect Railway Dispatch to Steel Works before the rail window closes."
        ));
        assert!(LocalVoicePipeline::response_is_on_goal(
            &SUBSCRIBERS[1],
            "Tell me the fever and when it began."
        ));
    }

    #[test]
    fn operator_prompt_keeps_only_a_small_subscriber_specific_dialogue_window() {
        let history = vec![
            "OPERATOR: Is the intake desk ready?".into(),
            "DR. SORIN VALE: Yes, tell me the fever.".into(),
        ];

        let prompt = SUBSCRIBERS[1].operator_session_prompt(&history, "The fever began tonight.");

        assert!(prompt.contains("Recent conversation: OPERATOR: Is the intake desk ready?"));
        assert!(prompt.contains("The fever began tonight."));
        assert!(prompt.contains("DR. SORIN VALE"));
    }

    #[test]
    fn clearing_a_completed_circuit_advances_to_the_second_fixed_call() {
        let mut core = test_core();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        complete_operator_session(&mut core, &[(4, 16)]);
        core.apply(snapshot(&[(1, 17)], -1, true));
        core.apply(snapshot(&[(4, 1)], -1, false));

        let output = core.apply(snapshot(&[], -1, false));

        assert_eq!(output.phase, Phase::IncomingCaller);
        assert!(output.line_lamps[0]);
        assert!(output.line_lamps[4]);
    }

    #[test]
    fn speaker_control_is_echoed_as_rust_owned_cabinet_output() {
        let mut core = test_core();
        let mut input = snapshot(&[], -1, false);
        input.speaker_enabled = false;

        assert!(!core.apply(input).speaker_active);
    }

    #[test]
    fn cabinet_output_exposes_the_continuous_microphone_level_for_visualization() {
        let output = test_core().apply(snapshot(&[], -1, false));

        assert_eq!(output.microphone_level, 0);
    }

    #[test]
    fn releasing_ptt_runs_a_bounded_profile_grounded_voice_exchange() {
        let pipeline = SuccessfulVoicePipeline {
            started: false,
            profile_identity: None,
            speaker_enabled: false,
        };
        let mut core = MvpCore::with_voice_pipeline(pipeline);

        core.apply(snapshot(&[(4, 16)], -1, false));
        let recording = core.apply(snapshot(&[(4, 16)], 0, false));
        assert_eq!(recording.phase, Phase::ConnectedToOperator);
        assert_eq!(recording.routing_status, "PTT RECORDING: RELEASE TO SEND");

        core.apply(snapshot(&[(4, 16)], -1, false));
        let response = complete_operator_session(&mut core, &[(4, 16)]);

        assert_eq!(response.phase, Phase::ConnectedToOperator);
        assert_eq!(
            response.routing_status,
            "OPERATOR SESSION ACTIVE: HOLD PTT TO CONTINUE OR CLEAR TO ROUTE"
        );
        assert!(response.speaker_active);
        assert!(
            response
                .printer
                .iter()
                .any(|line| line == "OPERATOR: My parent needs Kharad Clinic tonight.")
        );
        assert!(
            response
                .printer
                .iter()
                .any(|line| line.starts_with("NILA DAS: Stay calm."))
        );
    }

    #[test]
    fn every_active_subscriber_is_lit_and_can_hold_a_multi_turn_operator_session() {
        let mut core = test_core();

        let initial = core.apply(snapshot(&[], -1, false));
        for profile in SUBSCRIBERS {
            assert!(
                initial.line_lamps[profile.line],
                "{} starts off-hook for the MVP",
                profile.identity
            );
        }

        for profile in SUBSCRIBERS {
            core.apply(snapshot(&[(profile.line, OPERATOR_JACK)], -1, false));
            core.apply(snapshot(&[(profile.line, OPERATOR_JACK)], 0, false));
            core.apply(snapshot(&[(profile.line, OPERATOR_JACK)], -1, false));
            let first_response =
                complete_operator_session(&mut core, &[(profile.line, OPERATOR_JACK)]);
            assert_eq!(first_response.phase, Phase::ConnectedToOperator);

            core.apply(snapshot(&[(profile.line, OPERATOR_JACK)], 0, false));
            core.apply(snapshot(&[(profile.line, OPERATOR_JACK)], -1, false));
            let second_response =
                complete_operator_session(&mut core, &[(profile.line, OPERATOR_JACK)]);
            assert_eq!(second_response.phase, Phase::ConnectedToOperator);
        }
    }

    #[test]
    fn releasing_ptt_returns_promptly_while_voice_work_continues_in_the_background() {
        use std::sync::mpsc;

        struct BlockingVoicePipeline(mpsc::Receiver<()>);

        impl VoicePipeline for BlockingVoicePipeline {
            fn begin_capture(&mut self) -> Result<(), VoicePipelineError> {
                Ok(())
            }

            fn finish_operator_session(
                &mut self,
                _: &SubscriberProfile,
                _: bool,
            ) -> Result<OperatorSession, VoicePipelineError> {
                self.0.recv().expect("test releases the voice worker");
                Ok(OperatorSession {
                    transcript: "Please connect me.".into(),
                    response: "I am ready.".into(),
                })
            }
        }

        let (release, blocked_worker) = mpsc::channel();
        let mut core = MvpCore::with_voice_pipeline(BlockingVoicePipeline(blocked_worker));
        core.apply(snapshot(&[(4, OPERATOR_JACK)], -1, false));
        core.apply(snapshot(&[(4, OPERATOR_JACK)], 0, false));

        let started = Instant::now();
        let queued = core.apply(snapshot(&[(4, OPERATOR_JACK)], -1, false));

        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(queued.phase, Phase::OperatorResponding);
        assert_eq!(
            queued.routing_status,
            "VOICE PROCESSING: CABINET REMAINS RESPONSIVE"
        );

        release.send(()).expect("release voice worker");
        for _ in 0..20 {
            let completed = core.apply(snapshot(&[(4, OPERATOR_JACK)], -1, false));
            if completed.phase == Phase::ConnectedToOperator {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("voice worker did not complete");
    }

    #[test]
    fn a_voice_stage_failure_stops_only_the_exchange_and_ptt_retries_it() {
        let mut core = MvpCore::with_voice_pipeline(RetriableVoicePipeline { exchanges: 0 });
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));

        core.apply(snapshot(&[(4, 16)], -1, false));
        let failed = complete_operator_session(&mut core, &[(4, 16)]);
        assert_eq!(failed.phase, Phase::RecoverableSystemFailure);
        assert_eq!(failed.routing_status, "SYSTEM FAILURE: HOLD PTT TO RETRY");
        assert!(
            failed
                .printer
                .iter()
                .any(|line| line
                    == "OPERATOR SESSION FAILED: STT FAILED: MICROPHONE INPUT WAS EMPTY")
        );
        assert!(failed.line_lamps[4]);

        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        let retried = complete_operator_session(&mut core, &[(4, 16)]);
        assert_eq!(retried.phase, Phase::ConnectedToOperator);
        assert_eq!(
            retried.routing_status,
            "OPERATOR SESSION ACTIVE: HOLD PTT TO CONTINUE OR CLEAR TO ROUTE"
        );
    }

    #[test]
    fn reset_cancels_an_active_ptt_capture_before_replacing_the_core() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut core = MvpCore::with_voice_pipeline(CaptureCancellationPipeline(cancelled.clone()));
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));

        let mut reset = snapshot(&[], -1, false);
        reset.reset = true;
        let output = core.apply(reset);

        assert!(cancelled.load(Ordering::SeqCst));
        assert_eq!(output.reset_status, "RESET COMPLETE");
    }

    #[test]
    fn local_pipeline_mixes_all_cpal_input_channels_to_mono() {
        let samples = Arc::new(Mutex::new(Vec::new()));
        let recording = Arc::new(AtomicBool::new(true));
        let microphone_level = Arc::new(AtomicU32::new(0));

        LocalVoicePipeline::append_captured_samples(
            &[0_i16, 1_000, 0, -1_000],
            &samples,
            &recording,
            &microphone_level,
            2,
        );

        assert_eq!(*samples.lock().expect("read captured samples"), [500, -500]);
        assert_eq!(microphone_level.load(Ordering::Acquire), 1_000);
    }

    #[test]
    fn local_pipeline_only_keeps_audio_while_ptt_is_active() {
        let samples = Arc::new(Mutex::new(Vec::new()));
        let recording = Arc::new(AtomicBool::new(false));
        let microphone_level = Arc::new(AtomicU32::new(0));

        LocalVoicePipeline::append_captured_samples(
            &[100_i16, 200],
            &samples,
            &recording,
            &microphone_level,
            1,
        );
        recording.store(true, Ordering::Release);
        LocalVoicePipeline::append_captured_samples(
            &[300_i16, 400],
            &samples,
            &recording,
            &microphone_level,
            1,
        );
        recording.store(false, Ordering::Release);
        LocalVoicePipeline::append_captured_samples(
            &[500_i16, 600],
            &samples,
            &recording,
            &microphone_level,
            1,
        );

        assert_eq!(*samples.lock().expect("read captured samples"), [300, 400]);
        assert_eq!(microphone_level.load(Ordering::Acquire), 600);
    }

    #[test]
    fn local_pipeline_rejects_a_silent_microphone_segment_before_stt() {
        let result = LocalVoicePipeline::captured_audio_has_signal(&[0, 16, -16]);

        assert!(matches!(
            result,
            Err(VoicePipelineError {
                stage: VoiceStage::Capture,
                ..
            })
        ));
    }

    #[test]
    fn local_pipeline_rejects_a_wav_header_without_microphone_frames() {
        let path = LocalVoicePipeline::voice_artifact_path(VoiceStage::Capture, "test-empty.wav")
            .expect("create the voice artifact directory");
        fs::write(&path, [0_u8; 44]).expect("write a header-only WAV fixture");

        let result = LocalVoicePipeline::captured_audio_has_frames(&path);

        let _ = fs::remove_file(path);
        assert!(matches!(
            result,
            Err(VoicePipelineError {
                stage: VoiceStage::Capture,
                ..
            })
        ));
    }

    #[test]
    fn local_http_client_decodes_chunked_tts_audio() {
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: audio/wav\r\n\r\n4\r\nRIFF\r\n4\r\nWAVE\r\n0\r\n\r\n";

        let body = LocalVoicePipeline::http_response_body(VoiceStage::Tts, response)
            .expect("decode the local Pocket TTS response");

        assert_eq!(body, b"RIFFWAVE");
    }
}
