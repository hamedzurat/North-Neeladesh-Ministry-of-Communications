//! Deliberately small, offline authority for the Cabinet Frontend MVP.
//! The newline-delimited JSON protocol is disposable: it exists only to make
//! the Odin/Rust boundary inspectable during the MVP demonstration.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const SHIFT_START_MINUTES: u16 = 9 * 60;
const SHIFT_END_MINUTES: u16 = 17 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    IncomingCaller,
    ConnectedToOperator,
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
    pub initial_perspective: &'static str,
    pub paired_relationship: &'static str,
    pub permitted_actions: &'static [SubscriberAction],
    pub local_voice_configuration: &'static str,
}

impl SubscriberProfile {
    fn operator_session_prompt(&self, transcript: &str) -> String {
        format!(
            "You are {identity}, {role}. Personality: {personality} Speaking style: {style} Immediate goal: {goal} Relationship: {relationship} The Exchange Operator said: {transcript} Reply in character in one or two sentences, maximum 35 words. Do not invent facts or actions.",
            identity = self.identity,
            role = self.occupation_or_role,
            personality = self.personality,
            style = self.speaking_style,
            goal = self.immediate_goal,
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
/// only locally configured commands and never makes a network request.
pub trait VoicePipeline {
    fn begin_capture(&mut self) -> Result<(), VoicePipelineError>;
    fn finish_operator_session(
        &mut self,
        profile: &SubscriberProfile,
        speaker_enabled: bool,
    ) -> Result<OperatorSession, VoicePipelineError>;

    fn cancel_capture(&mut self) {}
}

struct ActiveCapture {
    process: Child,
    path: PathBuf,
}

struct LocalVoicePipeline {
    capture: Option<ActiveCapture>,
}

impl Default for LocalVoicePipeline {
    fn default() -> Self {
        Self { capture: None }
    }
}

impl LocalVoicePipeline {
    fn temporary_audio_path(extension: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "north-neeladesh-mvp-{}-{timestamp}.{extension}",
            std::process::id()
        ))
    }

    fn configured_command(variable: &str, stage: VoiceStage) -> Result<String, VoicePipelineError> {
        if let Ok(command) = std::env::var(variable) {
            return Ok(command);
        }
        let default = match variable {
            "NN_MVP_STT_COMMAND" => "scripts/local-stt.sh",
            "NN_MVP_DIALOGUE_COMMAND" => "scripts/local-dialogue.sh",
            "NN_MVP_TTS_COMMAND" => "scripts/local-tts.sh",
            _ => {
                return Err(VoicePipelineError::new(
                    stage,
                    format!("{variable} HAS NO LOCAL MVP ADAPTER"),
                ));
            }
        };
        Ok(default.into())
    }

    fn command_output(
        mut command: Command,
        stage: VoiceStage,
        input: Option<&str>,
    ) -> Result<String, VoicePipelineError> {
        if input.is_some() {
            command.stdin(Stdio::piped());
        }
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            VoicePipelineError::new(stage, format!("COULD NOT START LOCAL COMMAND: {error}"))
        })?;
        if let Some(input) = input {
            child
                .stdin
                .take()
                .expect("piped stdin is available")
                .write_all(input.as_bytes())
                .map_err(|error| {
                    VoicePipelineError::new(stage, format!("COULD NOT SEND INPUT: {error}"))
                })?;
        }
        let output = child.wait_with_output().map_err(|error| {
            VoicePipelineError::new(stage, format!("LOCAL COMMAND DID NOT COMPLETE: {error}"))
        })?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr)
                .split_whitespace()
                .take(12)
                .collect::<Vec<_>>()
                .join(" ");
            return Err(VoicePipelineError::new(
                stage,
                if detail.is_empty() {
                    format!("LOCAL COMMAND EXITED {}", output.status)
                } else {
                    detail
                },
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn bounded_text(value: &str) -> String {
        value
            .split_whitespace()
            .take(35)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn fallback_response(profile: &SubscriberProfile) -> String {
        Self::bounded_text(&format!(
            "Operator, please connect me. {}",
            profile.immediate_goal
        ))
    }
}

impl VoicePipeline for LocalVoicePipeline {
    fn begin_capture(&mut self) -> Result<(), VoicePipelineError> {
        if self.capture.is_some() {
            return Err(VoicePipelineError::new(
                VoiceStage::Capture,
                "PTT CAPTURE IS ALREADY ACTIVE",
            ));
        }
        let path = Self::temporary_audio_path("wav");
        let child = Command::new("pw-record")
            .args(["--rate", "16000", "--channels", "1", "--format", "s16"])
            .arg(&path)
            .spawn()
            .map_err(|error| {
                VoicePipelineError::new(
                    VoiceStage::Capture,
                    format!("MICROPHONE UNAVAILABLE: {error}"),
                )
            })?;
        self.capture = Some(ActiveCapture {
            process: child,
            path,
        });
        Ok(())
    }

    fn finish_operator_session(
        &mut self,
        profile: &SubscriberProfile,
        speaker_enabled: bool,
    ) -> Result<OperatorSession, VoicePipelineError> {
        let ActiveCapture { mut process, path } = self.capture.take().ok_or_else(|| {
            VoicePipelineError::new(VoiceStage::Capture, "NO PTT RECORDING WAS STARTED")
        })?;
        let _ = process.kill();
        let _ = process.wait();

        let result = (|| {
            let stt_command = Self::configured_command("NN_MVP_STT_COMMAND", VoiceStage::Stt)?;
            let transcript = Self::bounded_text(&Self::command_output(
                {
                    let mut command = Command::new(stt_command);
                    command.arg(&path);
                    command
                },
                VoiceStage::Stt,
                None,
            )?);
            if transcript.is_empty() {
                return Err(VoicePipelineError::new(
                    VoiceStage::Stt,
                    "NO SPEECH WAS TRANSCRIBED",
                ));
            }

            let dialogue_command =
                Self::configured_command("NN_MVP_DIALOGUE_COMMAND", VoiceStage::Dialogue)?;
            let prompt = profile.operator_session_prompt(&transcript);
            let first_response = Self::bounded_text(&Self::command_output(
                Command::new(&dialogue_command),
                VoiceStage::Dialogue,
                Some(&prompt),
            )?);
            let response = if first_response.is_empty() {
                let retry = Self::bounded_text(&Self::command_output(
                    Command::new(dialogue_command),
                    VoiceStage::Dialogue,
                    Some(&prompt),
                )?);
                if retry.is_empty() {
                    Self::fallback_response(profile)
                } else {
                    retry
                }
            } else {
                first_response
            };

            let speech_path = Self::temporary_audio_path("wav");
            let tts_command = Self::configured_command("NN_MVP_TTS_COMMAND", VoiceStage::Tts)?;
            let synthesis = Self::command_output(
                {
                    let mut command = Command::new(tts_command);
                    command
                        .arg(profile.local_voice_configuration)
                        .arg(&speech_path);
                    command
                },
                VoiceStage::Tts,
                Some(&response),
            );
            if let Err(error) = synthesis {
                let _ = fs::remove_file(&speech_path);
                return Err(error);
            }
            let playback = if speaker_enabled {
                let player =
                    std::env::var("NN_MVP_PLAY_COMMAND").unwrap_or_else(|_| "paplay".into());
                Command::new(player)
                    .arg(&speech_path)
                    .status()
                    .map_err(|error| {
                        VoicePipelineError::new(
                            VoiceStage::Tts,
                            format!("SPEAKER UNAVAILABLE: {error}"),
                        )
                    })
                    .and_then(|status| {
                        status.success().then_some(()).ok_or_else(|| {
                            VoicePipelineError::new(
                                VoiceStage::Tts,
                                format!("SPEAKER EXITED {status}"),
                            )
                        })
                    })
            } else {
                Ok(())
            };
            let _ = fs::remove_file(&speech_path);
            playback?;
            Ok(OperatorSession {
                transcript,
                response,
            })
        })();
        let _ = fs::remove_file(path);
        result
    }

    fn cancel_capture(&mut self) {
        if let Some(mut capture) = self.capture.take() {
            let _ = capture.process.kill();
            let _ = capture.process.wait();
            let _ = fs::remove_file(capture.path);
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
        initial_perspective: "The clinic may be the only safe place left for her household tonight.",
        paired_relationship: "Seeking practical help from Dr. Sorin Vale at Kharad Clinic.",
        permitted_actions: &[SubscriberAction::RequestKharadClinicRouting],
        local_voice_configuration: "pocket-tts:nila-low-warm",
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
        initial_perspective: "Care is scarce, but a clear account can still secure the right response.",
        paired_relationship: "Clinic contact for Nila Das, whose household needs urgent help.",
        permitted_actions: &[SubscriberAction::AssessIncomingHouseholdCall],
        local_voice_configuration: "pocket-tts:sorin-clear-neutral",
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
        initial_perspective: "A missed rail window becomes a citywide delay unless Steel Works decides now.",
        paired_relationship: "Needs a decision from Leela Voss at Steel Works before the rail window closes.",
        permitted_actions: &[SubscriberAction::RequestSteelWorksRouting],
        local_voice_configuration: "pocket-tts:arun-brisk-mid",
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
        initial_perspective: "The plant's commitments matter, but an unmanaged freight problem could expose her authority.",
        paired_relationship: "The Steel Works decision-maker sought by Arun Merek at Railway Dispatch.",
        permitted_actions: &[SubscriberAction::ConfirmSteelWorksFreightStatus],
        local_voice_configuration: "pocket-tts:leela-measured-low",
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
    voice_pipeline: Box<dyn VoicePipeline>,
    ptt_held: bool,
    capture_active: bool,
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
            voice_pipeline: Box::new(pipeline),
            ptt_held: false,
            capture_active: false,
            active_tap_action: -1,
        }
    }

    pub fn apply(&mut self, input: CabinetSnapshot) -> CabinetOutput {
        if input.reset {
            self.voice_pipeline.cancel_capture();
            *self = Self::new();
            return self.output(
                input.sequence,
                "RESET COMPLETE",
                input.directory_id,
                false,
                input.speaker_enabled,
            );
        }

        let (caller, callee) = self.current_pair();
        let connected_to_operator = has_pair(&input.cords, caller.line, OPERATOR_JACK);
        let ringing_attempt =
            has_pair(&input.cords, callee.line, RING_JACK) || input.crank_complete;
        let holding_ring_connection =
            input.cords.len() == 1 && has_pair(&input.cords, callee.line, RING_JACK);
        let valid_ringing_callee = holding_ring_connection && input.crank_complete;
        let direct = has_pair(&input.cords, caller.line, callee.line);
        let valid_direct = input.cords.len() == 1 && direct;
        let tap_action = tapped_bridge(&input.cords, caller.line, callee.line);
        let tapped = tap_action.is_some();

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
                self.routing_status = "OPERATOR CONNECTED: HOLD PTT";
            }
            Phase::ConnectedToOperator | Phase::RecoverableSystemFailure
                if connected_to_operator && ptt_pressed =>
            {
                self.start_capture();
            }
            Phase::ConnectedToOperator | Phase::RecoverableSystemFailure
                if ptt_released && self.capture_active =>
            {
                self.finish_operator_session(caller, input.speaker_enabled);
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
        match self.voice_pipeline.begin_capture() {
            Ok(()) => {
                self.capture_active = true;
                self.phase = Phase::ConnectedToOperator;
                self.routing_status = "PTT RECORDING: RELEASE TO SEND";
            }
            Err(error) => self.record_voice_error(error),
        }
    }

    fn finish_operator_session(&mut self, caller: SubscriberProfile, speaker_enabled: bool) {
        self.capture_active = false;
        match self
            .voice_pipeline
            .finish_operator_session(&caller, speaker_enabled)
        {
            Ok(exchange) => {
                self.receipts
                    .push(format!("OPERATOR: {}", exchange.transcript));
                self.receipts
                    .push(format!("{}: {}", caller.identity, exchange.response));
                self.receipts.push(format!(
                    "VOICE: {} — LOCAL STT / DIALOGUE / TTS COMPLETE",
                    caller.local_voice_configuration
                ));
                self.receipts
                    .push("--------------------------------".into());
                self.phase = Phase::AwaitingRouting;
                self.routing_status = "AWAITING ROUTING: RING CALLEE";
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
            lamps[self.current_pair().0.line] = true;
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
        }
    }
}

fn has_pair(cords: &[Cord], left: usize, right: usize) -> bool {
    cords
        .iter()
        .any(|Cord(a, b)| (*a == left && *b == right) || (*a == right && *b == left))
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

    #[test]
    fn fixed_call_transitions_from_incoming_to_direct_routing_receipt() {
        let mut core = test_core();
        let incoming = core.apply(snapshot(&[], -1, false));
        assert_eq!(incoming.phase, Phase::IncomingCaller);
        assert_eq!(incoming.clock_minutes, 9 * 60);
        assert!(incoming.line_lamps[4]);
        assert!(!incoming.line_lamps[1]);
        assert_eq!(
            core.apply(snapshot(&[(4, 16)], -1, false)).phase,
            Phase::ConnectedToOperator
        );
        core.apply(snapshot(&[(4, 16)], 0, false));
        assert_eq!(
            core.apply(snapshot(&[(4, 16)], -1, false)).phase,
            Phase::AwaitingRouting
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
                "pocket-tts:nila-low-warm",
            ),
            (
                4102,
                "DR. SORIN VALE",
                "KHARAD CLINIC",
                "Clinic intake worker",
                "pocket-tts:sorin-clear-neutral",
            ),
            (
                4103,
                "ARUN MEREK",
                "RAILWAY DISPATCH",
                "Railway Dispatch clerk",
                "pocket-tts:arun-brisk-mid",
            ),
            (
                4104,
                "LEELA VOSS",
                "STEEL WORKS",
                "Steel Works manager",
                "pocket-tts:leela-measured-low",
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
    fn clearing_a_completed_circuit_advances_to_the_second_fixed_call() {
        let mut core = test_core();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(1, 17)], -1, true));
        core.apply(snapshot(&[(4, 1)], -1, false));

        let output = core.apply(snapshot(&[], -1, false));

        assert_eq!(output.phase, Phase::IncomingCaller);
        assert!(output.line_lamps[0]);
        assert!(!output.line_lamps[4]);
    }

    #[test]
    fn speaker_control_is_echoed_as_rust_owned_cabinet_output() {
        let mut core = test_core();
        let mut input = snapshot(&[], -1, false);
        input.speaker_enabled = false;

        assert!(!core.apply(input).speaker_active);
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

        let response = core.apply(snapshot(&[(4, 16)], -1, false));

        assert_eq!(response.phase, Phase::AwaitingRouting);
        assert_eq!(response.routing_status, "AWAITING ROUTING: RING CALLEE");
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
    fn a_voice_stage_failure_stops_only_the_exchange_and_ptt_retries_it() {
        let mut core = MvpCore::with_voice_pipeline(RetriableVoicePipeline { exchanges: 0 });
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));

        let failed = core.apply(snapshot(&[(4, 16)], -1, false));
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
        let retried = core.apply(snapshot(&[(4, 16)], -1, false));
        assert_eq!(retried.phase, Phase::AwaitingRouting);
        assert_eq!(retried.routing_status, "AWAITING ROUTING: RING CALLEE");
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
    fn local_pipeline_defaults_to_the_three_separately_started_worker_adapters() {
        assert_eq!(
            LocalVoicePipeline::configured_command("NN_MVP_STT_COMMAND", VoiceStage::Stt)
                .as_deref(),
            Ok("scripts/local-stt.sh")
        );
        assert_eq!(
            LocalVoicePipeline::configured_command(
                "NN_MVP_DIALOGUE_COMMAND",
                VoiceStage::Dialogue,
            )
            .as_deref(),
            Ok("scripts/local-dialogue.sh")
        );
        assert_eq!(
            LocalVoicePipeline::configured_command("NN_MVP_TTS_COMMAND", VoiceStage::Tts)
                .as_deref(),
            Ok("scripts/local-tts.sh")
        );
    }
}
