use std::collections::VecDeque;
use std::env;
use std::io::{self, ErrorKind};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use exchange_protocol::{
    BackendDiagnostic, ClockState, CordConnection, DEBUG_PROTOCOL_VERSION, DebugAudio,
    DebugAudioKind, DebugCommand, DebugCounters, DebugFrontendState, DebugRequest, DebugResponse,
    DebugRunState, DebugSnapshot, DebugStoryNode, DebugStoryState, DebugSubscriberState,
    DebugVoiceConversation, DebugVoiceState, DirectoryPage, GamePhase, InputMessage, InputState,
    OutputDebug, PROTOCOL_VERSION, PortId, PrinterEntry, ProtocolError, RtpL16Packet,
    ServiceCallPhase, ServiceCallStatus, ServiceErrorCount, ServiceErrorKind, ServiceKind,
    ShiftPhase, ShiftStatus, StateMessage, StateOutput, TuningState, VOICE_AUDIO_SAMPLE_RATE,
    VoiceControl, VoiceControlMessage, VoiceInputAudioMessage, VoiceStatus,
    decode_voice_input_audio, decode_voice_status, encode_voice_control, read_frame, write_frame,
};
use exchange_voice_daemon::{
    CommandDialogueGenerator, CommandSpec, CommandSpeechToText, ConversationTurn, KnowledgeRecord,
    MicrophoneCapture, OperatorSession, PersistentQwen3TtsCommand, Qwen3TtsCommand,
    RelationshipNote, ResponseContext, SubscriberProfile, TextToSpeech, VoiceError, VoiceOutput,
};

pub mod story;

use story::{
    AuthoredContent, CompiledStoryGraph, GraphCompileError, StoryEligibilityState, StoryNodeKind,
    StoryPathSelection,
};

const MAX_FAULTS: usize = 16;
const MAX_CORDS: usize = 8;
const STRESS_PRINTER_ENTRY_COUNT: usize = 48;
const DEMO_SHIFT_DURATION_SECONDS: u64 = 8 * 60;
const DEMO_SHIFT_START_SECONDS: u64 = 8 * 60 * 60;

fn story_node_kind_label(kind: &StoryNodeKind) -> String {
    match kind {
        StoryNodeKind::RunStart { .. } => "run_start".to_string(),
        StoryNodeKind::ShiftCall { .. } => "shift_call".to_string(),
        StoryNodeKind::StoryEvent { .. } => "story_event".to_string(),
        StoryNodeKind::Conditional { .. } => "conditional".to_string(),
        StoryNodeKind::Ending { .. } => "ending".to_string(),
    }
}

pub struct Backend {
    state: StateOutput,
    story: CompiledStoryGraph,
    story_node_id: String,
    state_revision: u64,
    last_request: Option<InputMessage>,
    last_response: Option<StateMessage>,
    last_frontend_input: Option<InputMessage>,
    last_frontend_output: Option<StateMessage>,
    clock_started: Instant,
    last_crank_rotation_timestamps: [u64; 4],
    voice_speaker_active: bool,
    last_ptt: bool,
    voice_peer: Option<SocketAddr>,
    voice_session_id: Option<u64>,
    voice_turn_id: Option<u64>,
    next_voice_turn_id: u64,
    voice_state_revision: Option<u64>,
    voice_request_voice_id: Option<String>,
    pending_voice_control: Option<VoiceControlMessage>,
    voice_status: Option<VoiceStatus>,
    voice_transcript: Option<String>,
    voice_response_text: Option<String>,
    voice_subscriber_line: Option<u8>,
    frontend_firmware_version: Option<String>,
    frontend_transport_connected: bool,
    frontend_device_faults: Vec<String>,
    debug_elapsed_seconds: u64,
    debug_godmode: bool,
    debug_bypass_restrictions: bool,
    voice_conversations: VecDeque<VoiceConversationRecord>,
    next_voice_conversation_id: u64,
    operator_knowledge: Vec<String>,
    interference_reduced: bool,
    pending_voice_audio: VecDeque<Vec<u8>>,
    pending_voice_input: Option<VoiceInputAudioMessage>,
    pending_voice_input_samples: Vec<i16>,
    pending_voice_input_next_chunk: u32,
    voice_worker_active: bool,
    voice_id: Option<String>,
    run_generation: u64,
    initial_required_service_calls: u32,
    shift_started_real_elapsed_seconds: u64,
    required_service_kind: Option<ServiceKind>,
    last_service: Option<ServiceKind>,
    service_error_recorded: bool,
    last_interference_level: u8,
}

struct VoiceConversationRecord {
    summary: DebugVoiceConversation,
    capture_audio: Vec<i16>,
    tts_audio: Vec<i16>,
}

impl Backend {
    pub fn new() -> Self {
        Self::new_with_printer_stress(false)
    }

    pub fn new_four_shift_demo() -> Self {
        Self::new_four_shift_demo_with_printer_stress(false)
    }

    pub fn new_four_shift_demo_with_printer_stress(printer_stress: bool) -> Self {
        let story = AuthoredContent::four_shift_demo()
            .compile()
            .expect("built-in four-Shift authored Story Graph must compile");
        Self::with_story(story, printer_stress, 0, Some(ServiceKind::Ems))
    }

    pub fn new_with_printer_stress(printer_stress: bool) -> Self {
        let story = AuthoredContent::demo()
            .compile()
            .expect("built-in authored Story Graph must compile");
        Self::with_story(story, printer_stress, 1, Some(ServiceKind::Ems))
    }

    pub fn new_hardware_demo() -> Self {
        Self::new_hardware_demo_with_printer_stress(false)
    }

    pub fn new_hardware_demo_with_printer_stress(printer_stress: bool) -> Self {
        let story = AuthoredContent::hardware_demo()
            .compile()
            .expect("built-in hardware demo Story Graph must compile");
        Self::with_story(story, printer_stress, 1, Some(ServiceKind::Police))
    }

    pub fn new_with_story(content: AuthoredContent) -> Result<Self, GraphCompileError> {
        Self::new_with_story_and_printer_stress(content, false)
    }

    pub fn new_with_story_and_printer_stress(
        content: AuthoredContent,
        printer_stress: bool,
    ) -> Result<Self, GraphCompileError> {
        let initial_required_service_calls =
            if content.nodes.iter().any(|node| node.id == "shift_2_call") {
                0
            } else {
                1
            };
        Ok(Self::with_story(
            content.compile()?,
            printer_stress,
            initial_required_service_calls,
            Some(ServiceKind::Ems),
        ))
    }

    fn with_story(
        story: CompiledStoryGraph,
        printer_stress: bool,
        initial_required_service_calls: u32,
        required_service_kind: Option<ServiceKind>,
    ) -> Self {
        let story_node_id = story.start_node_id().to_string();
        Self {
            state: initial_state(
                printer_stress,
                initial_required_service_calls,
                required_service_kind,
            ),
            story,
            story_node_id,
            state_revision: 0,
            last_request: None,
            last_response: None,
            last_frontend_input: None,
            last_frontend_output: None,
            clock_started: Instant::now(),
            last_crank_rotation_timestamps: [0; 4],
            voice_speaker_active: false,
            last_ptt: false,
            voice_peer: None,
            voice_session_id: None,
            voice_turn_id: None,
            next_voice_turn_id: 1,
            voice_state_revision: None,
            voice_request_voice_id: None,
            pending_voice_control: None,
            voice_status: None,
            voice_transcript: None,
            voice_response_text: None,
            voice_subscriber_line: None,
            frontend_firmware_version: None,
            frontend_transport_connected: false,
            frontend_device_faults: Vec::new(),
            debug_elapsed_seconds: 0,
            debug_godmode: false,
            debug_bypass_restrictions: false,
            voice_conversations: VecDeque::new(),
            next_voice_conversation_id: 1,
            operator_knowledge: Vec::new(),
            interference_reduced: false,
            pending_voice_audio: VecDeque::new(),
            pending_voice_input: None,
            pending_voice_input_samples: Vec::new(),
            pending_voice_input_next_chunk: 0,
            voice_worker_active: false,
            voice_id: None,
            run_generation: 0,
            initial_required_service_calls,
            shift_started_real_elapsed_seconds: 0,
            required_service_kind,
            last_service: None,
            service_error_recorded: false,
            last_interference_level: 0,
        }
    }

    pub fn story_graph(&self) -> &CompiledStoryGraph {
        &self.story
    }

    pub fn story_node_id(&self) -> &str {
        &self.story_node_id
    }

    pub fn select_story_path(&mut self, proposal: Option<&str>) -> StoryPathSelection {
        if self.state.call.is_some() {
            return StoryPathSelection {
                node_id: self.story_node_id.clone(),
                used_default: false,
                rejected_proposal: proposal.is_some(),
            };
        }
        let selection = self.story.select_next_with_state(
            &self.story_node_id,
            proposal,
            &StoryEligibilityState {
                service_errors: self.state.shift.service_errors,
            },
        );
        if self
            .story
            .node(&self.story_node_id)
            .is_some_and(|node| !self.story.outgoing(&node.id).is_empty())
        {
            self.story_node_id = selection.node_id.clone();
            if let Some(StoryNodeKind::Ending { ending_id }) =
                self.story.node(&self.story_node_id).map(|node| &node.kind)
            {
                self.state.shift.phase = ShiftPhase::Settled;
                self.state.game_phase = GamePhase::Ended;
                if let Some(ending) = self.story.ending(ending_id) {
                    append_printer(&mut self.state, &ending_receipt(ending.conclusion.as_str()));
                }
            }
        }
        selection
    }

    pub fn reset_run(&mut self) {
        let printer_stress = self.state.printer_output.len() == STRESS_PRINTER_ENTRY_COUNT;
        self.state = initial_state(
            printer_stress,
            self.initial_required_service_calls,
            self.required_service_kind,
        );
        if !printer_stress {
            append_printer(&mut self.state, "RUN RESET // SHIFT READY");
        }
        self.story_node_id = self.story.start_node_id().to_string();
        self.state_revision = 0;
        self.last_request = None;
        self.last_response = None;
        self.last_frontend_input = None;
        self.last_frontend_output = None;
        self.clock_started = Instant::now();
        self.last_crank_rotation_timestamps = [0; 4];
        self.voice_speaker_active = false;
        self.last_ptt = false;
        self.voice_state_revision = None;
        self.next_voice_turn_id = 1;
        self.voice_request_voice_id = None;
        self.pending_voice_control = None;
        self.voice_status = self.voice_peer.map(|_| VoiceStatus::Ready);
        self.voice_transcript = None;
        self.voice_response_text = None;
        self.voice_subscriber_line = None;
        self.frontend_firmware_version = None;
        self.frontend_transport_connected = false;
        self.frontend_device_faults.clear();
        self.debug_elapsed_seconds = 0;
        self.shift_started_real_elapsed_seconds = 0;
        self.debug_godmode = false;
        self.debug_bypass_restrictions = false;
        self.voice_conversations.clear();
        self.next_voice_conversation_id = 1;
        self.operator_knowledge.clear();
        self.interference_reduced = false;
        self.pending_voice_audio.clear();
        self.pending_voice_input = None;
        self.pending_voice_input_samples.clear();
        self.pending_voice_input_next_chunk = 0;
        self.voice_worker_active = false;
        self.voice_id = None;
        self.run_generation = self.run_generation.wrapping_add(1);
        self.last_service = None;
        self.service_error_recorded = false;
        self.last_interference_level = 0;
    }

    pub fn apply_input_message(&mut self, message: InputMessage) -> StateMessage {
        let repeated = self.last_request.as_ref() == Some(&message);
        self.frontend_firmware_version = message.input.debug.firmware_version.clone();
        self.frontend_transport_connected = message.input.debug.transport_connected;
        self.frontend_device_faults = message.input.debug.device_faults.clone();
        let response = self.apply_input_message_inner(message.clone());
        self.last_frontend_input = Some(message);
        self.last_frontend_output = Some(response.clone());
        if !repeated
            && !response.accepted
            && let Some(error) = &response.error
        {
            self.add_diagnostic(BackendDiagnostic {
                code: format!("frontend_{}", error.code),
                message: error.message.clone(),
            });
        }
        response
    }

