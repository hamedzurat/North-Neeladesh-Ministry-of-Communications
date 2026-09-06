use std::io::{Read, Write};
use std::net::UdpSocket;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use exchange_protocol::{
    ProtocolError, RtpL16Packet, VOICE_AUDIO_PACKET_SAMPLES, VOICE_AUDIO_SAMPLE_RATE,
    VOICE_PROTOCOL_VERSION, VoiceControlMessage, VoiceStatus, VoiceStatusMessage,
    decode_voice_control, encode_voice_status,
};
use serde::{Deserialize, Serialize};

pub const MAX_RESPONSE_CONTEXT_TOKENS: usize = 3_072;
const MAX_DIALOGUE_CHARS: usize = 2_000;

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
}

pub trait VoiceOutput {
    fn status(&mut self, message: VoiceStatusMessage) -> Result<(), VoiceError>;
    fn audio(&mut self, packet: RtpL16Packet) -> Result<(), VoiceError>;
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

    pub fn start_ptt(&mut self) -> Result<(), VoiceError> {
        if self.phase != SessionPhase::Ready {
            return Err(VoiceError::new(
                "invalid_session_phase",
                "PTT can only start a ready Operator Session",
            ));
        }
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
        let samples = match self
            .tts
            .synthesize(&context.profile.voice_id, &response.dialogue)
        {
            Ok(samples) => samples,
            Err(error) => return self.fail(error),
        };
        self.emit(
            VoiceStatus::Playing,
            Some(&transcript),
            Some(&response.dialogue),
            None,
        )?;
        if let Err(error) = self.send_audio(&samples) {
            return self.fail(error);
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

    fn send_audio(&mut self, samples: &[i16]) -> Result<(), VoiceError> {
        for (index, chunk) in samples.chunks(VOICE_AUDIO_PACKET_SAMPLES).enumerate() {
            self.output.audio(RtpL16Packet {
                marker: index == 0,
                sequence: index as u16,
                timestamp: (index * VOICE_AUDIO_PACKET_SAMPLES) as u32,
                ssrc: self.session_id as u32,
                samples: chunk.to_vec(),
            })?;
        }
        Ok(())
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
        self.phase = SessionPhase::Failed;
        self.emit_failure(&error);
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
    child: Option<Child>,
}

impl CommandMicrophone {
    pub fn new(spec: CommandSpec) -> Self {
        Self { spec, child: None }
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
        let child = Command::new(&self.spec.program)
            .args(&self.spec.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| VoiceError::new("capture_start_failed", error.to_string()))?;
        self.child = Some(child);
        Ok(())
    }

    fn finish(&mut self) -> Result<Vec<i16>, VoiceError> {
        let Some(mut child) = self.child.take() else {
            return Err(VoiceError::new(
                "capture_not_started",
                "microphone was not started",
            ));
        };
        let _ = child.kill();
        let mut bytes = Vec::new();
        if let Some(mut stdout) = child.stdout.take() {
            stdout
                .read_to_end(&mut bytes)
                .map_err(|error| VoiceError::new("capture_read_failed", error.to_string()))?;
        }
        let _ = child.wait();
        decode_pcm16(&bytes)
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
        decode_pcm16(&output)
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
                socket.set_nonblocking(true)?;
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

    pub fn try_receive_control(&self) -> Result<Option<VoiceControlMessage>, VoiceError> {
        let mut datagram = [0_u8; 512];
        match self.socket.recv(&mut datagram) {
            Ok(length) => decode_voice_control(&datagram[..length])
                .map(Some)
                .map_err(|error| VoiceError::new("voice_control_decode_failed", error.to_string())),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => Err(VoiceError::new(
                "voice_control_receive_failed",
                error.to_string(),
            )),
        }
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
    let mut child = Command::new(&spec.program)
        .args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| VoiceError::new("worker_start_failed", error.to_string()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| VoiceError::new("worker_stdout_failed", "worker stdout was unavailable"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| VoiceError::new("worker_stderr_failed", "worker stderr was unavailable"))?;
    let stdout_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let stderr_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        bytes
    });
    let mut stdin_thread = child.stdin.take().map(|mut stdin| {
        let input = input.to_vec();
        thread::spawn(move || stdin.write_all(&input))
    });
    let deadline = Instant::now() + spec.timeout;
    loop {
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
                })?;
                let _ = stderr_thread.join();
                if !status.success() {
                    return Err(VoiceError::new(
                        "worker_failed",
                        format!("worker exited with {status}"),
                    ));
                }
                return Ok(output);
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
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
                let _ = child.kill();
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
            assert_eq!(voice_id, "taren");
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
                voice_id: "taren".to_string(),
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
        let output = output.borrow();
        assert_eq!(output.statuses[0].status, VoiceStatus::Listening);
        assert_eq!(
            output.statuses.last().unwrap().status,
            VoiceStatus::Completed
        );
        assert_eq!(output.packets.len(), 2);
        assert!(output.packets[0].marker);
        assert_eq!(
            output.packets[1].timestamp,
            VOICE_AUDIO_PACKET_SAMPLES as u32
        );
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
}
