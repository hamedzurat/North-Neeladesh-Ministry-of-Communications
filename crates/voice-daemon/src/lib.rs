use std::collections::VecDeque;
use std::io::{self, BufReader, Read, Write};
use std::net::UdpSocket;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use exchange_protocol::{
    ProtocolError, RtpL16Packet, VOICE_AUDIO_PACKET_SAMPLES, VOICE_AUDIO_SAMPLE_RATE,
    VOICE_INPUT_SAMPLE_RATE, VOICE_PROTOCOL_VERSION, VoiceControlMessage, VoiceInputAudioMessage,
    VoiceStatus, VoiceStatusMessage, decode_voice_control, encode_voice_input_audio,
    encode_voice_status,
};
use serde::{Deserialize, Serialize};

pub const MAX_RESPONSE_CONTEXT_TOKENS: usize = 3_072;
const MAX_DIALOGUE_CHARS: usize = 2_000;
const DEFAULT_MAX_CAPTURE_SAMPLES: usize = VOICE_AUDIO_SAMPLE_RATE as usize * 15;
const MAX_WORKER_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
static WORKER_CANCELLED: AtomicBool = AtomicBool::new(false);
static LAST_LOG: OnceLock<Mutex<Instant>> = OnceLock::new();

pub fn log_voice_event(message: impl std::fmt::Display) {
    let now = Instant::now();
    let previous = LAST_LOG.get_or_init(|| Mutex::new(now));
    let mut previous = previous.lock().unwrap_or_else(|error| error.into_inner());
    let delta = now.duration_since(*previous);
    *previous = now;
    eprintln!("[+{:0.3}s] {message}", delta.as_secs_f64());
}

pub fn request_worker_cancellation() {
    WORKER_CANCELLED.store(true, Ordering::Release);
}

fn clear_worker_cancellation() {
    WORKER_CANCELLED.store(false, Ordering::Release);
}

fn worker_cancellation_requested() -> bool {
    WORKER_CANCELLED.load(Ordering::Acquire)
}