    pub fn debug_snapshot(&self) -> DebugSnapshot {
        let current_story_beat = self
            .story
            .node(&self.story_node_id)
            .and_then(|node| match &node.kind {
                StoryNodeKind::ShiftCall { beat_id, .. } => Some(beat_id.clone()),
                _ => None,
            });
        let subscribers = self
            .story
            .subscribers()
            .iter()
            .map(|subscriber| {
                let line = self
                    .story
                    .content_line_for_subscriber(&subscriber.id)
                    .map(|listing| listing.line);
                let status = line.map_or_else(
                    || "authored".to_string(),
                    |line| {
                        if self.state.line_lamps[line as usize] {
                            "off_hook".to_string()
                        } else {
                            "on_hook".to_string()
                        }
                    },
                );
                DebugSubscriberState {
                    id: subscriber.id.clone(),
                    name: subscriber.name.clone(),
                    line,
                    status,
                    availability: if line.is_some_and(|line| self.state.line_lamps[line as usize]) {
                        "off_hook".to_string()
                    } else {
                        "on_hook".to_string()
                    },
                    pressure: u32::from(
                        line.is_some_and(|line| self.state.line_lamps[line as usize]),
                    ),
                    current_goal: (subscriber.id == "taren_kesh")
                        .then_some("Reach the requested Callee".to_string())
                        .filter(|_| current_story_beat.is_some()),
                    status_flags: Vec::new(),
                }
            })
            .collect();
        let recent_errors = self.state.debug.messages.clone();

        DebugSnapshot {
            run: DebugRunState {
                number: 1,
                state_revision: self.state_revision,
                elapsed_seconds: self.elapsed_seconds(),
                game_phase: self.state.game_phase.clone(),
                godmode: self.debug_godmode,
                bypass_restrictions: self.debug_bypass_restrictions,
            },
            shift: self.state.shift.clone(),
            calls: self.state.calls.clone(),
            subscribers,
            story: DebugStoryState {
                current_node_id: self.story_node_id.clone(),
                frontier: self.story.outgoing(&self.story_node_id).to_vec(),
                current_story_beat,
                interference_reduced: self.interference_reduced,
                interference_level: self.state.interference_level,
                operator_knowledge: self.operator_knowledge.clone(),
                graph: self
                    .story
                    .nodes()
                    .map(|node| DebugStoryNode {
                        id: node.id.clone(),
                        kind: story_node_kind_label(&node.kind),
                        outgoing: self.story.outgoing(&node.id).to_vec(),
                    })
                    .collect(),
            },
            counters: DebugCounters {
                completed_routings: self.state.shift.completed_routings,
                completed_service_calls: self.state.shift.completed_service_calls,
                required_service_calls: self.state.shift.required_service_calls,
                service_errors: self.state.shift.service_errors,
                active_call_count: self.state.shift.active_call_count,
            },
            voice: DebugVoiceState {
                status: self.voice_status,
                speaker_active: self.state.speaker_active || self.voice_speaker_active,
                session_id: self.voice_session_id,
                turn_id: self.voice_turn_id,
                transcript: self.voice_transcript.clone(),
                response_text: self.voice_response_text.clone(),
                conversations: self
                    .voice_conversations
                    .iter()
                    .map(|conversation| conversation.summary.clone())
                    .collect(),
            },
            frontend: DebugFrontendState {
                firmware_version: self.frontend_firmware_version.clone(),
                transport_connected: self.frontend_transport_connected,
                device_faults: self.frontend_device_faults.clone(),
                last_input_json: self
                    .last_frontend_input
                    .as_ref()
                    .and_then(|message| serde_json::to_string_pretty(message).ok()),
                last_output_json: self
                    .last_frontend_output
                    .as_ref()
                    .and_then(|message| serde_json::to_string_pretty(message).ok()),
            },
            recent_errors,
        }
    }

    pub fn apply_debug_command(&mut self, command: DebugCommand) -> DebugResponse {
        let result = match command {
            DebugCommand::Snapshot => Ok(()),
            DebugCommand::ResetRun => {
                self.reset_run();
                Ok(())
            }
            DebugCommand::AdvanceTime { seconds } => {
                self.debug_elapsed_seconds = self
                    .debug_elapsed_seconds
                    .saturating_add(u64::from(seconds));
                self.state.clock.elapsed_seconds = self.elapsed_seconds();
                self.expire_calls();
                self.state_revision += 1;
                Ok(())
            }
            DebugCommand::InjectCall {
                caller_line,
                callee_line,
            } => self.debug_inject_call(caller_line, callee_line),
            DebugCommand::ForceStoryEvent { event_id } => self.debug_force_story_event(&event_id),
            DebugCommand::SelectStoryPath { node_id } => self.debug_select_story_path(&node_id),
            DebugCommand::SetGodmode { enabled } => {
                self.debug_godmode = enabled;
                Ok(())
            }
            DebugCommand::SetBypassRestrictions { enabled } => {
                self.debug_bypass_restrictions = enabled;
                Ok(())
            }
            DebugCommand::GetVoiceAudio {
                conversation_id,
                kind,
            } => match self.voice_audio(conversation_id, kind) {
                Ok(audio) => {
                    return DebugResponse {
                        protocol_version: DEBUG_PROTOCOL_VERSION,
                        accepted: true,
                        error: None,
                        snapshot: self.debug_snapshot(),
                        audio: Some(audio),
                    };
                }
                Err(error) => Err(error),
            },
        };
        let (accepted, error) = match result {
            Ok(()) => (true, None),
            Err(error) => (false, Some(error)),
        };
        DebugResponse {
            protocol_version: DEBUG_PROTOCOL_VERSION,
            accepted,
            error,
            snapshot: self.debug_snapshot(),
            audio: None,
        }
    }

    fn voice_audio(
        &self,
        conversation_id: u64,
        kind: DebugAudioKind,
    ) -> Result<DebugAudio, ProtocolError> {
        let conversation = self
            .voice_conversations
            .iter()
            .find(|conversation| conversation.summary.id == conversation_id)
            .ok_or_else(|| {
                protocol_error(
                    "voice_conversation_not_found",
                    "voice conversation was not retained",
                )
            })?;
        if kind == DebugAudioKind::Tts
            && !conversation
                .summary
                .status
                .is_some_and(VoiceStatus::is_terminal)
        {
            return Err(protocol_error(
                "voice_audio_not_ready",
                "TTS audio is still being generated",
            ));
        }
        let samples = match kind {
            DebugAudioKind::Capture => &conversation.capture_audio,
            DebugAudioKind::Tts => &conversation.tts_audio,
        };
        Ok(DebugAudio {
            sample_rate: match kind {
                DebugAudioKind::Capture => exchange_protocol::VOICE_INPUT_SAMPLE_RATE,
                DebugAudioKind::Tts => VOICE_AUDIO_SAMPLE_RATE,
            },
            channels: 1,
            samples: samples.clone(),
        })
    }

    fn debug_inject_call(&mut self, caller_line: u8, callee_line: u8) -> Result<(), ProtocolError> {
        if caller_line >= 16 || callee_line >= 16 || caller_line == callee_line {
            return Err(protocol_error(
                "invalid_debug_call",
                "debug Calls must use two different subscriber lines from 0 through 15",
            ));
        }
        if !self.debug_godmode
            && !self.debug_bypass_restrictions
            && self.state.shift.active_call_count > 0
        {
            return Err(protocol_error(
                "debug_call_restricted",
                "enable bypass restrictions before injecting a Call during an active Call",
            ));
        }
        let call = exchange_protocol::CallStatus {
            caller_line,
            requested_callee_line: callee_line,
            phase: exchange_protocol::CallPhase::Waiting,
        };
        self.state.calls.push(call.clone());
        if self.state.call.is_none() {
            self.state.call = Some(call);
        }
        self.state.line_lamps = lamps_for_calls(&self.state.calls);
        self.state.shift.active_call_count = self
            .state
            .calls
            .iter()
            .filter(|call| {
                !matches!(
                    call.phase,
                    exchange_protocol::CallPhase::Completed
                        | exchange_protocol::CallPhase::Missed
                        | exchange_protocol::CallPhase::Misrouted
                        | exchange_protocol::CallPhase::Failed
                )
            })
            .count()
            .min(u8::MAX as usize) as u8;
        self.state.game_phase = GamePhase::Shift;
        self.state.shift.phase = ShiftPhase::Active;
        self.state_revision += 1;
        Ok(())
    }

    fn debug_force_story_event(&mut self, event_id: &str) -> Result<(), ProtocolError> {
        let Some(node_id) = self.story.story_event_node_id(event_id) else {
            return Err(protocol_error(
                "unknown_story_event",
                format!("story event {event_id} is not authored"),
            ));
        };
        self.story_node_id = node_id.to_string();
        self.state_revision += 1;
        Ok(())
    }

    fn debug_select_story_path(&mut self, node_id: &str) -> Result<(), ProtocolError> {
        if self.debug_godmode || self.debug_bypass_restrictions {
            if self.story.node(node_id).is_none() {
                return Err(protocol_error(
                    "unknown_story_node",
                    format!("story node {node_id} is not authored"),
                ));
            }
            self.story_node_id = node_id.to_string();
            self.state_revision += 1;
            return Ok(());
        }
        let selection = self.select_story_path(Some(node_id));
        if selection.rejected_proposal {
            return Err(protocol_error(
                "story_path_rejected",
                format!("story node {node_id} is not eligible from the current frontier"),
            ));
        }
        Ok(())
    }

    fn real_elapsed_seconds(&self) -> u64 {
        self.clock_started
            .elapsed()
            .as_secs()
            .saturating_add(self.debug_elapsed_seconds)
    }

    fn elapsed_seconds(&self) -> u32 {
        accelerated_clock_seconds(self.real_elapsed_seconds())
    }

    fn apply_input_message_inner(&mut self, message: InputMessage) -> StateMessage {
        if self.last_request.as_ref() == Some(&message)
            && let Some(response) = &self.last_response
        {
            return response.clone();
        }

        if let Err(error) = validate_message(self, &message) {
            return rejected_response(
                message.input_sequence,
                error,
                self.state_revision,
                &self.state,
            );
        }

        self.state.clock.elapsed_seconds = self.elapsed_seconds();
        self.expire_calls();
        let input = &message.input;
        let input_service = service_from_controls(&input.held_controls);
        let monitoring_story = self.story_node_id == "shift_3_call"
            || self.story_node_id.starts_with("intercepted_signal")
            || self.story_node_id.starts_with("hardware_demo");
        let ptt = input.held_controls.ptt;
        let crank_rotation_timestamps = input.crank_rotation_timestamps;
        let authored_call = if self.state.call.is_none() {
            self.prepare_story_call(input.directory_digits, &input.cord_topology)
        } else {
            self.story_call_for_node(&self.story_node_id, input.directory_digits)
        };
        if has_diegetic_interference(&self.story_node_id) {
            self.interference_reduced = tuning_reduces_interference(&input.tuning);
        }
        let interference_level = interference_level(&self.story_node_id, &input.tuning);
        let mut next_state = self.state.clone();
        next_state.interference_level = interference_level;
        if interference_level != self.last_interference_level {
            append_printer(
                &mut next_state,
                &format!(
                    "INTERFERENCE // LEVEL {interference_level}% // {}",
                    if interference_level == 0 {
                        "CLEAR"
                    } else {
                        "TUNE REQUIRED"
                    }
                ),
            );
            self.last_interference_level = interference_level;
        }
        let final_standoff = self.story_node_id == "ending_civil_war";
        let authored_competing_call = operator_caller_line(&input.cord_topology)
            .and_then(|line| self.story.authored_call_for_caller_line(line));
        let interference_blocks_routing = has_diegetic_interference(&self.story_node_id)
            && !self.interference_reduced
            && self
                .state
                .call
                .as_ref()
                .is_some_and(|call| direct_routing_topology(input, call));
        let pre_ring_direct_connection = self.state.call.as_ref().is_some_and(|call| {
            call.phase == exchange_protocol::CallPhase::OperatorSession
                && (has_direct_subscriber_circuit(input, call.caller_line)
                    || has_tap_bridge_circuit(
                        &input.cord_topology,
                        &PortId::Subscriber(call.caller_line),
                        &PortId::Subscriber(call.requested_callee_line),
                    ))
        });
        let transition = if pre_ring_direct_connection || interference_blocks_routing {
            unchanged_call_transition(&self.state)
        } else {
            advance_calls(
                &self.state,
                input,
                self.last_crank_rotation_timestamps,
                authored_call,
                authored_competing_call,
            )
        };
        if let Some(outcome) = transition.story_outcome {
            self.advance_story_outcome(outcome);
        }
        next_state.call = transition.call;
        next_state.calls = transition.calls;
        next_state.line_lamps = transition.line_lamps;
        next_state.game_phase = transition.game_phase;
        next_state.shift = transition.shift;
        if transition.story_outcome == Some(StoryOutcome::Invalid) {
            record_service_error(&mut next_state, ServiceErrorKind::MisroutedCall);
        }
        if pre_ring_direct_connection {
            next_state.debug.messages.push(BackendDiagnostic {
                code: "ring_generator_required".to_string(),
                message:
                    "connect the requested Callee to the Ring Generator and crank before routing"
                        .to_string(),
            });
            if next_state.debug.messages.len() > MAX_FAULTS {
                next_state.debug.messages.remove(0);
            }
        }
        apply_service_transition(
            self,
            &mut next_state,
            input,
            transition.story_event_complete,
        );
        if transition.story_event_complete {
            self.settle_story_event(&next_state);
            if self.story_is_terminal() {
                next_state.shift.phase = ShiftPhase::Settled;
                next_state.game_phase = GamePhase::Ended;
                self.append_ending_receipt(&mut next_state);
            } else {
                next_state.calls.clear();
                next_state.call = None;
                next_state.line_lamps = [false; 16];
                next_state.shift.number = next_state.shift.number.saturating_add(1);
                next_state.shift.phase = ShiftPhase::Ready;
                next_state.shift.active_call_count = 0;
                next_state.shift.required_service_calls =
                    self.required_service_calls_for_shift(next_state.shift.number);
                if let Some(service) = self.required_service_kind_for_shift(next_state.shift.number)
                {
                    append_printer(
                        &mut next_state,
                        &format!(
                            "SERVICE RULE // {} REQUIRED THIS SHIFT",
                            service_label(service)
                        ),
                    );
                }
                next_state.game_phase = GamePhase::Ready;
                next_state.clock.shift = next_state.shift.number;
                self.shift_started_real_elapsed_seconds = self.real_elapsed_seconds();
                self.interference_reduced = false;
                self.service_error_recorded = false;
            }
        }
        if final_standoff {
            next_state.calls.clear();
            next_state.call = None;
            next_state.line_lamps = [false; 16];
            next_state.shift.phase = ShiftPhase::Settled;
            next_state.shift.active_call_count = 0;
            next_state.game_phase = GamePhase::Ended;
            self.append_ending_receipt(&mut next_state);
        }
        next_state.tap_bridge_monitoring = tap_bridge_monitoring(input, &next_state);
        if next_state.tap_bridge_monitoring.is_some()
            && monitoring_story
            && self.operator_knowledge.is_empty()
        {
            self.operator_knowledge
                .push("Neri Tal's intercepted signal mentions Vira Dhal".to_string());
        }
        let routing_receipt = transition.routing_receipt;

        if let Some(text) = routing_receipt {
            append_printer(&mut next_state, &text);
        }

        self.last_crank_rotation_timestamps = crank_rotation_timestamps;
        next_state.clock.elapsed_seconds = self.elapsed_seconds();
        self.state_revision += 1;
        let speaker_active = speaker_is_active(input, &next_state) || self.voice_speaker_active;
        self.state = StateOutput {
            line_lamps: next_state.line_lamps,
            game_phase: next_state.game_phase,
            clock: next_state.clock,
            speaker_active,
            interference_level: next_state.interference_level,
            tuning: input.tuning.clone(),
            directory_pages: directory_pages(input.directory_digits),
            printer_output: next_state.printer_output,
            call: next_state.call,
            calls: next_state.calls,
            service_call: next_state.service_call,
            tap_bridge_monitoring: next_state.tap_bridge_monitoring,
            shift: next_state.shift,
            debug: OutputDebug {
                messages: next_state.debug.messages,
            },
        };

        let response = StateMessage {
            protocol_version: PROTOCOL_VERSION,
            input_sequence: message.input_sequence,
            accepted: true,
            error: None,
            state_revision: self.state_revision,
            output: self.state.clone(),
        };
        self.last_request = Some(message);
        self.last_response = Some(response.clone());
        self.last_service = input_service;
        if ptt != self.last_ptt {
            self.last_ptt = ptt;
            let voice_id = if ptt {
                let caller_line = self.state.call.as_ref().map_or(0, |call| call.caller_line);
                let voice_id = self.voice_id_for_line(caller_line);
                self.voice_request_voice_id = Some(voice_id.clone());
                voice_id
            } else {
                self.voice_request_voice_id
                    .take()
                    .unwrap_or_else(|| "Ryan".to_string())
            };
            if ptt {
                self.voice_id = Some(voice_id.clone());
                self.voice_subscriber_line = self.state.call.as_ref().map(|call| call.caller_line);
                self.voice_turn_id = Some(self.next_voice_turn_id);
                self.next_voice_turn_id = self.next_voice_turn_id.wrapping_add(1);
            }
            let turn_id = self.voice_turn_id.unwrap_or(1);
            self.pending_voice_control = Some(VoiceControlMessage {
                protocol_version: exchange_protocol::VOICE_PROTOCOL_VERSION,
                session_id: 1,
                turn_id,
                state_revision: self.state_revision,
                voice_id,
                control: if ptt {
                    VoiceControl::StartPtt
                } else {
                    VoiceControl::ReleasePtt
                },
            });
        }
        response
    }

    fn prepare_story_call(
        &mut self,
        digits: [u8; 4],
        cord_topology: &[CordConnection],
    ) -> Option<(u8, u8)> {
        if matches!(
            self.story.node(&self.story_node_id).map(|node| &node.kind),
            Some(StoryNodeKind::RunStart { .. })
        ) {
            let caller_line = self.story_call_for_node(
                self.story
                    .outgoing(&self.story_node_id)
                    .first()
                    .map_or("", String::as_str),
                digits,
            );
            if !cord_topology.is_empty()
                && !caller_line.is_some_and(|(caller, _)| {
                    has_exact_cords(
                        cord_topology,
                        &[(&PortId::Subscriber(caller), &PortId::Operator)],
                    )
                })
            {
                return None;
            }
            let selection = self.story.select_next(&self.story_node_id, None);
            if !self.story_call_matches(selection.node_id.as_str(), digits) {
                return None;
            }
            self.story_node_id = selection.node_id;
        }
        if let Some(StoryNodeKind::StoryEvent { event_id, .. }) =
            self.story.node(&self.story_node_id).map(|node| &node.kind)
            && self
                .story
                .story_event(event_id)
                .is_some_and(|event| event.outcomes.len() > 1)
        {
            let choice = match directory_id(digits) {
                2 => Some("final_taren_call"),
                4 => Some("final_oren_call"),
                _ => Some("ending_civil_war"),
            };
            self.story_node_id = self
                .story
                .select_next_with_state(
                    &self.story_node_id,
                    choice,
                    &StoryEligibilityState {
                        service_errors: self.state.shift.service_errors,
                    },
                )
                .node_id;
        }
        self.story_call_for_node(&self.story_node_id, digits)
    }

    fn story_call_for_node(&self, node_id: &str, digits: [u8; 4]) -> Option<(u8, u8)> {
        let node = self.story.node(node_id)?;
        let StoryNodeKind::ShiftCall { beat_id, .. } = &node.kind else {
            return None;
        };
        let beat = self.story.story_beat(beat_id)?;
        let premise = self.story.call_premise(&beat.call_premise_id)?;
        let caller_line = self.line_for_listing(&premise.caller_line_id)?;
        let callee_line = self.line_for_listing(&premise.callee_line_id)?;
        let directory_id = directory_id(digits);
        premise
            .directory_ids
            .contains(&directory_id)
            .then_some((caller_line, callee_line))
    }

    fn story_call_matches(&self, node_id: &str, digits: [u8; 4]) -> bool {
        self.story_call_for_node(node_id, digits).is_some()
    }

    fn line_for_listing(&self, listing_id: &str) -> Option<u8> {
        self.story
            .line_listing(listing_id)
            .map(|listing| listing.line)
    }

    fn advance_story_outcome(&mut self, outcome: StoryOutcome) {
        let Some(node) = self.story.node(&self.story_node_id) else {
            return;
        };
        let StoryNodeKind::ShiftCall {
            on_success,
            on_missed,
            on_invalid,
            ..
        } = &node.kind
        else {
            return;
        };
        self.story_node_id = match outcome {
            StoryOutcome::Success => on_success.clone(),
            StoryOutcome::Missed => on_missed.clone(),
            StoryOutcome::Invalid => on_invalid.clone(),
        };
    }

    fn settle_story_event(&mut self, state: &StateOutput) {
        loop {
            let kind = self.story.node(&self.story_node_id).map(|node| &node.kind);
            if let Some(StoryNodeKind::StoryEvent { event_id, .. }) = kind
                && self
                    .story
                    .story_event(event_id)
                    .is_some_and(|event| event.outcomes.len() > 1)
            {
                break;
            }
            if !matches!(
                kind,
                Some(StoryNodeKind::StoryEvent { .. } | StoryNodeKind::Conditional { .. })
            ) {
                break;
            }
            let selection = self.story.select_next_with_state(
                &self.story_node_id,
                None,
                &StoryEligibilityState {
                    service_errors: state.shift.service_errors,
                },
            );
            self.story_node_id = selection.node_id;
        }
    }

    fn story_is_terminal(&self) -> bool {
        matches!(
            self.story.node(&self.story_node_id).map(|node| &node.kind),
            Some(StoryNodeKind::Ending { .. })
        )
    }

    fn expire_calls(&mut self) {
        let shift_elapsed = self
            .real_elapsed_seconds()
            .saturating_sub(self.shift_started_real_elapsed_seconds)
            .min(u32::MAX as u64) as u32;
        if shift_elapsed < call_patience_seconds(self.state.shift.number) {
            return;
        }
        let mut focused_expired = false;
        for call in &mut self.state.calls {
            if !matches!(
                call.phase,
                exchange_protocol::CallPhase::Waiting
                    | exchange_protocol::CallPhase::OperatorSession
                    | exchange_protocol::CallPhase::AwaitingRouting
                    | exchange_protocol::CallPhase::Held
                    | exchange_protocol::CallPhase::Ringing
            ) {
                continue;
            }
            if self
                .state
                .call
                .as_ref()
                .is_some_and(|focused| focused.caller_line == call.caller_line)
            {
                focused_expired = true;
            }
            call.phase = exchange_protocol::CallPhase::Missed;
        }
        if focused_expired {
            if let Some(call) = &mut self.state.call {
                call.phase = exchange_protocol::CallPhase::Missed;
            }
            if matches!(
                self.story.node(&self.story_node_id).map(|node| &node.kind),
                Some(StoryNodeKind::ShiftCall { .. })
            ) {
                self.advance_story_outcome(StoryOutcome::Missed);
            }
        }
        self.state.shift.active_call_count = self
            .state
            .calls
            .iter()
            .filter(|call| {
                !matches!(
                    call.phase,
                    exchange_protocol::CallPhase::Completed
                        | exchange_protocol::CallPhase::Missed
                        | exchange_protocol::CallPhase::Misrouted
                        | exchange_protocol::CallPhase::Failed
                )
            })
            .count()
            .min(u8::MAX as usize) as u8;
    }

    fn append_ending_receipt(&self, state: &mut StateOutput) {
        if let Some(StoryNodeKind::Ending { ending_id }) =
            self.story.node(&self.story_node_id).map(|node| &node.kind)
            && let Some(ending) = self.story.ending(ending_id)
        {
            append_printer(state, &ending_receipt(ending.conclusion.as_str()));
        }
    }

    fn voice_id_for_line(&self, line: u8) -> String {
        self.story
            .subscriber_id_for_line(line)
            .map_or_else(|| "Ryan".to_string(), select_voice_id)
    }

    fn required_service_calls_for_shift(&self, shift: u8) -> u32 {
        if self.initial_required_service_calls == 0 {
            u32::from(shift >= 2)
        } else {
            self.initial_required_service_calls
        }
    }

    fn required_service_kind_for_shift(&self, shift: u8) -> Option<ServiceKind> {
        (self.required_service_calls_for_shift(shift) > 0)
            .then_some(self.required_service_kind)
            .flatten()
    }

    pub fn apply_voice_datagram(&mut self, datagram: &[u8]) -> bool {
        self.apply_voice_datagram_from(datagram, None)
    }