fn prepare_process_group(command: &mut Command) {
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn terminate_process_group(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubscriberProfile {
    pub subscriber_id: u8,
    pub name: String,
    pub voice_id: String,
    pub personality: String,
    pub baseline_goals: Vec<String>,
    pub initial_perspective: String,
    pub relationships: Vec<RelationshipNote>,
    pub permitted_actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConversationTurn {
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KnowledgeRecord {
    pub fact: String,
    pub learned_from: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BeliefRecord {
    pub proposition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelationshipNote {
    pub subject: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemoryRecord {
    pub summary: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResponseContext {
    pub profile: SubscriberProfile,
    pub subscriber_goal: String,
    pub call_premise: String,
    pub story_beat_direction: String,
    pub permitted_knowledge: Vec<KnowledgeRecord>,
    pub beliefs: Vec<BeliefRecord>,
    pub relationship_notes: Vec<RelationshipNote>,
    pub memories: Vec<MemoryRecord>,
    pub recent_conversation: Vec<ConversationTurn>,
    pub current_input: Option<String>,
}

impl ResponseContext {
    pub fn validate(&self) -> Result<(), VoiceError> {
        let tokens = approximate_tokens(self);
        if tokens > MAX_RESPONSE_CONTEXT_TOKENS {
            return Err(VoiceError::new(
                "response_context_too_large",
                format!("response context is approximately {tokens} tokens"),
            ));
        }
        if self.recent_conversation.len() > 6 {
            return Err(VoiceError::new(
                "response_context_too_many_turns",
                "response context contains more than six recent turns",
            ));
        }
        Ok(())
    }

    fn with_transcript(&self, transcript: &str) -> Result<Self, VoiceError> {
        let mut context = self.clone();
        context.current_input = Some(transcript.to_string());
        context.validate()?;
        Ok(context)
    }
}

fn approximate_tokens(context: &ResponseContext) -> usize {
    let mut chars = context.profile.name.len()
        + context.profile.voice_id.len()
        + context.profile.personality.len()
        + context.subscriber_goal.len()
        + context.call_premise.len()
        + context.story_beat_direction.len();
    for goal in &context.profile.baseline_goals {
        chars += goal.len();
    }
    chars += context.profile.initial_perspective.len();
    for relationship in &context.profile.relationships {
        chars += relationship.subject.len() + relationship.note.len();
    }
    for action in &context.profile.permitted_actions {
        chars += action.len();
    }
    for value in &context.permitted_knowledge {
        chars += value.fact.len() + value.learned_from.len();
    }
    for value in &context.beliefs {
        chars += value.proposition.len();
    }
    for value in &context.relationship_notes {
        chars += value.subject.len() + value.note.len();
    }
    for value in &context.memories {
        chars += value.summary.len() + value.source.len();
    }
    for turn in &context.recent_conversation {
        chars += turn.speaker.len() + turn.text.len();
    }
    if let Some(input) = &context.current_input {
        chars += input.len();
    }
    chars.div_ceil(4)
}

pub trait MicrophoneCapture {
    fn start(&mut self) -> Result<(), VoiceError>;
    fn finish(&mut self) -> Result<Vec<i16>, VoiceError>;
}

pub trait SpeechToText {
    fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError>;
}

pub trait DialogueGenerator {
    fn generate(
        &mut self,
        context: &ResponseContext,
        transcript: &str,
    ) -> Result<SubscriberResponse, VoiceError>;
}

pub trait TextToSpeech {
    fn synthesize(&mut self, voice_id: &str, text: &str) -> Result<Vec<i16>, VoiceError>;

    fn synthesize_stream(
        &mut self,
        voice_id: &str,
        text: &str,
        emit: &mut dyn FnMut(&[i16]) -> Result<(), VoiceError>,
    ) -> Result<usize, VoiceError> {
        let samples = self.synthesize(voice_id, text)?;
        let length = samples.len();
        emit(&samples)?;
        Ok(length)
    }
}

pub trait VoiceOutput {
    fn status(&mut self, message: VoiceStatusMessage) -> Result<(), VoiceError>;
    fn audio(&mut self, packet: RtpL16Packet) -> Result<(), VoiceError>;
    fn finish(&mut self) -> Result<(), VoiceError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriberResponse {
    pub dialogue: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    Ready,
    Listening,
    Completed,
    Failed,
    Cancelled,
}

pub struct OperatorSession {
    session_id: u64,
    turn_id: u64,
    state_revision: u64,
    context: ResponseContext,
    capture: Box<dyn MicrophoneCapture>,
    stt: Box<dyn SpeechToText>,
    dialogue: Box<dyn DialogueGenerator>,
    tts: Box<dyn TextToSpeech>,
    output: Box<dyn VoiceOutput>,
    phase: SessionPhase,
}

impl OperatorSession {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: u64,
        state_revision: u64,
        context: ResponseContext,
        capture: Box<dyn MicrophoneCapture>,
        stt: Box<dyn SpeechToText>,
        dialogue: Box<dyn DialogueGenerator>,
        tts: Box<dyn TextToSpeech>,
        output: Box<dyn VoiceOutput>,
    ) -> Result<Self, VoiceError> {
        context.validate()?;
        Ok(Self {
            session_id,
            turn_id: 1,
            state_revision,
            context,
            capture,
            stt,
            dialogue,
            tts,
            output,
            phase: SessionPhase::Ready,
        })
    }

    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    pub fn set_state_revision(&mut self, state_revision: u64) {
        self.state_revision = state_revision;
    }

    pub fn set_voice_id(&mut self, voice_id: &str) -> Result<(), VoiceError> {
        if self.phase != SessionPhase::Ready {
            return Err(VoiceError::new(
                "invalid_session_phase",
                "voice id can only change before a turn starts",
            ));
        }
        if voice_id.trim().is_empty() {
            return Err(VoiceError::new(
                "invalid_voice_id",
                "voice id cannot be empty",
            ));
        }
        self.context.profile.voice_id = voice_id.to_string();
        Ok(())
    }

    pub fn start_ptt(&mut self) -> Result<(), VoiceError> {
        if self.phase != SessionPhase::Ready {
            return Err(VoiceError::new(
                "invalid_session_phase",
                "PTT can only start a ready Operator Session",
            ));
        }
        clear_worker_cancellation();
        if let Err(error) = self.capture.start() {
            self.phase = SessionPhase::Failed;
            self.emit_failure(&error);
            return Err(error);
        }
        self.phase = SessionPhase::Listening;
        self.emit(VoiceStatus::Listening, None, None, None)?;
        Ok(())
    }

    pub fn announce_ready(&mut self) -> Result<(), VoiceError> {
        self.emit(VoiceStatus::Ready, None, None, None)
    }

    pub fn prepare_next_turn(&mut self) -> Result<(), VoiceError> {
        if self.phase != SessionPhase::Completed {
            return Err(VoiceError::new(
                "invalid_session_phase",
                "only a completed Operator Session can start its next turn",
            ));
        }
        self.turn_id = self
            .turn_id
            .checked_add(1)
            .ok_or_else(|| VoiceError::new("turn_id_exhausted", "voice turn id overflowed"))?;
        clear_worker_cancellation();
        self.phase = SessionPhase::Ready;
        self.emit(VoiceStatus::Ready, None, None, None)
    }

    pub fn cancel(&mut self) -> Result<(), VoiceError> {
        if matches!(
            self.phase,
            SessionPhase::Completed | SessionPhase::Failed | SessionPhase::Cancelled
        ) {
            return Err(VoiceError::new(
                "invalid_session_phase",
                "a finished Operator Session cannot be cancelled",
            ));
        }
        if self.phase == SessionPhase::Listening {
            let _ = self.capture.finish();
        }
        self.phase = SessionPhase::Cancelled;
        self.emit(VoiceStatus::Cancelled, None, None, None)
    }

    pub fn release_ptt(&mut self) -> Result<SubscriberResponse, VoiceError> {
        if self.phase != SessionPhase::Listening {
            return Err(VoiceError::new(
                "invalid_session_phase",
                "PTT can only release an active Operator Session",
            ));
        }
        let samples = match self.capture.finish() {
            Ok(samples) => samples,
            Err(error) => return self.fail(error),
        };
        self.emit(VoiceStatus::Transcribing, None, None, None)?;
        let transcript = match self.stt.transcribe(&samples) {
            Ok(transcript) if !transcript.trim().is_empty() => transcript,
            Ok(_) => {
                return self.fail(VoiceError::new(
                    "empty_transcript",
                    "speech recognition returned no transcript",
                ));
            }
            Err(error) => return self.fail(error),
        };
        let context = match self.context.with_transcript(&transcript) {
            Ok(context) => context,
            Err(error) => return self.fail(error),
        };
        self.emit(
            VoiceStatus::GeneratingResponse,
            Some(&transcript),
            None,
            None,
        )?;
        let response = match self.dialogue.generate(&context, &transcript) {
            Ok(response) if !response.dialogue.trim().is_empty() => response,
            Ok(_) => {
                return self.fail(VoiceError::new(
                    "empty_dialogue",
                    "dialogue generation returned no speech",
                ));
            }
            Err(error) => return self.fail(error),
        };
        if response.dialogue.chars().count() > MAX_DIALOGUE_CHARS {
            return self.fail(VoiceError::new(
                "dialogue_too_long",
                "subscriber dialogue exceeds the bounded turn limit",
            ));
        }
        self.emit(
            VoiceStatus::Synthesizing,
            Some(&transcript),
            Some(&response.dialogue),
            None,
        )?;
        self.emit(
            VoiceStatus::Playing,
            Some(&transcript),
            Some(&response.dialogue),
            None,
        )?;
        let session_id = self.session_id;
        let output = &mut self.output;
        let mut packet_index = 0_usize;
        let mut sample_offset = 0_usize;
        let result = self.tts.synthesize_stream(
            &context.profile.voice_id,
            &response.dialogue,
            &mut |samples| {
                if worker_cancellation_requested() {
                    return Err(VoiceError::new(
                        "worker_cancelled",
                        "voice playback was cancelled",
                    ));
                }
                for chunk in samples.chunks(VOICE_AUDIO_PACKET_SAMPLES) {
                    output.audio(RtpL16Packet {
                        marker: packet_index == 0,
                        sequence: packet_index as u16,
                        timestamp: sample_offset as u32,
                        ssrc: session_id as u32,
                        samples: chunk.to_vec(),
                    })?;
                    packet_index += 1;
                    sample_offset += chunk.len();
                }
                Ok(())
            },
        );
        let samples = match result {
            Ok(samples) if samples > 0 => samples,
            Ok(_) => {
                return self.fail(VoiceError::new(
                    "tts_empty_output",
                    "Qwen3-TTS returned no audio samples",
                ));
            }
            Err(error) => return self.fail(error),
        };
        if samples != sample_offset {
            return self.fail(VoiceError::new(
                "tts_output_mismatch",
                "Qwen3-TTS reported a different sample count than it emitted",
            ));
        }
        if let Err(error) = self.output.finish() {
            return self.fail(error);
        }
        self.context.recent_conversation.extend([
            ConversationTurn {
                speaker: "operator".to_string(),
                text: transcript.clone(),
            },
            ConversationTurn {
                speaker: "subscriber".to_string(),
                text: response.dialogue.clone(),
            },
        ]);
        self.context.current_input = None;
        while self.context.validate().is_err() && !self.context.recent_conversation.is_empty() {
            self.context.recent_conversation.remove(0);
        }
        self.emit(
            VoiceStatus::Completed,
            Some(&transcript),
            Some(&response.dialogue),
            None,
        )?;
        self.phase = SessionPhase::Completed;
        Ok(response)
    }

    fn emit(
        &mut self,
        status: VoiceStatus,
        transcript: Option<&str>,
        response_text: Option<&str>,
        error: Option<ProtocolError>,
    ) -> Result<(), VoiceError> {
        self.output.status(VoiceStatusMessage {
            protocol_version: VOICE_PROTOCOL_VERSION,
            session_id: self.session_id,
            turn_id: self.turn_id,
            state_revision: self.state_revision,
            status,
            transcript: transcript.map(str::to_string),
            response_text: response_text.map(str::to_string),
            error,
        })
    }

    fn emit_failure(&mut self, error: &VoiceError) {
        let _ = self.emit(
            VoiceStatus::Failed,
            None,
            None,
            Some(ProtocolError {
                code: error.code.clone(),
                message: error.message.clone(),
            }),
        );
    }

    fn fail<T>(&mut self, error: VoiceError) -> Result<T, VoiceError> {
        if worker_cancellation_requested() || error.code == "worker_cancelled" {
            self.phase = SessionPhase::Cancelled;
            let _ = self.emit(VoiceStatus::Cancelled, None, None, None);
        } else {
            self.phase = SessionPhase::Failed;
            self.emit_failure(&error);
        }
        Err(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceError {
    pub code: String,
    pub message: String,
}

impl VoiceError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for VoiceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for VoiceError {}

pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn from_words(words: &str) -> Result<Self, VoiceError> {
        let mut parts = words.split_whitespace();
        let Some(program) = parts.next() else {
            return Err(VoiceError::new(
                "invalid_command",
                "command cannot be empty",
            ));
        };
        Ok(Self::new(program, parts.map(str::to_string).collect()))
    }
}

pub struct CommandMicrophone {
    spec: CommandSpec,
    max_samples: usize,
    child: Option<Arc<Mutex<Child>>>,
    reader: Option<JoinHandle<Result<Vec<u8>, VoiceError>>>,
    diagnostics: Option<JoinHandle<Result<Vec<u8>, VoiceError>>>,
}

impl CommandMicrophone {
    pub fn new(spec: CommandSpec) -> Self {
        Self::with_max_samples(spec, DEFAULT_MAX_CAPTURE_SAMPLES)
    }

    pub fn with_max_samples(spec: CommandSpec, max_samples: usize) -> Self {
        Self {
            spec,
            max_samples,
            child: None,
            reader: None,
            diagnostics: None,
        }
    }
}

impl MicrophoneCapture for CommandMicrophone {
    fn start(&mut self) -> Result<(), VoiceError> {
        if self.child.is_some() {
            return Err(VoiceError::new(
                "capture_already_started",
                "microphone is already capturing",
            ));
        }
        let mut command = Command::new(&self.spec.program);
        command
            .args(&self.spec.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        prepare_process_group(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| VoiceError::new("capture_start_failed", error.to_string()))?;
        let mut stdout = child.stdout.take().ok_or_else(|| {
            VoiceError::new("capture_stdout_failed", "capture stdout was unavailable")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            VoiceError::new("capture_stderr_failed", "capture stderr was unavailable")
        })?;
        let child = Arc::new(Mutex::new(child));
        let child_for_reader = Arc::clone(&child);
        let max_bytes = self.max_samples.saturating_add(1).saturating_mul(2);
        let reader = thread::spawn(move || {
            let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
            let mut buffer = [0_u8; 8 * 1024];
            loop {
                let length = stdout
                    .read(&mut buffer)
                    .map_err(|error| VoiceError::new("capture_read_failed", error.to_string()))?;
                if length == 0 {
                    break;
                }
                let remaining = max_bytes.saturating_sub(bytes.len());
                bytes.extend_from_slice(&buffer[..length.min(remaining)]);
                if bytes.len() >= max_bytes {
                    if let Ok(mut child) = child_for_reader.lock() {
                        terminate_process_group(&mut child);
                    }
                }
            }
            Ok(bytes)
        });
        let diagnostics =
            thread::spawn(move || read_bounded(stderr, 16 * 1024, "capture_stderr_failed"));
        self.child = Some(child);
        self.reader = Some(reader);
        self.diagnostics = Some(diagnostics);
        Ok(())
    }

    fn finish(&mut self) -> Result<Vec<i16>, VoiceError> {
        let Some(child) = self.child.take() else {
            return Err(VoiceError::new(
                "capture_not_started",
                "microphone was not started",
            ));
        };
        let mut child = child.lock().map_err(|_| {
            VoiceError::new("capture_wait_failed", "capture process lock was poisoned")
        })?;
        let (status, stopped_for_release) = match child
            .try_wait()
            .map_err(|error| VoiceError::new("capture_wait_failed", error.to_string()))?
        {
            Some(status) => (status, false),
            None => {
                terminate_process_group(&mut child);
                let status = child
                    .wait()
                    .map_err(|error| VoiceError::new("capture_wait_failed", error.to_string()))?;
                (status, true)
            }
        };
        drop(child);
        let bytes = self
            .reader
            .take()
            .ok_or_else(|| {
                VoiceError::new("capture_reader_failed", "capture reader was unavailable")
            })?
            .join()
            .map_err(|_| VoiceError::new("capture_reader_failed", "capture reader panicked"))??;
        let diagnostics = self
            .diagnostics
            .take()
            .ok_or_else(|| {
                VoiceError::new(
                    "capture_stderr_failed",
                    "capture stderr reader was unavailable",
                )
            })?
            .join()
            .map_err(|_| {
                VoiceError::new("capture_stderr_failed", "capture stderr reader panicked")
            })??;
        let samples = decode_pcm16(&bytes)?;
        if samples.len() > self.max_samples {
            return Err(VoiceError::new(
                "capture_too_long",
                format!("capture exceeded {} samples", self.max_samples),
            ));
        }
        if !status.success() && (!stopped_for_release || samples.is_empty()) {
            return Err(VoiceError::new(
                "capture_failed",
                format!(
                    "capture exited with {status}: {}",
                    diagnostic_text(&diagnostics)
                ),
            ));
        }
        Ok(samples)
    }
}

pub struct CpalMicrophone {
    _stream: Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    active: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    input_rate: u32,
    max_samples: usize,
}

impl CpalMicrophone {
    pub fn new(max_samples: usize) -> Result<Self, VoiceError> {
        let host = cpal::default_host();
        let device = host.default_input_device().ok_or_else(|| {
            VoiceError::new(
                "capture_device_unavailable",
                "no default audio input device is connected",
            )
        })?;
        log_voice_event(format_args!(
            "voice input device: {}",
            device.name().unwrap_or_else(|_| "<unnamed>".to_string())
        ));
        let supported = device
            .default_input_config()
            .map_err(|error| VoiceError::new("capture_config_unavailable", error.to_string()))?;
        let input_rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let config = supported.config();
        let samples = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let max_input_samples = max_samples
            .saturating_mul(input_rate as usize)
            .div_ceil(VOICE_INPUT_SAMPLE_RATE as usize)
            .saturating_add(input_rate as usize);
        let stream_error = Arc::clone(&error);
        let error_handler = move |stream_error_value: cpal::StreamError| {
            if let Ok(mut error) = stream_error.lock() {
                *error = Some(stream_error_value.to_string());
            }
        };
        let stream = match supported.sample_format() {
            SampleFormat::F32 => {
                let samples = Arc::clone(&samples);
                let active = Arc::clone(&active);
                device.build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        push_cpal_input(data, channels, &samples, &active, max_input_samples)
                    },
                    error_handler,
                    None,
                )
            }
            SampleFormat::I16 => {
                let samples = Arc::clone(&samples);
                let active = Arc::clone(&active);
                device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        let converted: Vec<f32> = data
                            .iter()
                            .map(|sample| *sample as f32 / i16::MAX as f32)
                            .collect();
                        push_cpal_input(&converted, channels, &samples, &active, max_input_samples)
                    },
                    error_handler,
                    None,
                )
            }
            SampleFormat::U16 => {
                let samples = Arc::clone(&samples);
                let active = Arc::clone(&active);
                device.build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        let converted: Vec<f32> = data
                            .iter()
                            .map(|sample| (*sample as f32 - 32_768.0) / 32_768.0)
                            .collect();
                        push_cpal_input(&converted, channels, &samples, &active, max_input_samples)
                    },
                    error_handler,
                    None,
                )
            }
            format => {
                return Err(VoiceError::new(
                    "capture_format_unsupported",
                    format!("unsupported input sample format: {format:?}"),
                ));
            }
        }
        .map_err(|error| VoiceError::new("capture_stream_failed", error.to_string()))?;
        stream
            .play()
            .map_err(|error| VoiceError::new("capture_stream_start_failed", error.to_string()))?;
        Ok(Self {
            _stream: stream,
            samples,
            active,
            error,
            input_rate,
            max_samples,
        })
    }
}

fn push_cpal_input(
    data: &[f32],
    channels: usize,
    samples: &Arc<Mutex<Vec<f32>>>,
    active: &Arc<AtomicBool>,
    max_input_samples: usize,
) {
    if !active.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut captured) = samples.lock() else {
        return;
    };
    for frame in data.chunks(channels) {
        if captured.len() >= max_input_samples {
            break;
        }
        captured.push(frame.iter().copied().sum::<f32>() / frame.len().max(1) as f32);
    }
}

impl MicrophoneCapture for CpalMicrophone {
    fn start(&mut self) -> Result<(), VoiceError> {
        if self.active.swap(true, Ordering::AcqRel) {
            return Err(VoiceError::new(
                "capture_already_started",
                "microphone is already capturing",
            ));
        }
        self.samples
            .lock()
            .map_err(|_| VoiceError::new("capture_buffer_failed", "capture buffer was poisoned"))?
            .clear();
        *self
            .error
            .lock()
            .map_err(|_| VoiceError::new("capture_error_failed", "capture error was poisoned"))? =
            None;
        Ok(())
    }

    fn finish(&mut self) -> Result<Vec<i16>, VoiceError> {
        if !self.active.swap(false, Ordering::AcqRel) {
            return Err(VoiceError::new(
                "capture_not_started",
                "microphone was not started",
            ));
        }
        let error = self
            .error
            .lock()
            .map_err(|_| VoiceError::new("capture_error_failed", "capture error was poisoned"))?
            .clone();
        if let Some(error) = error {
            return Err(VoiceError::new("capture_failed", error));
        }
        let samples = self
            .samples
            .lock()
            .map_err(|_| VoiceError::new("capture_buffer_failed", "capture buffer was poisoned"))?
            .clone();
        if samples.is_empty() {
            return Err(VoiceError::new(
                "capture_empty",
                "microphone returned no samples",
            ));
        }
        let output = resample_mono(&samples, self.input_rate, VOICE_INPUT_SAMPLE_RATE);
        if output.len() > self.max_samples {
            return Err(VoiceError::new(
                "capture_too_long",
                format!("capture exceeded {} samples", self.max_samples),
            ));
        }
        log_voice_event(format_args!(
            "voice capture: {} samples at {} Hz",
            output.len(),
            VOICE_INPUT_SAMPLE_RATE
        ));
        Ok(output)
    }
}

pub struct CommandSpeechToText {
    spec: CommandSpec,
}

impl CommandSpeechToText {
    pub fn new(spec: CommandSpec) -> Self {
        Self { spec }
    }
}

impl SpeechToText for CommandSpeechToText {
    fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
        let bytes = encode_pcm16(samples);
        let output = run_command(&self.spec, &bytes)?;
        String::from_utf8(output)
            .map(|text| text.trim().to_string())
            .map_err(|error| VoiceError::new("stt_invalid_output", error.to_string()))
    }
}

#[derive(Serialize)]
struct DialogueRequest<'a> {
    context: &'a ResponseContext,
    transcript: &'a str,
}

pub struct CommandDialogueGenerator {
    spec: CommandSpec,
}

impl CommandDialogueGenerator {
    pub fn new(spec: CommandSpec) -> Self {
        Self { spec }
    }
}

#[derive(Deserialize)]
struct DialogueResult {
    dialogue: String,
}

impl DialogueGenerator for CommandDialogueGenerator {
    fn generate(
        &mut self,
        context: &ResponseContext,
        transcript: &str,
    ) -> Result<SubscriberResponse, VoiceError> {
        let input = serde_json::to_vec(&DialogueRequest {
            context,
            transcript,
        })
        .map_err(|error| VoiceError::new("dialogue_request_failed", error.to_string()))?;
        let output = run_command(&self.spec, &input)?;
        serde_json::from_slice::<DialogueResult>(&output)
            .map(|result| SubscriberResponse {
                dialogue: result.dialogue,
            })
            .map_err(|error| VoiceError::new("dialogue_invalid_output", error.to_string()))
    }
}

pub struct Qwen3TtsCommand {
    spec: CommandSpec,
}

impl Qwen3TtsCommand {
    pub fn new(spec: CommandSpec) -> Self {
        Self { spec }
    }
}

#[derive(Serialize)]
struct TtsRequest<'a> {
    engine: &'static str,
    model: &'static str,
    voice_id: &'a str,
    text: &'a str,
    sample_rate: u32,
}

impl TextToSpeech for Qwen3TtsCommand {
    fn synthesize(&mut self, voice_id: &str, text: &str) -> Result<Vec<i16>, VoiceError> {
        let input = serde_json::to_vec(&TtsRequest {
            engine: "qwen3-tts",
            model: "Qwen3-TTS-1.7B",
            voice_id,
            text,
            sample_rate: VOICE_AUDIO_SAMPLE_RATE,
        })
        .map_err(|error| VoiceError::new("tts_request_failed", error.to_string()))?;
        let output = run_command(&self.spec, &input)?;
        let samples = decode_pcm16(&output)?;
        if samples.is_empty() {
            return Err(VoiceError::new(
                "tts_empty_output",
                "Qwen3-TTS returned no audio samples",
            ));
        }
        Ok(samples)
    }

    fn synthesize_stream(
        &mut self,
        voice_id: &str,
        text: &str,
        emit: &mut dyn FnMut(&[i16]) -> Result<(), VoiceError>,
    ) -> Result<usize, VoiceError> {
        let input = serde_json::to_vec(&TtsRequest {
            engine: "qwen3-tts",
            model: "Qwen3-TTS-1.7B",
            voice_id,
            text,
            sample_rate: VOICE_AUDIO_SAMPLE_RATE,
        })
        .map_err(|error| VoiceError::new("tts_request_failed", error.to_string()))?;
        let mut pending = Vec::new();
        let mut emitted = 0;
        run_command_stream(&self.spec, &input, &mut |bytes| {
            pending.extend_from_slice(bytes);
            let complete_length = pending.len() - (pending.len() % 2);
            if complete_length == 0 {
                return Ok(());
            }
            let samples = decode_pcm16(&pending[..complete_length])?;
            pending.drain(..complete_length);
            emitted += samples.len();
            emit(&samples)
        })?;
        if !pending.is_empty() {
            return Err(VoiceError::new(
                "invalid_pcm",
                "Qwen3-TTS streamed an odd number of PCM bytes",
            ));
        }
        Ok(emitted)
    }
}

pub struct PersistentQwen3TtsCommand {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl PersistentQwen3TtsCommand {
    pub fn new(spec: CommandSpec) -> Result<Self, VoiceError> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .arg("--persistent")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        prepare_process_group(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| VoiceError::new("tts_start_failed", error.to_string()))?;
        let input = child.stdin.take().ok_or_else(|| {
            VoiceError::new("tts_stdin_failed", "persistent TTS stdin was unavailable")
        })?;
        let output = child.stdout.take().ok_or_else(|| {
            VoiceError::new("tts_stdout_failed", "persistent TTS stdout was unavailable")
        })?;
        Ok(Self {
            child,
            input,
            output: BufReader::new(output),
        })
    }
}

impl TextToSpeech for PersistentQwen3TtsCommand {
    fn synthesize(&mut self, voice_id: &str, text: &str) -> Result<Vec<i16>, VoiceError> {
        let mut samples = Vec::new();
        self.synthesize_stream(voice_id, text, &mut |chunk| {
            samples.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(samples)
    }

    fn synthesize_stream(
        &mut self,
        voice_id: &str,
        text: &str,
        emit: &mut dyn FnMut(&[i16]) -> Result<(), VoiceError>,
    ) -> Result<usize, VoiceError> {
        if worker_cancellation_requested() {
            return Err(VoiceError::new(
                "worker_cancelled",
                "voice playback was cancelled",
            ));
        }
        let input = serde_json::to_string(&TtsRequest {
            engine: "qwen3-tts",
            model: "Qwen3-TTS-1.7B",
            voice_id,
            text,
            sample_rate: VOICE_AUDIO_SAMPLE_RATE,
        })
        .map_err(|error| VoiceError::new("tts_request_failed", error.to_string()))?;
        self.input
            .write_all(input.as_bytes())
            .and_then(|_| self.input.write_all(b"\n"))
            .and_then(|_| self.input.flush())
            .map_err(|error| VoiceError::new("tts_request_send_failed", error.to_string()))?;

        let mut emitted = 0;
        loop {
            let mut header = [0_u8; 4];
            self.output
                .read_exact(&mut header)
                .map_err(|error| VoiceError::new("tts_stream_read_failed", error.to_string()))?;
            let length = u32::from_le_bytes(header) as usize;
            if length == 0 {
                break;
            }
            if length > MAX_WORKER_OUTPUT_BYTES || length % 2 != 0 {
                return Err(VoiceError::new(
                    "invalid_pcm",
                    "persistent Qwen3-TTS returned an invalid PCM frame",
                ));
            }
            let mut bytes = vec![0_u8; length];
            self.output
                .read_exact(&mut bytes)
                .map_err(|error| VoiceError::new("tts_stream_read_failed", error.to_string()))?;
            let samples = decode_pcm16(&bytes)?;
            emitted += samples.len();
            emit(&samples)?;
        }
        Ok(emitted)
    }
}

impl Drop for PersistentQwen3TtsCommand {
    fn drop(&mut self) {
        terminate_process_group(&mut self.child);
        let _ = self.child.wait();
    }
}

pub struct UdpVoiceOutput {
    socket: UdpSocket,
    sequence: u16,
    timestamp: u32,
    ssrc: u32,
}

impl UdpVoiceOutput {
    pub fn connect(
        backend_address: &str,
        session_id: u64,
        state_revision: u64,
    ) -> Result<Self, VoiceError> {
        let socket = UdpSocket::bind("0.0.0.0:0")
            .and_then(|socket| {
                socket.connect(backend_address)?;
                Ok(socket)
            })
            .map_err(|error| VoiceError::new("voice_udp_setup_failed", error.to_string()))?;
        Ok(Self {
            socket,
            sequence: 0,
            timestamp: state_revision as u32,
            ssrc: 0x4e4e_0000 ^ session_id as u32,
        })
    }

    pub fn try_clone(&self) -> Result<Self, VoiceError> {
        Ok(Self {
            socket: self
                .socket
                .try_clone()
                .map_err(|error| VoiceError::new("voice_udp_setup_failed", error.to_string()))?,
            sequence: self.sequence,
            timestamp: self.timestamp,
            ssrc: self.ssrc,
        })
    }

    pub fn receive_control(&self) -> Result<VoiceControlMessage, VoiceError> {
        let mut datagram = [0_u8; 512];
        let length = self
            .socket
            .recv(&mut datagram)
            .map_err(|error| VoiceError::new("voice_control_receive_failed", error.to_string()))?;
        decode_voice_control(&datagram[..length])
            .map_err(|error| VoiceError::new("voice_control_decode_failed", error.to_string()))
    }

    pub fn receive_datagram(&self) -> Result<Vec<u8>, VoiceError> {
        let mut datagram = vec![0_u8; 65_535];
        let length = match self.socket.recv(&mut datagram) {
            Ok(length) => length,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(VoiceError::new(
                    "voice_datagram_timeout",
                    "voice relay receive timed out",
                ));
            }
            Err(error) => {
                return Err(VoiceError::new(
                    "voice_datagram_receive_failed",
                    error.to_string(),
                ));
            }
        };
        datagram.truncate(length);
        Ok(datagram)
    }

    pub fn set_receive_timeout(&self, timeout: Duration) -> Result<(), VoiceError> {
        self.socket
            .set_read_timeout(Some(timeout))
            .map_err(|error| VoiceError::new("voice_timeout_setup_failed", error.to_string()))
    }

    pub fn send_input_audio(&self, message: &VoiceInputAudioMessage) -> Result<(), VoiceError> {
        let datagram = encode_voice_input_audio(message)
            .map_err(|error| VoiceError::new("voice_input_encode_failed", error.to_string()))?;
        self.socket
            .send(&datagram)
            .map_err(|error| VoiceError::new("voice_input_send_failed", error.to_string()))?;
        Ok(())
    }
}

impl VoiceOutput for UdpVoiceOutput {
    fn status(&mut self, message: VoiceStatusMessage) -> Result<(), VoiceError> {
        let datagram = encode_voice_status(&message)
            .map_err(|error| VoiceError::new("voice_status_encode_failed", error.to_string()))?;
        self.socket
            .send(&datagram)
            .map_err(|error| VoiceError::new("voice_status_send_failed", error.to_string()))?;
        Ok(())
    }

    fn audio(&mut self, mut packet: RtpL16Packet) -> Result<(), VoiceError> {
        packet.sequence = self.sequence;
        packet.timestamp = self.timestamp;
        packet.ssrc = self.ssrc;
        self.sequence = self.sequence.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(packet.samples.len() as u32);
        self.socket
            .send(&packet.encode())
            .map_err(|error| VoiceError::new("voice_audio_send_failed", error.to_string()))?;
        Ok(())
    }
}

pub trait AudioPlayback {
    fn write(&mut self, samples: &[i16]) -> Result<(), VoiceError>;
    fn finish(&mut self) -> Result<(), VoiceError>;
}

pub struct CommandAudioPlayback {
    child: Child,
    input: Option<ChildStdin>,
    timeout: Duration,
}

pub struct CpalAudioPlayback {
    _stream: Stream,
    queue: Arc<Mutex<VecDeque<f32>>>,
    error: Arc<Mutex<Option<String>>>,
    output_rate: u32,
    timeout: Duration,
}

impl CpalAudioPlayback {
    pub fn new(timeout: Duration) -> Result<Self, VoiceError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            VoiceError::new(
                "playback_device_unavailable",
                "no default audio output device is available",
            )
        })?;
        log_voice_event(format_args!(
            "voice output device: {}",
            device.name().unwrap_or_else(|_| "<unnamed>".to_string())
        ));
        let supported = device
            .default_output_config()
            .map_err(|error| VoiceError::new("playback_config_unavailable", error.to_string()))?;
        let output_rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let config = supported.config();
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let error = Arc::new(Mutex::new(None));
        let stream_error = Arc::clone(&error);
        let error_handler = move |stream_error_value: cpal::StreamError| {
            if let Ok(mut error) = stream_error.lock() {
                *error = Some(stream_error_value.to_string());
            }
        };
        let stream = match supported.sample_format() {
            SampleFormat::F32 => {
                let queue = Arc::clone(&queue);
                device.build_output_stream(
                    &config,
                    move |data: &mut [f32], _| fill_cpal_output(data, channels, &queue),
                    error_handler,
                    None,
                )
            }
            SampleFormat::I16 => {
                let queue = Arc::clone(&queue);
                device.build_output_stream(
                    &config,
                    move |data: &mut [i16], _| {
                        let mut mono = vec![0.0; data.len() / channels.max(1)];
                        fill_cpal_output(&mut mono, 1, &queue);
                        for (frame, sample) in data.chunks_mut(channels).zip(mono) {
                            let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                            frame.fill(value);
                        }
                    },
                    error_handler,
                    None,
                )
            }
            SampleFormat::U16 => {
                let queue = Arc::clone(&queue);
                device.build_output_stream(
                    &config,
                    move |data: &mut [u16], _| {
                        let mut mono = vec![0.0; data.len() / channels.max(1)];
                        fill_cpal_output(&mut mono, 1, &queue);
                        for (frame, sample) in data.chunks_mut(channels).zip(mono) {
                            let value = ((sample.clamp(-1.0, 1.0) + 1.0) * 32_767.5) as u16;
                            frame.fill(value);
                        }
                    },
                    error_handler,
                    None,
                )
            }
            format => {
                return Err(VoiceError::new(
                    "playback_format_unsupported",
                    format!("unsupported output sample format: {format:?}"),
                ));
            }
        }
        .map_err(|error| VoiceError::new("playback_stream_failed", error.to_string()))?;
        stream
            .play()
            .map_err(|error| VoiceError::new("playback_stream_start_failed", error.to_string()))?;
        Ok(Self {
            _stream: stream,
            queue,
            error,
            output_rate,
            timeout,
        })
    }

    fn write(&mut self, samples: &[i16]) -> Result<(), VoiceError> {
        self.check_error()?;
        let input: Vec<f32> = samples
            .iter()
            .map(|sample| *sample as f32 / i16::MAX as f32)
            .collect();
        let resampled = resample_f32(&input, VOICE_AUDIO_SAMPLE_RATE, self.output_rate);
        self.queue
            .lock()
            .map_err(|_| VoiceError::new("playback_queue_failed", "playback queue was poisoned"))?
            .extend(resampled.into_iter());
        Ok(())
    }

    fn finish(&mut self) -> Result<(), VoiceError> {
        let deadline = Instant::now() + self.timeout;
        loop {
            self.check_error()?;
            let empty = self
                .queue
                .lock()
                .map_err(|_| {
                    VoiceError::new("playback_queue_failed", "playback queue was poisoned")
                })?
                .is_empty();
            if empty {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(VoiceError::new(
                    "audio_playback_timeout",
                    "audio playback exceeded its deadline",
                ));
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn check_error(&self) -> Result<(), VoiceError> {
        let error = self
            .error
            .lock()
            .map_err(|_| VoiceError::new("playback_error_failed", "playback error was poisoned"))?
            .clone();
        if let Some(error) = error {
            return Err(VoiceError::new("audio_playback_failed", error));
        }
        Ok(())
    }
}

impl AudioPlayback for CommandAudioPlayback {
    fn write(&mut self, samples: &[i16]) -> Result<(), VoiceError> {
        Self::write(self, samples)
    }

    fn finish(&mut self) -> Result<(), VoiceError> {
        Self::finish(self)
    }
}

fn fill_cpal_output(data: &mut [f32], channels: usize, queue: &Arc<Mutex<VecDeque<f32>>>) {
    let Ok(mut queue) = queue.lock() else {
        data.fill(0.0);
        return;
    };
    for frame in data.chunks_mut(channels.max(1)) {
        let sample = queue.pop_front().unwrap_or(0.0);
        frame.fill(sample);
    }
}

fn resample_mono(samples: &[f32], input_rate: u32, output_rate: u32) -> Vec<i16> {
    resample_f32(samples, input_rate, output_rate)
        .into_iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

fn resample_f32(samples: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let output_length = (samples.len() as u64 * output_rate as u64 / input_rate as u64) as usize;
    (0..output_length)
        .map(|index| {
            let position = index as f64 * input_rate as f64 / output_rate as f64;
            let lower = position.floor() as usize;
            let upper = (lower + 1).min(samples.len() - 1);
            let fraction = position - lower as f64;
            let sample =
                samples[lower] as f64 * (1.0 - fraction) + samples[upper] as f64 * fraction;
            sample.clamp(-1.0, 1.0) as f32
        })
        .collect()
}

impl CommandAudioPlayback {
    pub fn new(spec: CommandSpec) -> Result<Self, VoiceError> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        prepare_process_group(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| VoiceError::new("audio_playback_start_failed", error.to_string()))?;
        let input = child.stdin.take().ok_or_else(|| {
            VoiceError::new(
                "audio_playback_stdin_failed",
                "audio playback stdin was unavailable",
            )
        })?;
        Ok(Self {
            child,
            input: Some(input),
            timeout: spec.timeout,
        })
    }

    fn write(&mut self, samples: &[i16]) -> Result<(), VoiceError> {
        if worker_cancellation_requested() {
            return Err(VoiceError::new(
                "worker_cancelled",
                "voice playback was cancelled",
            ));
        }
        let input = self.input.as_mut().ok_or_else(|| {
            VoiceError::new(
                "audio_playback_finished",
                "audio playback is already finished",
            )
        })?;
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        input
            .write_all(&bytes)
            .map_err(|error| VoiceError::new("audio_playback_write_failed", error.to_string()))
    }

    fn finish(&mut self) -> Result<(), VoiceError> {
        if worker_cancellation_requested() {
            terminate_process_group(&mut self.child);
            let _ = self.child.wait();
            self.input.take();
            return Err(VoiceError::new(
                "worker_cancelled",
                "voice playback was cancelled",
            ));
        }
        self.input.take();
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            if worker_cancellation_requested() {
                terminate_process_group(&mut self.child);
                let _ = self.child.wait();
                return Err(VoiceError::new(
                    "worker_cancelled",
                    "voice playback was cancelled",
                ));
            }
            match self.child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
                Ok(None) => {
                    terminate_process_group(&mut self.child);
                    let _ = self.child.wait();
                    return Err(VoiceError::new(
                        "audio_playback_timeout",
                        "audio playback exceeded its deadline",
                    ));
                }
                Err(error) => {
                    terminate_process_group(&mut self.child);
                    let _ = self.child.wait();
                    return Err(VoiceError::new(
                        "audio_playback_wait_failed",
                        error.to_string(),
                    ));
                }
            }
        };
        if !status.success() {
            return Err(VoiceError::new(
                "audio_playback_failed",
                format!("audio playback exited with {status}"),
            ));
        }
        Ok(())
    }
}

pub struct TeeVoiceOutput {
    network: UdpVoiceOutput,
    playback: Box<dyn AudioPlayback>,
}

impl TeeVoiceOutput {
    pub fn new(network: UdpVoiceOutput, playback: Box<dyn AudioPlayback>) -> Self {
        Self { network, playback }
    }
}

impl VoiceOutput for TeeVoiceOutput {
    fn status(&mut self, message: VoiceStatusMessage) -> Result<(), VoiceError> {
        self.network.status(message)
    }

    fn audio(&mut self, packet: RtpL16Packet) -> Result<(), VoiceError> {
        self.network.audio(packet.clone())?;
        self.playback.write(&packet.samples)
    }

    fn finish(&mut self) -> Result<(), VoiceError> {
        self.playback.finish()
    }
}

impl AudioPlayback for CpalAudioPlayback {
    fn write(&mut self, samples: &[i16]) -> Result<(), VoiceError> {
        Self::write(self, samples)
    }

    fn finish(&mut self) -> Result<(), VoiceError> {
        Self::finish(self)
    }
}

fn encode_pcm16(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect()
}

fn decode_pcm16(bytes: &[u8]) -> Result<Vec<i16>, VoiceError> {
    if bytes.len() % 2 != 0 {
        return Err(VoiceError::new(
            "invalid_pcm",
            "PCM output must contain an even number of bytes",
        ));
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect())
}