    pub fn apply_voice_datagram_from(&mut self, datagram: &[u8], peer: Option<SocketAddr>) -> bool {
        if let Ok(message) = decode_voice_input_audio(datagram) {
            if message.protocol_version != exchange_protocol::VOICE_PROTOCOL_VERSION
                || self.voice_session_id != Some(message.session_id)
                || self.voice_turn_id != Some(message.turn_id)
                || self.voice_worker_active
                || self
                    .voice_state_revision
                    .is_some_and(|revision| message.state_revision < revision)
                || (peer.is_some() && self.voice_peer != peer)
            {
                return false;
            }
            if message.chunk_index == 0 {
                self.pending_voice_input_samples.clear();
                self.pending_voice_input_next_chunk = 0;
            }
            if message.chunk_index != self.pending_voice_input_next_chunk {
                return false;
            }
            self.pending_voice_input_samples
                .extend_from_slice(&message.samples);
            if self.pending_voice_input_samples.len()
                > exchange_protocol::VOICE_INPUT_SAMPLE_RATE as usize * 15
            {
                self.pending_voice_input_samples.clear();
                self.pending_voice_input_next_chunk = 0;
                return false;
            }
            self.pending_voice_input_next_chunk =
                self.pending_voice_input_next_chunk.saturating_add(1);
            if message.complete {
                let mut complete_message = message;
                complete_message.samples = std::mem::take(&mut self.pending_voice_input_samples);
                self.pending_voice_input = Some(complete_message);
                self.pending_voice_input_next_chunk = 0;
            }
            return true;
        }
        if let Ok(message) = decode_voice_status(datagram) {
            if message.protocol_version != exchange_protocol::VOICE_PROTOCOL_VERSION {
                return false;
            }
            if message.status == VoiceStatus::Ready {
                if let Some(peer) = peer {
                    self.voice_peer = Some(peer);
                }
                self.voice_session_id = Some(message.session_id);
                self.voice_turn_id = Some(message.turn_id);
                self.voice_state_revision = Some(message.state_revision);
            } else {
                if let Some(expected_peer) = self.voice_peer
                    && let Some(peer) = peer
                    && expected_peer != peer
                {
                    return false;
                }
                if let Some(expected_session_id) = self.voice_session_id
                    && expected_session_id != message.session_id
                {
                    return false;
                }
                if let Some(expected_turn_id) = self.voice_turn_id
                    && expected_turn_id != message.turn_id
                {
                    return false;
                }
                if let Some(previous_revision) = self.voice_state_revision
                    && message.state_revision < previous_revision
                {
                    return false;
                }
                if self.voice_session_id.is_none() {
                    return false;
                }
                self.voice_state_revision = Some(
                    self.voice_state_revision
                        .unwrap_or(message.state_revision)
                        .max(message.state_revision),
                );
            }
            self.voice_status = Some(message.status);
            self.voice_speaker_active = matches!(message.status, VoiceStatus::Playing);
            if message.status != VoiceStatus::Ready {
                let conversation_id = self.ensure_voice_conversation(
                    message.session_id,
                    message.turn_id,
                    message.state_revision,
                    &[],
                );
                self.update_voice_conversation(
                    conversation_id,
                    message.status,
                    message.error.clone(),
                    None,
                    None,
                );
            }
            if message.status == VoiceStatus::Failed {
                self.add_diagnostic(BackendDiagnostic {
                    code: "voice_failed".to_string(),
                    message: message.error.map_or_else(
                        || "voice daemon failed without details".to_string(),
                        |error| format!("{}: {}", error.code, error.message),
                    ),
                });
            }
            return true;
        }

        if peer.is_some() && self.voice_peer != peer {
            return false;
        }
        self.voice_session_id.is_some() && exchange_protocol::RtpL16Packet::decode(datagram).is_ok()
    }

    pub fn take_voice_control(&mut self) -> Option<(VoiceControlMessage, SocketAddr)> {
        let peer = self.voice_peer?;
        Some((self.pending_voice_control.take()?, peer))
    }

    fn take_voice_datagrams(&mut self) -> Vec<Vec<u8>> {
        self.pending_voice_audio.drain(..).collect()
    }

    fn queue_voice_datagram(&mut self, datagram: Vec<u8>) {
        self.pending_voice_audio.push_back(datagram);
    }

    fn take_voice_input(
        &mut self,
    ) -> Option<(VoiceInputAudioMessage, String, u8, Vec<String>, u64, u64)> {
        if self.voice_worker_active {
            return None;
        }
        let input = self.pending_voice_input.take()?;
        self.voice_worker_active = true;
        let conversation_id = self.ensure_voice_conversation(
            input.session_id,
            input.turn_id,
            input.state_revision,
            &input.samples,
        );
        if let Some(conversation) = self.voice_conversation_mut(conversation_id) {
            conversation.capture_audio = input.samples.clone();
            conversation.summary.captured_samples =
                conversation.capture_audio.len().min(u32::MAX as usize) as u32;
        }
        Some((
            input,
            self.voice_id.clone().unwrap_or_else(|| "Ryan".to_string()),
            self.voice_subscriber_line.unwrap_or(0),
            self.operator_knowledge.clone(),
            self.run_generation,
            conversation_id,
        ))
    }

    fn finish_voice_worker(&mut self) {
        self.voice_worker_active = false;
    }

    fn ensure_voice_conversation(
        &mut self,
        session_id: u64,
        turn_id: u64,
        state_revision: u64,
        capture_audio: &[i16],
    ) -> u64 {
        if let Some(conversation) = self.voice_conversations.iter().find(|conversation| {
            conversation.summary.session_id == session_id && conversation.summary.turn_id == turn_id
        }) {
            return conversation.summary.id;
        }
        let conversation_id = self.next_voice_conversation_id;
        self.next_voice_conversation_id = self.next_voice_conversation_id.wrapping_add(1);
        self.voice_conversations.push_back(VoiceConversationRecord {
            summary: DebugVoiceConversation {
                id: conversation_id,
                session_id,
                turn_id,
                state_revision,
                status: None,
                started_elapsed_seconds: self.elapsed_seconds(),
                finished_elapsed_seconds: None,
                captured_samples: capture_audio.len().min(u32::MAX as usize) as u32,
                tts_samples: 0,
                transcript: None,
                response_text: None,
                error: None,
            },
            capture_audio: capture_audio.to_vec(),
            tts_audio: Vec::new(),
        });
        conversation_id
    }

    fn voice_conversation_mut(
        &mut self,
        conversation_id: u64,
    ) -> Option<&mut VoiceConversationRecord> {
        self.voice_conversations
            .iter_mut()
            .find(|conversation| conversation.summary.id == conversation_id)
    }

    fn update_voice_conversation(
        &mut self,
        conversation_id: u64,
        status: VoiceStatus,
        error: Option<ProtocolError>,
        transcript: Option<String>,
        response_text: Option<String>,
    ) {
        let finished_elapsed_seconds = self.elapsed_seconds();
        if let Some(conversation) = self.voice_conversation_mut(conversation_id) {
            conversation.summary.status = Some(status);
            conversation.summary.error = error;
            if let Some(transcript) = transcript {
                conversation.summary.transcript = Some(transcript);
            }
            if let Some(response_text) = response_text {
                conversation.summary.response_text = Some(response_text);
            }
            if status.is_terminal() {
                conversation.summary.finished_elapsed_seconds = Some(finished_elapsed_seconds);
            }
        }
    }

    fn fail_voice_conversation(&mut self, conversation_id: u64, error: &VoiceError) {
        let finished_elapsed_seconds = self.elapsed_seconds();
        if let Some(conversation) = self.voice_conversation_mut(conversation_id) {
            conversation.summary.status = Some(VoiceStatus::Failed);
            conversation.summary.finished_elapsed_seconds = Some(finished_elapsed_seconds);
            conversation.summary.error = Some(ProtocolError {
                code: error.code.clone(),
                message: error.message.clone(),
            });
        }
    }

    fn mark_frontend_disconnected(&mut self) {
        self.frontend_transport_connected = false;
    }

    fn add_diagnostic(&mut self, diagnostic: BackendDiagnostic) {
        self.state.debug.messages.push(diagnostic);
        if self.state.debug.messages.len() > MAX_FAULTS {
            self.state.debug.messages.remove(0);
        }
    }
}

fn select_voice_id(subscriber_id: &str) -> String {
    match subscriber_id {
        "taren_kesh" => "Ryan",
        "vira_dhal" => "Vivian",
        "leya_varan" => "Serena",
        "oren_vey" => "Dylan",
        "neri_tal" => "Eric",
        _ => "Ryan",
    }
    .to_string()
}

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

struct CallTransition {
    calls: Vec<exchange_protocol::CallStatus>,
    call: Option<exchange_protocol::CallStatus>,
    line_lamps: [bool; 16],
    game_phase: GamePhase,
    shift: ShiftStatus,
    routing_receipt: Option<String>,
    story_outcome: Option<StoryOutcome>,
    story_event_complete: bool,
}

struct SingleCallTransition {
    call: Option<exchange_protocol::CallStatus>,
    line_lamps: [bool; 16],
    game_phase: GamePhase,
    shift: ShiftStatus,
    routing_receipt: Option<String>,
    story_outcome: Option<StoryOutcome>,
    story_event_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoryOutcome {
    Success,
    Missed,
    Invalid,
}

fn operator_caller_line(cords: &[CordConnection]) -> Option<u8> {
    cords.iter().find_map(|cord| {
        if cord.first == PortId::Operator {
            match cord.second {
                PortId::Subscriber(line) => Some(line),
                _ => None,
            }
        } else if cord.second == PortId::Operator {
            match cord.first {
                PortId::Subscriber(line) => Some(line),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn advance_calls(
    state: &StateOutput,
    input: &InputState,
    previous_crank_rotation_timestamps: [u64; 4],
    authored_call: Option<(u8, u8)>,
    authored_competing_call: Option<(u8, u8)>,
) -> CallTransition {
    let operator_line = operator_caller_line(&input.cord_topology);
    let mut calls = state.calls.clone();
    if calls.is_empty()
        && let Some(call) = state.call.clone()
    {
        calls.push(call);
    }

    if state
        .service_call
        .as_ref()
        .is_some_and(|service| service.phase == ServiceCallPhase::Active)
        && service_from_controls(&input.held_controls).is_some()
    {
        return CallTransition {
            line_lamps: lamps_for_calls(&calls),
            call: None,
            calls,
            game_phase: state.game_phase.clone(),
            shift: state.shift.clone(),
            routing_receipt: None,
            story_outcome: None,
            story_event_complete: false,
        };
    }

    if let Some((caller_line, callee_line)) = authored_competing_call
        && !calls.iter().any(|call| call.caller_line == caller_line)
        && !calls.is_empty()
    {
        calls.push(exchange_protocol::CallStatus {
            caller_line,
            requested_callee_line: callee_line,
            phase: exchange_protocol::CallPhase::OperatorSession,
        });
    }

    let previous_focused_line = state.call.as_ref().map(|call| call.caller_line);
    let authored_caller = authored_call.map(|(caller, _)| caller).or_else(|| {
        state.call.as_ref().and_then(|call| {
            matches!(
                call.phase,
                exchange_protocol::CallPhase::Completed
                    | exchange_protocol::CallPhase::Missed
                    | exchange_protocol::CallPhase::Misrouted
            )
            .then_some(call.caller_line)
        })
    });
    let mut next_calls = Vec::with_capacity(calls.len());
    let mut focused_call = None;
    let mut game_phase = state.game_phase.clone();
    let mut shift = state.shift.clone();
    let mut routing_receipt = None;
    let mut story_outcome = None;
    let mut story_event_complete = false;

    if calls.is_empty() {
        let single = advance_single_call(
            state,
            input,
            previous_crank_rotation_timestamps,
            authored_call,
        );
        if let Some(call) = single.call.clone() {
            focused_call = Some(call.clone());
            next_calls.push(call);
        }
        let line_lamps = lamps_for_calls(&next_calls);
        return CallTransition {
            calls: next_calls,
            call: focused_call,
            line_lamps,
            game_phase: single.game_phase,
            shift: single.shift,
            routing_receipt: single.routing_receipt,
            story_outcome: single.story_outcome,
            story_event_complete: single.story_event_complete,
        };
    }

    for call in calls {
        let mut single_state = state.clone();
        single_state.call = Some(call.clone());
        single_state.calls = vec![call.clone()];
        let mut single = advance_single_call(
            &single_state,
            input,
            previous_crank_rotation_timestamps,
            None,
        );
        if previous_focused_line == Some(call.caller_line)
            && operator_line.is_some()
            && operator_line != Some(call.caller_line)
            && call.phase == exchange_protocol::CallPhase::OperatorSession
        {
            single.call = Some(exchange_protocol::CallStatus {
                phase: exchange_protocol::CallPhase::Held,
                ..call.clone()
            });
            single.line_lamps = lamps_for_call(single.call.as_ref());
        }
        if let Some(next_call) = single.call {
            if operator_line == Some(next_call.caller_line)
                || (operator_line.is_none() && previous_focused_line == Some(next_call.caller_line))
            {
                focused_call = Some(next_call.clone());
            }
            next_calls.push(next_call);
        }
        game_phase = single.game_phase;
        shift = single.shift;
        if authored_caller == Some(call.caller_line) {
            story_outcome = single.story_outcome;
            story_event_complete = single.story_event_complete;
            routing_receipt = single.routing_receipt;
        } else if previous_focused_line == Some(call.caller_line) {
            story_event_complete = single.story_event_complete;
        }
    }

    if focused_call.is_none() && previous_focused_line.is_some() {
        focused_call = next_calls
            .iter()
            .find(|call| Some(call.caller_line) == previous_focused_line)
            .cloned();
    }
    shift.active_call_count = next_calls
        .iter()
        .filter(|call| {
            !matches!(
                call.phase,
                exchange_protocol::CallPhase::Completed
                    | exchange_protocol::CallPhase::Missed
                    | exchange_protocol::CallPhase::Misrouted
                    | exchange_protocol::CallPhase::Failed
            )
        })
        .count()
        .min(u8::MAX as usize) as u8;
    CallTransition {
        calls: next_calls.clone(),
        call: focused_call,
        line_lamps: lamps_for_calls(&next_calls),
        game_phase,
        shift,
        routing_receipt,
        story_outcome,
        story_event_complete,
    }
}

fn advance_single_call(
    state: &StateOutput,
    input: &InputState,
    previous_crank_rotation_timestamps: [u64; 4],
    authored_call: Option<(u8, u8)>,
) -> SingleCallTransition {
    if state.call.is_none() {
        let Some((caller_line, callee)) = authored_call else {
            if input.cord_topology.is_empty() {
                return SingleCallTransition {
                    line_lamps: [false; 16],
                    call: None,
                    game_phase: state.game_phase.clone(),
                    shift: state.shift.clone(),
                    routing_receipt: None,
                    story_outcome: None,
                    story_event_complete: false,
                };
            }
            return unchanged_transition(state);
        };
        let caller = PortId::Subscriber(caller_line);
        let operator_cord = has_exact_cords(&input.cord_topology, &[(&caller, &PortId::Operator)]);
        if !input.cord_topology.is_empty() && !operator_cord {
            return unchanged_transition(state);
        }
        let phase = if operator_cord {
            exchange_protocol::CallPhase::OperatorSession
        } else {
            exchange_protocol::CallPhase::Waiting
        };
        let call = exchange_protocol::CallStatus {
            caller_line,
            requested_callee_line: callee,
            phase,
        };
        let active_call_count = 1;
        return SingleCallTransition {
            line_lamps: lamps_for_call(Some(&call)),
            call: Some(call),
            game_phase: if operator_cord {
                GamePhase::Shift
            } else {
                state.game_phase.clone()
            },
            shift: ShiftStatus {
                active_call_count,
                phase: if operator_cord {
                    ShiftPhase::Active
                } else {
                    state.shift.phase.clone()
                },
                ..state.shift.clone()
            },
            routing_receipt: None,
            story_outcome: None,
            story_event_complete: false,
        };
    }

    let call = state.call.as_ref().expect("call checked above");
    let caller = PortId::Subscriber(call.caller_line);
    let callee = PortId::Subscriber(call.requested_callee_line);
    let caller_operator = has_exact_cords(&input.cord_topology, &[(&caller, &PortId::Operator)]);
    let ring_generator = has_ring_generator(&input.cord_topology, &caller, &callee);
    let direct_circuit = has_exact_cords(&input.cord_topology, &[(&caller, &callee)])
        || has_tap_bridge_circuit(&input.cord_topology, &caller, &callee);

    let mut next_call = call.clone();
    let mut next_game_phase = state.game_phase.clone();
    let mut next_shift = state.shift.clone();
    let mut routing_receipt = None;
    let mut story_outcome = None;
    let story_event_complete = false;
    let invalid_circuit = has_wrong_direct_circuit(&input.cord_topology, &caller, &callee);

    if call.phase == exchange_protocol::CallPhase::AwaitingRouting && input.cord_topology.is_empty()
    {
        next_call.phase = exchange_protocol::CallPhase::Missed;
        story_outcome = Some(StoryOutcome::Missed);
        return SingleCallTransition {
            line_lamps: lamps_for_call(Some(&next_call)),
            call: Some(next_call),
            game_phase: next_game_phase,
            shift: next_shift,
            routing_receipt,
            story_outcome,
            story_event_complete,
        };
    }

    match call.phase {
        exchange_protocol::CallPhase::Waiting => {
            if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
                next_game_phase = GamePhase::Shift;
                next_shift.phase = ShiftPhase::Active;
            } else if !input.cord_topology.is_empty() {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::OperatorSession => {
            if direct_circuit {
                next_call.phase = exchange_protocol::CallPhase::Connected;
                next_shift.completed_routings += 1;
                next_game_phase = GamePhase::Shift;
                story_outcome = Some(StoryOutcome::Success);
                routing_receipt = Some(format!(
                    "ROUTING {} -> {}",
                    call.caller_line, call.requested_callee_line
                ));
            } else if ring_generator {
                if !crank_satisfies_ringing(
                    input.crank_rotation_timestamps,
                    previous_crank_rotation_timestamps,
                ) {
                    return unchanged_transition(state);
                }
                next_call.phase = exchange_protocol::CallPhase::Ringing;
            } else if input.cord_topology.is_empty() {
                next_call.phase = exchange_protocol::CallPhase::AwaitingRouting;
            } else if !caller_operator {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::AwaitingRouting => {
            if direct_circuit {
                next_call.phase = exchange_protocol::CallPhase::Connected;
                next_shift.completed_routings += 1;
                next_game_phase = GamePhase::Shift;
                story_outcome = Some(StoryOutcome::Success);
                routing_receipt = Some(format!(
                    "ROUTING {} -> {}",
                    call.caller_line, call.requested_callee_line
                ));
            } else if invalid_circuit {
                next_call.phase = exchange_protocol::CallPhase::Misrouted;
                story_outcome = Some(StoryOutcome::Invalid);
            } else if ring_generator {
                if !crank_satisfies_ringing(
                    input.crank_rotation_timestamps,
                    previous_crank_rotation_timestamps,
                ) {
                    return unchanged_transition(state);
                }
                next_call.phase = exchange_protocol::CallPhase::Ringing;
            } else if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
            } else if !input.cord_topology.is_empty() {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Held => {
            if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
                next_game_phase = GamePhase::Shift;
                next_shift.phase = ShiftPhase::Active;
            } else if !input.cord_topology.is_empty() {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Ringing => {
            if direct_circuit {
                next_call.phase = exchange_protocol::CallPhase::Connected;
                next_shift.completed_routings += 1;
                next_game_phase = GamePhase::Shift;
                story_outcome = Some(StoryOutcome::Success);
                routing_receipt = Some(format!(
                    "ROUTING {} -> {}",
                    call.caller_line, call.requested_callee_line
                ));
            } else if invalid_circuit {
                next_call.phase = exchange_protocol::CallPhase::Misrouted;
                story_outcome = Some(StoryOutcome::Invalid);
            } else if ring_generator {
                if crank_satisfies_ringing(
                    input.crank_rotation_timestamps,
                    previous_crank_rotation_timestamps,
                ) {
                    next_call.phase = exchange_protocol::CallPhase::Ringing;
                } else {
                    next_call.phase = exchange_protocol::CallPhase::AwaitingRouting;
                }
            } else if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
            } else if input.cord_topology.is_empty() {
                next_call.phase = exchange_protocol::CallPhase::AwaitingRouting;
            } else {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Connected => {
            if direct_circuit {
                next_call.phase = exchange_protocol::CallPhase::Completed;
            } else if input.cord_topology.is_empty() {
                next_shift.active_call_count = 0;
                return SingleCallTransition {
                    call: None,
                    line_lamps: [false; 16],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                    story_outcome: None,
                    story_event_complete: true,
                };
            } else {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Completed => {
            if input.cord_topology.is_empty() {
                return SingleCallTransition {
                    call: None,
                    line_lamps: [false; 16],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                    story_outcome: None,
                    story_event_complete: true,
                };
            }
            if !direct_circuit {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Missed
        | exchange_protocol::CallPhase::Misrouted
        | exchange_protocol::CallPhase::Failed => {
            if input.cord_topology.is_empty() {
                next_shift.active_call_count = 0;
                return SingleCallTransition {
                    call: None,
                    line_lamps: [false; 16],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                    story_outcome: None,
                    story_event_complete: true,
                };
            }
        }
    }

    next_shift.active_call_count =
        u8::from(next_call.phase != exchange_protocol::CallPhase::Completed);
    SingleCallTransition {
        line_lamps: lamps_for_call(Some(&next_call)),
        call: Some(next_call),
        game_phase: next_game_phase,
        shift: next_shift,
        routing_receipt,
        story_outcome,
        story_event_complete,
    }
}

fn unchanged_transition(state: &StateOutput) -> SingleCallTransition {
    SingleCallTransition {
        call: state.call.clone(),
        line_lamps: state.line_lamps,
        game_phase: state.game_phase.clone(),
        shift: state.shift.clone(),
        routing_receipt: None,
        story_outcome: None,
        story_event_complete: false,
    }
}

fn unchanged_call_transition(state: &StateOutput) -> CallTransition {
    let calls = if state.calls.is_empty() {
        state.call.clone().into_iter().collect()
    } else {
        state.calls.clone()
    };
    CallTransition {
        calls,
        call: state.call.clone(),
        line_lamps: state.line_lamps,
        game_phase: state.game_phase.clone(),
        shift: state.shift.clone(),
        routing_receipt: None,
        story_outcome: None,
        story_event_complete: false,
    }
}

fn direct_routing_topology(input: &InputState, call: &exchange_protocol::CallStatus) -> bool {
    let caller = PortId::Subscriber(call.caller_line);
    let callee = PortId::Subscriber(call.requested_callee_line);
    has_exact_cords(&input.cord_topology, &[(&caller, &callee)])
        || has_tap_bridge_circuit(&input.cord_topology, &caller, &callee)
}

fn has_direct_subscriber_circuit(input: &InputState, caller_line: u8) -> bool {
    let caller = PortId::Subscriber(caller_line);
    input.cord_topology.len() == 1
        && input.cord_topology.iter().any(|cord| {
            (cord.first == caller && matches!(cord.second, PortId::Subscriber(_)))
                || (cord.second == caller && matches!(cord.first, PortId::Subscriber(_)))
        })
}

fn crank_satisfies_ringing(current: [u64; 4], previous: [u64; 4]) -> bool {
    current[3] > previous[3]
}

fn valid_crank_history(timestamps: [u64; 4]) -> bool {
    let mut previous = 0;
    let mut nonzero_seen = false;
    for timestamp in timestamps {
        if timestamp == 0 {
            if nonzero_seen {
                return false;
            }
        } else {
            if timestamp <= previous {
                return false;
            }
            previous = timestamp;
            nonzero_seen = true;
        }
    }
    true
}

fn tuning_reduces_interference(tuning: &TuningState) -> bool {
    (384..=640).contains(&tuning.coarse) && (384..=640).contains(&tuning.fine)
}

fn has_diegetic_interference(story_node_id: &str) -> bool {
    story_node_id == "shift_2_call" || story_node_id == "hardware_demo_call"
}

fn interference_level(story_node_id: &str, tuning: &TuningState) -> u8 {
    if !has_diegetic_interference(story_node_id) {
        return 0;
    }
    if tuning_reduces_interference(tuning) {
        return 0;
    }
    let distance =
        u32::from(tuning.coarse.abs_diff(512)).saturating_add(u32::from(tuning.fine.abs_diff(512)));
    (distance.saturating_mul(100) / 1024).clamp(10, 100) as u8
}

fn call_patience_seconds(shift: u8) -> u32 {
    match shift {
        1 => 90,
        2 => 120,
        3 => 150,
        _ => 90,
    }
}

fn accelerated_clock_seconds(real_elapsed_seconds: u64) -> u32 {
    DEMO_SHIFT_START_SECONDS
        .saturating_add(
            real_elapsed_seconds
                .min(DEMO_SHIFT_DURATION_SECONDS)
                .saturating_mul(60),
        )
        .min(u32::MAX as u64) as u32
}

fn speaker_is_active(input: &InputState, state: &StateOutput) -> bool {
    let held = &input.held_controls;
    let operator_active = held.ptt
        && input
            .cord_topology
            .iter()
            .any(|cord| cord.first == PortId::Operator || cord.second == PortId::Operator);
    let tap_active = tap_bridge_monitoring(input, state).is_some();
    operator_active || held.police || held.ems || held.fire || tap_active
}

fn service_from_controls(held: &exchange_protocol::HeldControls) -> Option<ServiceKind> {
    if held.police {
        Some(ServiceKind::Police)
    } else if held.ems {
        Some(ServiceKind::Ems)
    } else if held.fire {
        Some(ServiceKind::Fire)
    } else {
        None
    }
}

fn apply_service_transition(
    backend: &mut Backend,
    state: &mut StateOutput,
    input: &InputState,
    settling: bool,
) {
    let held_service = service_from_controls(&input.held_controls);
    match state
        .service_call
        .as_ref()
        .map(|call| (call.service, call.phase))
    {
        Some((service, ServiceCallPhase::Active)) => {
            if held_service.is_none() {
                state.service_call = Some(ServiceCallStatus {
                    service,
                    phase: ServiceCallPhase::Completed,
                });
                state.shift.completed_service_calls += 1;
                append_printer(
                    state,
                    &format!("SERVICE {} COMPLETED", service_label(service)),
                );
            }
        }
        _ if held_service.is_some()
            && backend.last_service != held_service
            && state.shift.phase == ShiftPhase::Active =>
        {
            let service = held_service.expect("service checked above");
            state.service_call = Some(ServiceCallStatus {
                service,
                phase: ServiceCallPhase::Active,
            });
            for call in &mut state.calls {
                if call.phase == exchange_protocol::CallPhase::OperatorSession {
                    call.phase = exchange_protocol::CallPhase::Held;
                }
            }
            state.call = None;
            state.line_lamps = lamps_for_calls(&state.calls);
        }
        _ => {}
    }

    if settling
        && !backend.service_error_recorded
        && state.shift.completed_service_calls < state.shift.required_service_calls
    {
        record_service_error(state, ServiceErrorKind::MissedRequiredServiceCall);
        backend.service_error_recorded = true;
        let required = backend
            .required_service_kind_for_shift(state.shift.number)
            .map_or("SERVICE".to_string(), |service| {
                service_label(service).to_string()
            });
        append_printer(state, &format!("SERVICE ERROR {required} REQUIRED"));
    }
}

fn service_label(service: ServiceKind) -> &'static str {
    match service {
        ServiceKind::Police => "POLICE",
        ServiceKind::Ems => "EMS",
        ServiceKind::Fire => "FIRE",
    }
}

fn record_service_error(state: &mut StateOutput, kind: ServiceErrorKind) {
    state.shift.service_errors += 1;
    if let Some(error) = state
        .shift
        .service_error_counts
        .iter_mut()
        .find(|error| error.kind == kind)
    {
        error.count += 1;
    } else {
        state
            .shift
            .service_error_counts
            .push(ServiceErrorCount { kind, count: 1 });
    }
}

fn append_printer(state: &mut StateOutput, text: &str) {
    let entry_id = state
        .printer_output
        .last()
        .map_or(1, |entry| entry.entry_id + 1);
    state.printer_output.push(PrinterEntry {
        entry_id,
        text: text.to_string(),
    });
}

fn ending_receipt(conclusion: &str) -> String {
    format!("ENDING // {conclusion}")
}

fn directory_id(digits: [u8; 4]) -> u16 {
    digits
        .iter()
        .fold(0_u16, |value, digit| value * 10 + *digit as u16)
}

fn has_wrong_direct_circuit(cords: &[CordConnection], caller: &PortId, callee: &PortId) -> bool {
    cords.len() == 1
        && cords.iter().any(|cord| {
            let other = if &cord.first == caller {
                Some(&cord.second)
            } else if &cord.second == caller {
                Some(&cord.first)
            } else {
                None
            };
            matches!(other, Some(PortId::Subscriber(line)) if callee != &PortId::Subscriber(*line))
        })
}

fn lamps_for_call(call: Option<&exchange_protocol::CallStatus>) -> [bool; 16] {
    let mut lamps = [false; 16];
    let Some(call) = call else {
        return lamps;
    };
    lamps[call.caller_line as usize] = true;
    if matches!(
        call.phase,
        exchange_protocol::CallPhase::Ringing
            | exchange_protocol::CallPhase::Connected
            | exchange_protocol::CallPhase::Completed
    ) {
        lamps[call.requested_callee_line as usize] = true;
    }
    lamps
}

fn lamps_for_calls(calls: &[exchange_protocol::CallStatus]) -> [bool; 16] {
    let mut lamps = [false; 16];
    for call in calls {
        lamps[call.caller_line as usize] = true;
        if matches!(
            call.phase,
            exchange_protocol::CallPhase::Ringing
                | exchange_protocol::CallPhase::Connected
                | exchange_protocol::CallPhase::Completed
        ) {
            lamps[call.requested_callee_line as usize] = true;
        }
    }
    lamps
}

fn has_tap_bridge_circuit(cords: &[CordConnection], caller: &PortId, callee: &PortId) -> bool {
    (1..=2).any(|bridge| {
        let first = PortId::Tap(bridge * 2 - 1);
        let second = PortId::Tap(bridge * 2);
        has_exact_cords(cords, &[(caller, &first), (callee, &second)])
            || has_exact_cords(cords, &[(caller, &second), (callee, &first)])
    })
}

fn tap_bridge_monitoring(input: &InputState, state: &StateOutput) -> Option<u8> {
    (1..=2).find(|bridge| {
        let held = if *bridge == 1 {
            input.held_controls.tap_1
        } else {
            input.held_controls.tap_2
        };
        held && state.calls.iter().any(|call| {
            matches!(
                call.phase,
                exchange_protocol::CallPhase::Connected | exchange_protocol::CallPhase::Completed
            ) && has_tap_bridge_circuit(
                &input.cord_topology,
                &PortId::Subscriber(call.caller_line),
                &PortId::Subscriber(call.requested_callee_line),
            )
        })
    })
}

fn has_ring_generator(cords: &[CordConnection], caller: &PortId, callee: &PortId) -> bool {
    has_exact_cords(
        cords,
        &[
            (caller, &PortId::Operator),
            (callee, &PortId::RingGenerator),
        ],
    )
}

fn has_exact_cords(cords: &[CordConnection], expected: &[(&PortId, &PortId)]) -> bool {
    cords.len() == expected.len()
        && expected.iter().all(|(first, second)| {
            cords.iter().any(|cord| {
                (&cord.first == *first && &cord.second == *second)
                    || (&cord.first == *second && &cord.second == *first)
            })
        })
}

pub fn serve(listener: TcpListener) -> io::Result<()> {
    serve_with_voice_and_debug(listener, None, None)
}

pub fn serve_with_voice(listener: TcpListener, voice_socket: Option<UdpSocket>) -> io::Result<()> {
    serve_with_voice_and_debug(listener, voice_socket, None)
}

pub fn serve_with_voice_and_debug(
    listener: TcpListener,
    voice_socket: Option<UdpSocket>,
    debug_listener: Option<TcpListener>,
) -> io::Result<()> {
    let backend = Arc::new(Mutex::new(Backend::new_hardware_demo_with_printer_stress(
        printer_stress_enabled(),
    )));
    let voice_socket = voice_socket.map(Arc::new);
    if let Some(debug_listener) = debug_listener {
        let backend = Arc::clone(&backend);
        let diagnostics_backend = Arc::clone(&backend);
        thread::spawn(move || {
            if let Err(error) = serve_debug(debug_listener, backend)
                && let Ok(mut backend) = diagnostics_backend.lock()
            {
                backend.add_diagnostic(BackendDiagnostic {
                    code: "debug_surface_failed".to_string(),
                    message: error.to_string(),
                });
            }
        });
    }
    if let Some(voice_socket) = &voice_socket {
        let backend = Arc::clone(&backend);
        let socket = voice_socket
            .try_clone()
            .map_err(|error| io::Error::other(format!("voice socket clone failed: {error}")))?;
        thread::spawn(move || serve_voice(socket, backend));
    }
    for connection in listener.incoming() {
        let stream = connection?;
        let backend = Arc::clone(&backend);
        let voice_socket = voice_socket.as_ref().map(Arc::clone);
        let diagnostics_backend = Arc::clone(&backend);
        thread::spawn(move || {
            if let Err(error) = handle_connection_with_voice(stream, backend, voice_socket)
                && let Ok(mut backend) = diagnostics_backend.lock()
            {
                backend.add_diagnostic(BackendDiagnostic {
                    code: "frontend_connection_failed".to_string(),
                    message: error.to_string(),
                });
            }
        });
    }
    Ok(())
}

pub fn serve_debug(listener: TcpListener, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    for connection in listener.incoming() {
        let stream = connection?;
        let backend = Arc::clone(&backend);
        let diagnostics_backend = Arc::clone(&backend);
        thread::spawn(move || {
            if let Err(error) = handle_debug_connection(stream, backend)
                && let Ok(mut backend) = diagnostics_backend.lock()
            {
                backend.add_diagnostic(BackendDiagnostic {
                    code: "debug_connection_failed".to_string(),
                    message: error.to_string(),
                });
            }
        });
    }
    Ok(())
}

fn handle_debug_connection(mut stream: TcpStream, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    loop {
        let request: DebugRequest = match read_frame(&mut stream) {
            Ok(request) => request,
            Err(exchange_protocol::FrameError::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(io::Error::new(ErrorKind::InvalidData, error)),
        };
        let response = {
            let mut backend = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            if request.protocol_version != DEBUG_PROTOCOL_VERSION {
                DebugResponse {
                    protocol_version: DEBUG_PROTOCOL_VERSION,
                    accepted: false,
                    error: Some(protocol_error(
                        "unsupported_protocol_version",
                        format!("expected debug protocol version {DEBUG_PROTOCOL_VERSION}"),
                    )),
                    snapshot: backend.debug_snapshot(),
                    audio: None,
                }
            } else {
                backend.apply_debug_command(request.command)
            }
        };
        write_frame(&mut stream, &response)
            .map_err(|error| io::Error::new(ErrorKind::BrokenPipe, error))?;
    }
}

pub fn serve_voice(socket: UdpSocket, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_millis(50)))?;
    let mut datagram = [0_u8; 65_535];
    let mut next_audio_send = None;
    loop {
        let mut peer = None;
        let work = match socket.recv_from(&mut datagram) {
            Ok((length, received_peer)) => {
                peer = Some(received_peer);
                let mut backend = backend
                    .lock()
                    .map_err(|_| io::Error::other("backend state lock poisoned"))?;
                backend.apply_voice_datagram_from(&datagram[..length], Some(received_peer));
                backend.take_voice_input()
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                None
            }
            Err(error) => return Err(error),
        };
        let (outgoing, peer, control) = {
            let mut backend = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            (
                backend.take_voice_datagrams(),
                peer.or(backend.voice_peer),
                backend.take_voice_control(),
            )
        };
        if let Some(peer) = peer {
            send_voice_datagrams(&socket, peer, outgoing, &mut next_audio_send)?;
        }
        if let Some((control, peer)) = control {
            let datagram = encode_voice_control(&control).map_err(|error| {
                io::Error::other(format!("voice control encode failed: {error}"))
            })?;
            socket.send_to(&datagram, peer)?;
        }
        if let Some((
            input,
            voice_id,
            subscriber_line,
            operator_knowledge,
            generation,
            conversation_id,
        )) = work
        {
            let backend = Arc::clone(&backend);
            thread::spawn(move || {
                run_voice_worker(
                    input,
                    voice_id,
                    subscriber_line,
                    operator_knowledge,
                    generation,
                    conversation_id,
                    backend,
                )
            });
        }
    }
}

fn send_voice_datagrams(
    socket: &UdpSocket,
    peer: SocketAddr,
    outgoing: Vec<Vec<u8>>,
    next_audio_send: &mut Option<Instant>,
) -> io::Result<()> {
    for datagram in outgoing {
        let audio_samples = RtpL16Packet::decode(&datagram)
            .ok()
            .map(|packet| packet.samples.len());
        if let Some(audio_samples) = audio_samples {
            if let Some(deadline) = *next_audio_send {
                let now = Instant::now();
                if deadline > now {
                    thread::sleep(deadline.duration_since(now));
                }
            }
            socket.send_to(&datagram, peer)?;
            *next_audio_send = Some(
                Instant::now()
                    + Duration::from_secs_f64(
                        audio_samples as f64 / VOICE_AUDIO_SAMPLE_RATE as f64,
                    ),
            );
        } else {
            socket.send_to(&datagram, peer)?;
        }
    }
    Ok(())
}

pub fn handle_connection(stream: TcpStream, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    handle_connection_with_voice(stream, backend, None)
}

fn handle_connection_with_voice(
    mut stream: TcpStream,
    backend: Arc<Mutex<Backend>>,
    voice_socket: Option<Arc<UdpSocket>>,
) -> io::Result<()> {
    loop {
        let message = match read_frame(&mut stream) {
            Ok(message) => message,
            Err(exchange_protocol::FrameError::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
                ) =>
            {
                if let Ok(mut backend) = backend.lock() {
                    backend.mark_frontend_disconnected();
                }
                return Ok(());
            }
            Err(error) => return Err(io::Error::new(ErrorKind::InvalidData, error)),
        };
        let (response, voice_control) = {
            let mut backend = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            let response = backend.apply_input_message(message);
            (response, backend.take_voice_control())
        };
        if let (Some(socket), Some((control, peer))) = (voice_socket.as_ref(), voice_control) {
            let datagram = encode_voice_control(&control).map_err(|error| {
                io::Error::other(format!("voice control encode failed: {error}"))
            })?;
            socket.send_to(&datagram, peer)?;
        }
        write_frame(&mut stream, &response)
            .map_err(|error| io::Error::new(ErrorKind::BrokenPipe, error))?;
    }
}

fn validate_message(backend: &Backend, message: &InputMessage) -> Result<(), ProtocolError> {
    if message.protocol_version != PROTOCOL_VERSION {
        return Err(protocol_error(
            "unsupported_protocol_version",
            format!("expected protocol version {PROTOCOL_VERSION}"),
        ));
    }
    if message.input_sequence == 0 {
        return Err(protocol_error(
            "invalid_input_sequence",
            "input_sequence must be positive",
        ));
    }
    if message.input_sequence
        <= backend
            .last_request
            .as_ref()
            .map_or(0, |request| request.input_sequence)
    {
        return Err(protocol_error(
            "duplicate_input_sequence",
            "input_sequence must increase, or repeat the exact previous request",
        ));
    }
    if message.expected_state_revision != backend.state_revision {
        return Err(protocol_error(
            "stale_state_revision",
            format!(
                "expected state revision {}, received {}",
                backend.state_revision, message.expected_state_revision
            ),
        ));
    }

    let input = &message.input;
    validate_cords(&input.cord_topology)?;
    if input.directory_digits.iter().any(|digit| *digit > 9) {
        return Err(protocol_error(
            "invalid_directory_digits",
            "directory digits must be decimal digits",
        ));
    }
    if !valid_crank_history(input.crank_rotation_timestamps) {
        return Err(protocol_error(
            "invalid_crank_timestamps",
            "crank rotation timestamps must be strictly increasing after leading zeroes",
        ));
    }
    if input.tuning.coarse > 1_023 || input.tuning.fine > 1_023 {
        return Err(protocol_error(
            "invalid_tuning",
            "tuning values must be between 0 and 1023",
        ));
    }
    if input.debug.device_faults.len() > MAX_FAULTS
        || input
            .debug
            .device_faults
            .iter()
            .any(|fault| fault.len() > 128)
    {
        return Err(protocol_error(
            "invalid_diagnostics",
            "frontend diagnostics contain too many or too-long faults",
        ));
    }
    Ok(())
}

fn validate_cords(cords: &[CordConnection]) -> Result<(), ProtocolError> {
    if cords.len() > MAX_CORDS {
        return Err(protocol_error(
            "too_many_cords",
            "an input cannot contain more than 8 cords",
        ));
    }
    let mut ports = Vec::with_capacity(cords.len() * 2);
    for cord in cords {
        if cord.first == cord.second {
            return Err(protocol_error(
                "invalid_cord",
                "a cord must connect two different ports",
            ));
        }
        validate_port(&cord.first)?;
        validate_port(&cord.second)?;
        if ports.contains(&cord.first) || ports.contains(&cord.second) {
            return Err(protocol_error(
                "duplicate_port",
                "a port may appear in only one cord",
            ));
        }
        ports.push(cord.first.clone());
        ports.push(cord.second.clone());
    }
    Ok(())
}

fn validate_port(port: &PortId) -> Result<(), ProtocolError> {
    match port {
        PortId::Subscriber(line) if *line < 16 => Ok(()),
        PortId::Tap(index) if (1..=4).contains(index) => Ok(()),
        PortId::Operator | PortId::RingGenerator => Ok(()),
        PortId::Subscriber(_) => Err(protocol_error(
            "invalid_port",
            "subscriber ports must be numbered 0 through 15",
        )),
        PortId::Tap(_) => Err(protocol_error(
            "invalid_port",
            "Tap Bridge jacks must be tap_1 through tap_4",
        )),
    }
}

fn rejected_response(
    input_sequence: u64,
    error: ProtocolError,
    state_revision: u64,
    output: &StateOutput,
) -> StateMessage {
    StateMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence,
        accepted: false,
        error: Some(error),
        state_revision,
        output: output.clone(),
    }
}

fn protocol_error(code: &str, message: impl Into<String>) -> ProtocolError {
    ProtocolError {
        code: code.to_string(),
        message: message.into(),
    }
}

fn printer_stress_enabled() -> bool {
    matches!(env::var("NN_BACKEND_PRINTER_STRESS").as_deref(), Ok("1"))
}

struct ReceivedCapture {
    samples: Option<Vec<i16>>,
    active: bool,
}

impl ReceivedCapture {
    fn new(samples: Vec<i16>) -> Self {
        Self {
            samples: Some(samples),
            active: false,
        }
    }
}

impl MicrophoneCapture for ReceivedCapture {
    fn start(&mut self) -> Result<(), VoiceError> {
        self.active = true;
        Ok(())
    }

    fn finish(&mut self) -> Result<Vec<i16>, VoiceError> {
        if !self.active {
            return Err(VoiceError::new(
                "capture_not_started",
                "remote voice capture was not started",
            ));
        }
        self.active = false;
        Ok(self.samples.take().unwrap_or_default())
    }
}

struct BackendVoiceOutput {
    backend: Arc<Mutex<Backend>>,
    generation: u64,
    conversation_id: u64,
}

impl VoiceOutput for BackendVoiceOutput {
    fn status(&mut self, message: exchange_protocol::VoiceStatusMessage) -> Result<(), VoiceError> {
        let relay_message = exchange_protocol::VoiceStatusMessage {
            transcript: None,
            response_text: None,
            ..message.clone()
        };
        let datagram = exchange_protocol::encode_voice_status(&relay_message)
            .map_err(|error| VoiceError::new("voice_status_encode_failed", error.to_string()))?;
        let mut backend = self.backend.lock().map_err(|_| {
            VoiceError::new("voice_backend_lock_failed", "backend state lock poisoned")
        })?;
        if backend.run_generation != self.generation {
            return Err(VoiceError::new(
                "stale_voice_worker",
                "voice worker belongs to a reset Run",
            ));
        }
        backend.voice_status = Some(message.status);
        if message.transcript.is_some() {
            backend.voice_transcript = message.transcript.clone();
        }
        if message.response_text.is_some() {
            backend.voice_response_text = message.response_text.clone();
        }
        backend.voice_speaker_active = matches!(message.status, VoiceStatus::Playing);
        backend.update_voice_conversation(
            self.conversation_id,
            message.status,
            message.error.clone(),
            message.transcript.clone(),
            message.response_text.clone(),
        );
        if message.status == VoiceStatus::Failed {
            backend.add_diagnostic(BackendDiagnostic {
                code: "voice_worker_failed".to_string(),
                message: message.error.map_or_else(
                    || "voice worker failed without details".to_string(),
                    |error| format!("{}: {}", error.code, error.message),
                ),
            });
        }
        backend.queue_voice_datagram(datagram);
        Ok(())
    }

    fn audio(&mut self, packet: exchange_protocol::RtpL16Packet) -> Result<(), VoiceError> {
        let mut backend = self.backend.lock().map_err(|_| {
            VoiceError::new("voice_backend_lock_failed", "backend state lock poisoned")
        })?;
        if backend.run_generation != self.generation {
            return Err(VoiceError::new(
                "stale_voice_worker",
                "voice worker belongs to a reset Run",
            ));
        }
        if let Some(conversation) = backend.voice_conversation_mut(self.conversation_id) {
            conversation.tts_audio.extend_from_slice(&packet.samples);
            conversation.summary.tts_samples =
                conversation.tts_audio.len().min(u32::MAX as usize) as u32;
        }
        backend.queue_voice_datagram(packet.encode());
        Ok(())
    }
}

fn run_voice_worker(
    input: VoiceInputAudioMessage,
    voice_id: String,
    subscriber_line: u8,
    operator_knowledge: Vec<String>,
    generation: u64,
    conversation_id: u64,
    backend: Arc<Mutex<Backend>>,
) {
    let session_id = input.session_id;
    let turn_id = input.turn_id;
    let state_revision = input.state_revision;
    let output: Box<dyn VoiceOutput> = Box::new(BackendVoiceOutput {
        backend: Arc::clone(&backend),
        generation,
        conversation_id,
    });
    let result =
        run_voice_worker_session(input, voice_id, subscriber_line, operator_knowledge, output);
    if let Err(error) = result {
        let mut backend = backend
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if backend.run_generation == generation {
            if let Ok(datagram) =
                exchange_protocol::encode_voice_status(&exchange_protocol::VoiceStatusMessage {
                    protocol_version: exchange_protocol::VOICE_PROTOCOL_VERSION,
                    session_id,
                    turn_id,
                    state_revision,
                    status: VoiceStatus::Failed,
                    transcript: None,
                    response_text: None,
                    error: Some(ProtocolError {
                        code: error.code.clone(),
                        message: error.message.clone(),
                    }),
                })
            {
                backend.queue_voice_datagram(datagram);
            }
            backend.voice_status = Some(VoiceStatus::Failed);
            backend.voice_speaker_active = false;
            backend.fail_voice_conversation(conversation_id, &error);
            backend.add_diagnostic(BackendDiagnostic {
                code: "voice_worker_failed".to_string(),
                message: format!("{}: {}", error.code, error.message),
            });
            backend.voice_worker_active = false;
        }
    } else {
        let mut backend = backend
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if backend.run_generation == generation {
            backend.finish_voice_worker();
        }
    }
}

fn run_voice_worker_session(
    input: VoiceInputAudioMessage,
    voice_id: String,
    subscriber_line: u8,
    operator_knowledge: Vec<String>,
    output: Box<dyn VoiceOutput>,
) -> Result<(), VoiceError> {
    let stt = CommandSpec::from_words(&env::var("NN_VOICE_STT_COMMAND").map_err(|_| {
        VoiceError::new(
            "voice_worker_not_configured",
            "NN_VOICE_STT_COMMAND is not configured",
        )
    })?)?;
    let dialogue =
        CommandSpec::from_words(&env::var("NN_VOICE_DIALOGUE_COMMAND").map_err(|_| {
            VoiceError::new(
                "voice_worker_not_configured",
                "NN_VOICE_DIALOGUE_COMMAND is not configured",
            )
        })?)?;
    let tts = CommandSpec::from_words(&env::var("NN_VOICE_TTS_COMMAND").map_err(|_| {
        VoiceError::new(
            "voice_worker_not_configured",
            "NN_VOICE_TTS_COMMAND is not configured",
        )
    })?)?;
    let tts: Box<dyn TextToSpeech> = if env::var_os("NN_VOICE_TTS_PERSISTENT").is_some() {
        Box::new(PersistentQwen3TtsCommand::new(tts)?)
    } else {
        Box::new(Qwen3TtsCommand::new(tts))
    };
    let mut session = OperatorSession::new(
        input.session_id,
        input.state_revision,
        demo_response_context(subscriber_line, voice_id, operator_knowledge),
        Box::new(ReceivedCapture::new(input.samples)),
        Box::new(CommandSpeechToText::new(stt)),
        Box::new(CommandDialogueGenerator::new(dialogue)),
        tts,
        output,
    )?;
    session.start_ptt()?;
    session.release_ptt().map(|_| ())
}

fn demo_response_context(
    subscriber_line: u8,
    voice_id: String,
    operator_knowledge: Vec<String>,
) -> ResponseContext {
    let (subscriber_id, name, personality, goal, premise, direction, relationship) =
        match subscriber_line {
            0 => (
                1,
                "Taren Kesh",
                "precise railway dispatcher under pressure",
                "Keep the railway moving",
                "Connect me to the Records Office, Subscriber 0002.",
                "Ask for a four-digit Subscriber ID before routing",
                "Vira Dhal is a trusted records clerk",
            ),
            1 => (
                2,
                "Vira Dhal",
                "careful factory records clerk protecting a fragile supply ledger",
                "Keep the relief records moving",
                "Connect me to Rail Dispatch, Subscriber 0001.",
                "Give the Operator the requested public Subscriber ID",
                "Taren Kesh works the Rail Dispatch desk",
            ),
            2 => (
                3,
                "Dr. Leya Varan",
                "direct emergency physician balancing triage and family duty",
                "Get help to the parent collapse",
                "Connect me to the Border Post, Subscriber 0004.",
                "Ask for the emergency connection by Subscriber ID",
                "Oren Vey controls the border post response",
            ),
            3 => (
                4,
                "Captain Oren Vey",
                "disciplined State Protection Directorate captain weighing order against civilians",
                "Keep the border post under control",
                "Connect me to the Records Office, Subscriber 0002.",
                "State the condition of the corridor plainly",
                "Vira Dhal holds records the Directorate wants",
            ),
            4 => (
                5,
                "Neri Tal",
                "alert railway signal operator who notices patterns before officials do",
                "Warn the exchange about the intercepted signal",
                "Connect me to the Records Office, Subscriber 0002.",
                "Describe the signal without inventing its source",
                "Taren Kesh works the Rail Dispatch desk",
            ),
            _ => (
                1,
                "Taren Kesh",
                "precise railway dispatcher under pressure",
                "Keep the railway moving",
                "Connect me to the Records Office, Subscriber 0002.",
                "Ask for a four-digit Subscriber ID before routing",
                "Vira Dhal is a trusted records clerk",
            ),
        };
    let mut permitted_knowledge = vec![match subscriber_line {
        0 => KnowledgeRecord {
            fact: "Taren Kesh is waiting on a relief train record".to_string(),
            learned_from: "railway_dispatch_call".to_string(),
        },
        1 => KnowledgeRecord {
            fact: "The directory lists the Rail Dispatch Subscriber".to_string(),
            learned_from: "directory_terminal".to_string(),
        },
        2 => KnowledgeRecord {
            fact: "The exchange can place an EMS Service Call".to_string(),
            learned_from: "clinic_protocol".to_string(),
        },
        3 => KnowledgeRecord {
            fact: "The border post is under emergency authority".to_string(),
            learned_from: "directorate_notice".to_string(),
        },
        4 => KnowledgeRecord {
            fact: "The railway signal repeated after the relief train request".to_string(),
            learned_from: "signal_box_log".to_string(),
        },
        _ => KnowledgeRecord {
            fact: "The directory lists Vira Dhal".to_string(),
            learned_from: "directory_terminal".to_string(),
        },
    }];
    permitted_knowledge.extend(operator_knowledge.into_iter().map(|fact| KnowledgeRecord {
        fact,
        learned_from: "tap_bridge_monitoring".to_string(),
    }));
    ResponseContext {
        profile: SubscriberProfile {
            subscriber_id,
            name: name.to_string(),
            voice_id,
            personality: personality.to_string(),
            baseline_goals: vec![goal.to_string()],
            initial_perspective: "The exchange is under observation".to_string(),
            relationships: vec![RelationshipNote {
                subject: relationship.to_string(),
                note: "Relevant to the current Call".to_string(),
            }],
            permitted_actions: vec!["request_routing".to_string()],
        },
        subscriber_goal: goal.to_string(),
        call_premise: premise.to_string(),
        story_beat_direction: direction.to_string(),
        permitted_knowledge,
        beliefs: Vec::new(),
        relationship_notes: Vec::new(),
        memories: Vec::new(),
        recent_conversation: Vec::<ConversationTurn>::new(),
        current_input: None,
    }
}

fn initial_state(
    printer_stress: bool,
    required_service_calls: u32,
    required_service_kind: Option<ServiceKind>,
) -> StateOutput {
    StateOutput {
        line_lamps: [false; 16],
        game_phase: GamePhase::Ready,
        clock: ClockState {
            shift: 1,
            elapsed_seconds: accelerated_clock_seconds(0),
        },
        speaker_active: false,
        interference_level: 0,
        tuning: TuningState::default(),
        directory_pages: directory_pages([0, 0, 0, 1]),
        printer_output: initial_printer_output(
            printer_stress,
            required_service_calls,
            required_service_kind,
        ),
        call: None,
        calls: Vec::new(),
        service_call: None,
        tap_bridge_monitoring: None,
        shift: ShiftStatus {
            number: 1,
            phase: ShiftPhase::Ready,
            active_call_count: 0,
            completed_routings: 0,
            required_service_calls,
            completed_service_calls: 0,
            service_errors: 0,
            service_error_counts: Vec::new(),
        },
        debug: OutputDebug {
            messages: vec![BackendDiagnostic {
                code: "backend_ready".to_string(),
                message: "accepting Cabinet Frontend input".to_string(),
            }],
        },
    }
}

fn initial_printer_output(
    printer_stress: bool,
    required_service_calls: u32,
    required_service_kind: Option<ServiceKind>,
) -> Vec<PrinterEntry> {
    if !printer_stress {
        let mut entries = vec![PrinterEntry {
            entry_id: 1,
            text: "PROVINCIAL EXCHANGE READY // SHIFT CLOCK 08:00-16:00".to_string(),
        }];
        if required_service_calls > 0
            && let Some(service) = required_service_kind
        {
            entries.push(PrinterEntry {
                entry_id: 2,
                text: format!(
                    "SERVICE RULE // {} REQUIRED THIS SHIFT",
                    service_label(service)
                ),
            });
        }
        return entries;
    }

    (1..=STRESS_PRINTER_ENTRY_COUNT)
        .map(|entry_id| PrinterEntry {
            entry_id: entry_id as u64,
            text: format!("PRINTER STRESS LINE {entry_id:02} // PAPER CHECK"),
        })
        .collect()
}

fn directory_pages(digits: [u8; 4]) -> Vec<DirectoryPage> {
    let id = digits
        .iter()
        .fold(0_u16, |value, digit| value * 10 + *digit as u16);
    let record = match id {
        1 => Some((
            "TAREN KESH",
            "RAIL DISPATCH",
            "Railway dispatcher",
            "Rail service",
            "Public dispatch desk",
        )),
        2 => Some((
            "VIRA DHAL",
            "RECORDS OFFICE",
            "Factory records clerk",
            "Relief records",
            "Public records contact",
        )),
        3 => Some((
            "DR. LEYA VARAN",
            "KHARAD CLINIC",
            "Emergency physician",
            "Emergency medical service",
            "Clinic duty line",
        )),
        4 => Some((
            "CAPTAIN OREN VEY",
            "BORDER POST",
            "State Protection Directorate",
            "Border security",
            "Public authority contact",
        )),
        5 => Some((
            "NERI TAL",
            "SIGNAL BOX",
            "Railway signal operator",
            "Rail service",
            "Signal maintenance desk",
        )),
        _ => None,
    };

    match record {
        Some((heading, listing, role, affiliation, note)) => vec![
            DirectoryPage {
                page_number: 1,
                heading: heading.to_string(),
                lines: vec![
                    format!("SUBSCRIBER ID {id:04}"),
                    format!("LINE LISTING // {listing}"),
                    role.to_string(),
                ],
            },
            DirectoryPage {
                page_number: 2,
                heading: "PROVINCIAL EXCHANGE".to_string(),
                lines: vec![
                    format!("AFFILIATION // {affiliation}"),
                    format!("PUBLIC NOTE // {note}"),
                ],
            },
        ],
        None => vec![DirectoryPage {
            page_number: 1,
            heading: "NO RECORD".to_string(),
            lines: vec![
                format!("SUBSCRIBER ID {id:04}"),
                "CHECK DIRECTORY SELECTION".to_string(),
            ],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_audio_datagrams_are_sent_at_realtime_rate() {
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let peer = receiver.local_addr().unwrap();
        let packet = RtpL16Packet {
            marker: false,
            sequence: 0,
            timestamp: 0,
            ssrc: 1,
            samples: vec![1; exchange_protocol::VOICE_AUDIO_PACKET_SAMPLES],
        }
        .encode();
        let started = Instant::now();
        let mut next_audio_send = None;

        send_voice_datagrams(
            &sender,
            peer,
            vec![packet.clone(), packet.clone(), packet],
            &mut next_audio_send,
        )
        .unwrap();

        assert!(started.elapsed() >= Duration::from_millis(30));
        let mut buffer = [0_u8; 2_000];
        for _ in 0..3 {
            receiver.recv(&mut buffer).unwrap();
        }
    }

    #[test]
    fn voice_debug_retains_conversation_outputs_and_audio() {
        let backend = Arc::new(Mutex::new(Backend::new()));
        let ready = exchange_protocol::VoiceStatusMessage {
            protocol_version: exchange_protocol::VOICE_PROTOCOL_VERSION,
            session_id: 7,
            turn_id: 3,
            state_revision: 2,
            status: VoiceStatus::Ready,
            transcript: None,
            response_text: None,
            error: None,
        };
        let input = exchange_protocol::VoiceInputAudioMessage {
            protocol_version: exchange_protocol::VOICE_PROTOCOL_VERSION,
            session_id: 7,
            turn_id: 3,
            state_revision: 2,
            chunk_index: 0,
            complete: true,
            samples: vec![10, 20, 30],
        };
        {
            let mut state = backend.lock().unwrap();
            assert!(
                state
                    .apply_voice_datagram(&exchange_protocol::encode_voice_status(&ready).unwrap())
            );
            assert!(state.apply_voice_datagram(
                &exchange_protocol::encode_voice_input_audio(&input).unwrap()
            ));
            let (_, _, _, _, generation, conversation_id) = state.take_voice_input().unwrap();
            drop(state);

            let mut output = BackendVoiceOutput {
                backend: Arc::clone(&backend),
                generation,
                conversation_id,
            };
            output
                .status(exchange_protocol::VoiceStatusMessage {
                    status: VoiceStatus::Playing,
                    transcript: Some("heard words".to_string()),
                    response_text: Some("spoken response".to_string()),
                    ..ready.clone()
                })
                .unwrap();
            output
                .audio(RtpL16Packet {
                    marker: true,
                    sequence: 0,
                    timestamp: 0,
                    ssrc: 7,
                    samples: vec![40, 50],
                })
                .unwrap();
            let partial_audio =
                backend
                    .lock()
                    .unwrap()
                    .apply_debug_command(DebugCommand::GetVoiceAudio {
                        conversation_id,
                        kind: exchange_protocol::DebugAudioKind::Tts,
                    });
            assert!(!partial_audio.accepted);
            output
                .status(exchange_protocol::VoiceStatusMessage {
                    status: VoiceStatus::Completed,
                    ..ready
                })
                .unwrap();
        }

        let snapshot = backend.lock().unwrap().debug_snapshot();
        let conversation = &snapshot.voice.conversations[0];
        assert_eq!(conversation.transcript.as_deref(), Some("heard words"));
        assert_eq!(
            conversation.response_text.as_deref(),
            Some("spoken response")
        );
        assert_eq!(conversation.captured_samples, 3);
        assert_eq!(conversation.tts_samples, 2);

        let audio = backend
            .lock()
            .unwrap()
            .apply_debug_command(DebugCommand::GetVoiceAudio {
                conversation_id: conversation.id,
                kind: exchange_protocol::DebugAudioKind::Tts,
            });
        assert!(audio.accepted);
        assert_eq!(audio.audio.unwrap().samples, vec![40, 50]);
    }
}