fn run_command(spec: &CommandSpec, input: &[u8]) -> Result<Vec<u8>, VoiceError> {
    if worker_cancellation_requested() {
        return Err(VoiceError::new(
            "worker_cancelled",
            "voice worker was cancelled before it started",
        ));
    }
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    prepare_process_group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| VoiceError::new("worker_start_failed", error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| VoiceError::new("worker_stdout_failed", "worker stdout was unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| VoiceError::new("worker_stderr_failed", "worker stderr was unavailable"))?;
    let stdout_thread = thread::spawn(move || -> Result<Vec<u8>, VoiceError> {
        read_bounded(stdout, MAX_WORKER_OUTPUT_BYTES + 1, "worker_stdout_failed")
    });
    let stderr_thread = thread::spawn(move || -> Result<Vec<u8>, VoiceError> {
        read_bounded(stderr, 16 * 1024, "worker_stderr_failed")
    });
    let mut stdin_thread = child.stdin.take().map(|mut stdin| {
        let input = input.to_vec();
        thread::spawn(move || stdin.write_all(&input))
    });
    let deadline = Instant::now() + spec.timeout;
    loop {
        if worker_cancellation_requested() {
            terminate_process_group(&mut child);
            let _ = child.wait();
            if let Some(stdin_thread) = stdin_thread.take() {
                let _ = stdin_thread.join();
            }
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Err(VoiceError::new(
                "worker_cancelled",
                "voice worker was cancelled",
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if let Some(stdin_thread) = stdin_thread.take() {
                    stdin_thread
                        .join()
                        .map_err(|_| {
                            VoiceError::new("worker_stdin_failed", "stdin writer panicked")
                        })?
                        .map_err(|error| {
                            VoiceError::new("worker_stdin_failed", error.to_string())
                        })?;
                }
                let output = stdout_thread.join().map_err(|_| {
                    VoiceError::new("worker_stdout_failed", "stdout reader panicked")
                })??;
                let diagnostics = stderr_thread.join().map_err(|_| {
                    VoiceError::new("worker_stderr_failed", "stderr reader panicked")
                })??;
                if !status.success() {
                    return Err(VoiceError::new(
                        "worker_failed",
                        format!(
                            "worker exited with {status}: {}",
                            diagnostic_text(&diagnostics)
                        ),
                    ));
                }
                if output.len() > MAX_WORKER_OUTPUT_BYTES {
                    return Err(VoiceError::new(
                        "worker_output_too_large",
                        "voice worker output exceeded the bounded output limit",
                    ));
                }
                return Ok(output);
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                terminate_process_group(&mut child);
                let _ = child.wait();
                if let Some(stdin_thread) = stdin_thread.take() {
                    let _ = stdin_thread.join();
                }
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(VoiceError::new(
                    "worker_timeout",
                    "voice worker exceeded its deadline",
                ));
            }
            Err(error) => {
                terminate_process_group(&mut child);
                let _ = child.wait();
                if let Some(stdin_thread) = stdin_thread.take() {
                    let _ = stdin_thread.join();
                }
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(VoiceError::new("worker_wait_failed", error.to_string()));
            }
        }
    }
}

fn run_command_stream(
    spec: &CommandSpec,
    input: &[u8],
    emit: &mut dyn FnMut(&[u8]) -> Result<(), VoiceError>,
) -> Result<usize, VoiceError> {
    if worker_cancellation_requested() {
        return Err(VoiceError::new(
            "worker_cancelled",
            "voice worker was cancelled before it started",
        ));
    }
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    prepare_process_group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| VoiceError::new("worker_start_failed", error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| VoiceError::new("worker_stdout_failed", "worker stdout was unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| VoiceError::new("worker_stderr_failed", "worker stderr was unavailable"))?;
    let (sender, receiver) = mpsc::channel();
    let stdout_thread = thread::spawn(move || {
        let mut stdout = stdout;
        let mut buffer = [0_u8; 16 * 1024];
        let mut total = 0;
        loop {
            let length = match stdout.read(&mut buffer) {
                Ok(length) => length,
                Err(error) => {
                    let _ = sender.send(Err(VoiceError::new(
                        "worker_stdout_failed",
                        error.to_string(),
                    )));
                    return;
                }
            };
            if length == 0 {
                let _ = sender.send(Ok(None));
                return;
            }
            total += length;
            if total > MAX_WORKER_OUTPUT_BYTES {
                let _ = sender.send(Err(VoiceError::new(
                    "worker_output_too_large",
                    "voice worker output exceeded the bounded output limit",
                )));
                return;
            }
            if sender.send(Ok(Some(buffer[..length].to_vec()))).is_err() {
                return;
            }
        }
    });
    let stderr_thread =
        thread::spawn(move || read_bounded(stderr, 16 * 1024, "worker_stderr_failed"));
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(input) {
            terminate_process_group(&mut child);
            let _ = child.wait();
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
            return Err(VoiceError::new("worker_stdin_failed", error.to_string()));
        }
    }
    let deadline = Instant::now() + spec.timeout;
    let mut total = 0;
    let result = loop {
        if worker_cancellation_requested() {
            terminate_process_group(&mut child);
            let _ = child.wait();
            break Err(VoiceError::new(
                "worker_cancelled",
                "voice worker was cancelled",
            ));
        }
        match receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(Ok(Some(bytes))) => {
                total += bytes.len();
                if let Err(error) = emit(&bytes) {
                    terminate_process_group(&mut child);
                    let _ = child.wait();
                    break Err(error);
                }
            }
            Ok(Ok(None)) => break Ok(()),
            Ok(Err(error)) => {
                terminate_process_group(&mut child);
                let _ = child.wait();
                break Err(error);
            }
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                terminate_process_group(&mut child);
                let _ = child.wait();
                break Err(VoiceError::new(
                    "worker_timeout",
                    "voice worker exceeded its deadline",
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err(VoiceError::new(
                    "worker_stdout_failed",
                    "voice worker stdout reader disconnected",
                ));
            }
        }
    };
    let status = if result.is_ok() {
        child
            .wait()
            .map_err(|error| VoiceError::new("worker_wait_failed", error.to_string()))?
    } else {
        let _ = child.wait();
        return result.map(|_| total);
    };
    let _ = stdout_thread.join();
    let diagnostics = stderr_thread
        .join()
        .map_err(|_| VoiceError::new("worker_stderr_failed", "stderr reader panicked"))??;
    if !status.success() {
        return Err(VoiceError::new(
            "worker_failed",
            format!(
                "worker exited with {status}: {}",
                diagnostic_text(&diagnostics)
            ),
        ));
    }
    Ok(total)
}

fn read_bounded<R: Read>(
    mut reader: R,
    limit: usize,
    error_code: &str,
) -> Result<Vec<u8>, VoiceError> {
    let mut captured = Vec::with_capacity(limit);
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let length = reader
            .read(&mut buffer)
            .map_err(|error| VoiceError::new(error_code, error.to_string()))?;
        if length == 0 {
            return Ok(captured);
        }
        let remaining = limit.saturating_sub(captured.len());
        captured.extend_from_slice(&buffer[..length.min(remaining)]);
    }
}

fn diagnostic_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use exchange_protocol::RtpL16Packet;

    struct FakeCapture;
    impl MicrophoneCapture for FakeCapture {
        fn start(&mut self) -> Result<(), VoiceError> {
            Ok(())
        }
        fn finish(&mut self) -> Result<Vec<i16>, VoiceError> {
            Ok(vec![1, 2, 3])
        }
    }

    struct FakeStt;
    impl SpeechToText for FakeStt {
        fn transcribe(&mut self, samples: &[i16]) -> Result<String, VoiceError> {
            assert_eq!(samples, &[1, 2, 3]);
            Ok("connect me to Vira".to_string())
        }
    }

    struct FakeDialogue;
    impl DialogueGenerator for FakeDialogue {
        fn generate(
            &mut self,
            context: &ResponseContext,
            transcript: &str,
        ) -> Result<SubscriberResponse, VoiceError> {
            assert_eq!(transcript, "connect me to Vira");
            assert_eq!(context.profile.name, "Taren Kesh");
            assert_eq!(context.current_input.as_deref(), Some("connect me to Vira"));
            Ok(SubscriberResponse {
                dialogue: "I am listening.".to_string(),
            })
        }
    }

    struct FakeTts;
    impl TextToSpeech for FakeTts {
        fn synthesize(&mut self, voice_id: &str, text: &str) -> Result<Vec<i16>, VoiceError> {
            assert_eq!(voice_id, "Ryan");
            assert_eq!(text, "I am listening.");
            Ok(vec![7; VOICE_AUDIO_PACKET_SAMPLES + 1])
        }
    }

    #[derive(Default)]
    struct FakeOutput {
        statuses: Vec<VoiceStatusMessage>,
        packets: Vec<RtpL16Packet>,
    }

    impl VoiceOutput for FakeOutput {
        fn status(&mut self, message: VoiceStatusMessage) -> Result<(), VoiceError> {
            self.statuses.push(message);
            Ok(())
        }
        fn audio(&mut self, packet: RtpL16Packet) -> Result<(), VoiceError> {
            self.packets.push(packet);
            Ok(())
        }
    }

    fn context() -> ResponseContext {
        ResponseContext {
            profile: SubscriberProfile {
                subscriber_id: 0,
                name: "Taren Kesh".to_string(),
                voice_id: "Ryan".to_string(),
                personality: "precise railway dispatcher".to_string(),
                baseline_goals: vec!["Keep the railway moving".to_string()],
                initial_perspective: "The exchange is under observation".to_string(),
                relationships: vec![RelationshipNote {
                    subject: "Vira Dhal".to_string(),
                    note: "A trusted records clerk".to_string(),
                }],
                permitted_actions: vec!["request_routing".to_string()],
            },
            subscriber_goal: "Reach the requested Callee".to_string(),
            call_premise: "A railway dispatch is waiting".to_string(),
            story_beat_direction: "Ask for an ordinary connection".to_string(),
            permitted_knowledge: vec![KnowledgeRecord {
                fact: "The directory lists Vira Dhal".to_string(),
                learned_from: "directory_terminal".to_string(),
            }],
            beliefs: Vec::new(),
            relationship_notes: Vec::new(),
            memories: Vec::new(),
            recent_conversation: Vec::new(),
            current_input: None,
        }
    }

    #[test]
    fn one_ptt_release_runs_a_bounded_subscriber_response() {
        let output = Rc::new(RefCell::new(FakeOutput::default()));
        let output_ref = Rc::clone(&output);
        struct SharedOutput(Rc<RefCell<FakeOutput>>);
        impl VoiceOutput for SharedOutput {
            fn status(&mut self, message: VoiceStatusMessage) -> Result<(), VoiceError> {
                self.0.borrow_mut().status(message)
            }
            fn audio(&mut self, packet: RtpL16Packet) -> Result<(), VoiceError> {
                self.0.borrow_mut().audio(packet)
            }
        }
        let mut session = OperatorSession::new(
            9,
            14,
            context(),
            Box::new(FakeCapture),
            Box::new(FakeStt),
            Box::new(FakeDialogue),
            Box::new(FakeTts),
            Box::new(SharedOutput(output_ref)),
        )
        .unwrap();

        session.start_ptt().unwrap();
        let response = session.release_ptt().unwrap();

        assert_eq!(response.dialogue, "I am listening.");
        assert_eq!(session.phase(), SessionPhase::Completed);
        let output_snapshot = output.borrow();
        assert_eq!(output_snapshot.statuses[0].status, VoiceStatus::Listening);
        assert_eq!(
            output_snapshot.statuses.last().unwrap().status,
            VoiceStatus::Completed
        );
        assert_eq!(output_snapshot.packets.len(), 2);
        assert!(output_snapshot.packets[0].marker);
        assert_eq!(
            output_snapshot.packets[1].timestamp,
            VOICE_AUDIO_PACKET_SAMPLES as u32
        );
        drop(output_snapshot);

        session.prepare_next_turn().unwrap();
        assert_eq!(session.phase(), SessionPhase::Ready);
        session.start_ptt().unwrap();
        session.release_ptt().unwrap();
        let output = output.borrow();
        assert_eq!(
            output.statuses.last().unwrap().status,
            VoiceStatus::Completed
        );
        assert_eq!(output.statuses.last().unwrap().turn_id, 2);
    }

    #[test]
    fn provider_failure_is_visible_without_a_successful_response() {
        struct FailingStt;
        impl SpeechToText for FailingStt {
            fn transcribe(&mut self, _: &[i16]) -> Result<String, VoiceError> {
                Err(VoiceError::new("stt_failed", "local recognizer stopped"))
            }
        }
        let output = Rc::new(RefCell::new(FakeOutput::default()));
        let output_ref = Rc::clone(&output);
        struct SharedOutput(Rc<RefCell<FakeOutput>>);
        impl VoiceOutput for SharedOutput {
            fn status(&mut self, message: VoiceStatusMessage) -> Result<(), VoiceError> {
                self.0.borrow_mut().status(message)
            }
            fn audio(&mut self, packet: RtpL16Packet) -> Result<(), VoiceError> {
                self.0.borrow_mut().audio(packet)
            }
        }
        let mut session = OperatorSession::new(
            9,
            14,
            context(),
            Box::new(FakeCapture),
            Box::new(FailingStt),
            Box::new(FakeDialogue),
            Box::new(FakeTts),
            Box::new(SharedOutput(output_ref)),
        )
        .unwrap();

        session.start_ptt().unwrap();
        let error = session.release_ptt().unwrap_err();

        assert_eq!(error.code, "stt_failed");
        assert_eq!(session.phase(), SessionPhase::Failed);
        let output = output.borrow();
        let failure = output.statuses.last().unwrap();
        assert_eq!(failure.status, VoiceStatus::Failed);
        assert_eq!(failure.error.as_ref().unwrap().code, "stt_failed");
        assert!(output.packets.is_empty());
    }

    #[test]
    fn response_context_rejects_unbounded_context() {
        let mut context = context();
        context.memories.push(MemoryRecord {
            summary: "x".repeat(MAX_RESPONSE_CONTEXT_TOKENS * 4),
            source: "test".to_string(),
        });
        assert_eq!(
            context.validate().unwrap_err().code,
            "response_context_too_large"
        );
    }

    #[test]
    fn local_worker_timeout_kills_the_child() {
        let mut spec = CommandSpec::new("sh", vec!["-c".to_string(), "sleep 1".to_string()]);
        spec.timeout = Duration::from_millis(100);

        let error = run_command(&spec, b"").unwrap_err();

        assert_eq!(error.code, "worker_timeout");
    }

    #[test]
    fn microphone_capture_is_bounded_and_decodes_signed_pcm() {
        let spec = CommandSpec::new(
            "sh",
            vec![
                "-c".to_string(),
                "printf '\\001\\000\\002\\000'; sleep 1".to_string(),
            ],
        );
        let mut capture = CommandMicrophone::with_max_samples(spec, 2);

        capture.start().unwrap();
        thread::sleep(Duration::from_millis(25));
        assert_eq!(capture.finish().unwrap(), vec![1, 2]);
    }

    #[test]
    fn microphone_capture_rejects_audio_beyond_the_bound() {
        let spec = CommandSpec::new(
            "sh",
            vec![
                "-c".to_string(),
                "printf '\\001\\000\\002\\000\\003\\000'".to_string(),
            ],
        );
        let mut capture = CommandMicrophone::with_max_samples(spec, 2);

        capture.start().unwrap();
        thread::sleep(Duration::from_millis(25));
        let error = capture.finish().unwrap_err();

        assert_eq!(error.code, "capture_too_long");
    }

    #[test]
    fn microphone_capture_rejects_an_independent_nonzero_exit() {
        let spec = CommandSpec::new(
            "sh",
            vec![
                "-c".to_string(),
                "printf '\\001\\000' ; printf 'capture failed' >&2; exit 3".to_string(),
            ],
        );
        let mut capture = CommandMicrophone::with_max_samples(spec, 2);

        capture.start().unwrap();
        thread::sleep(Duration::from_millis(25));
        let error = capture.finish().unwrap_err();

        assert_eq!(error.code, "capture_failed");
        assert!(error.message.contains("capture failed"));
    }

    #[test]
    fn failed_worker_preserves_stderr_diagnostics() {
        let spec = CommandSpec::new(
            "sh",
            vec![
                "-c".to_string(),
                "printf 'model missing' >&2; exit 3".to_string(),
            ],
        );

        let error = run_command(&spec, b"").unwrap_err();

        assert_eq!(error.code, "worker_failed");
        assert!(error.message.contains("model missing"));
    }

    #[test]
    fn tts_rejects_empty_audio_output() {
        let spec = CommandSpec::new("sh", vec!["-c".to_string(), "cat >/dev/null".to_string()]);
        let mut tts = Qwen3TtsCommand::new(spec);

        let error = tts.synthesize("taren", "hello").unwrap_err();

        assert_eq!(error.code, "tts_empty_output");
    }

    #[test]
    fn command_workers_accept_contract_fixtures_without_model_files() {
        let mut stt = CommandSpeechToText::new(CommandSpec::new(
            "sh",
            vec![
                "-c".to_string(),
                "cat >/dev/null; printf 'fixture transcript'".to_string(),
            ],
        ));
        assert_eq!(
            stt.transcribe(&[1, -2]),
            Ok("fixture transcript".to_string())
        );

        let mut dialogue = CommandDialogueGenerator::new(CommandSpec::new(
            "sh",
            vec![
                "-c".to_string(),
                "cat >/dev/null; printf '{\"dialogue\":\"fixture response\"}'".to_string(),
            ],
        ));
        assert_eq!(
            dialogue.generate(&context(), "fixture transcript"),
            Ok(SubscriberResponse {
                dialogue: "fixture response".to_string(),
            })
        );

        let mut tts = Qwen3TtsCommand::new(CommandSpec::new(
            "sh",
            vec![
                "-c".to_string(),
                "cat >/dev/null; printf '\\001\\000\\002\\000'".to_string(),
            ],
        ));
        assert_eq!(tts.synthesize("taren", "fixture response"), Ok(vec![1, 2]));
    }

    #[test]
    fn operator_session_cancel_is_visible_before_provider_work() {
        struct Output(Vec<VoiceStatusMessage>);
        impl VoiceOutput for Output {
            fn status(&mut self, message: VoiceStatusMessage) -> Result<(), VoiceError> {
                self.0.push(message);
                Ok(())
            }
            fn audio(&mut self, _: RtpL16Packet) -> Result<(), VoiceError> {
                Ok(())
            }
        }

        let mut session = OperatorSession::new(
            1,
            1,
            context(),
            Box::new(FakeCapture),
            Box::new(FakeStt),
            Box::new(FakeDialogue),
            Box::new(FakeTts),
            Box::new(Output(Vec::new())),
        )
        .unwrap();

        session.cancel().unwrap();

        assert_eq!(session.phase(), SessionPhase::Cancelled);
    }

    #[test]
    fn worker_output_is_bounded_without_deadlocking_the_child() {
        let spec = CommandSpec::new(
            "sh",
            vec![
                "-c".to_string(),
                format!(
                    "dd if=/dev/zero bs=1 count={} 2>/dev/null",
                    MAX_WORKER_OUTPUT_BYTES + 1
                ),
            ],
        );

        let error = run_command(&spec, b"").unwrap_err();

        assert_eq!(error.code, "worker_output_too_large");
    }

    #[test]
    fn local_audio_playback_consumes_little_endian_pcm_and_finishes() {
        let spec = CommandSpec::new("sh", vec!["-c".to_string(), "cat >/dev/null".to_string()]);
        let mut playback = CommandAudioPlayback::new(spec).unwrap();

        playback.write(&[1, -2]).unwrap();
        playback.finish().unwrap();
    }
}
