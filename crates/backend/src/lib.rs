//! Telephone-exchange backend.
//!
//! The backend keeps the public protocol-facing `Backend` interface here. The
//! supporting implementations are deliberately split by responsibility:
//! configuration (`config`), call state (`calls`), physical exchange rules
//! (`hardware`), story definitions (`stories`), and voice workers.

use std::collections::{HashMap, VecDeque};
use std::env;
use std::io::{self, ErrorKind};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use exchange_protocol::{
    BackendDiagnostic, CallPhase, CallStatus, DEBUG_PROTOCOL_VERSION, DebugActiveCall,
    DebugCallRecord, DebugCommand, DebugCounters, DebugFrontendState, DebugRequest, DebugResponse,
    DebugRunState, DebugSnapshot, DebugSubscriberState, DebugVoiceConversation, FrameError,
    GamePhase, HeldControls, InputMessage, InputState, PROTOCOL_VERSION, PortId, ProtocolError,
    RtpL16Packet, ShiftPhase, StateMessage, StateOutput, TEXT_PROTOCOL_VERSION, TextInputMessage,
    TextResponseMessage, TextStatus, VOICE_AUDIO_PACKET_SAMPLES, VOICE_AUDIO_SAMPLE_RATE,
    VOICE_PROTOCOL_VERSION, VoiceControl, VoiceControlMessage, VoiceStatus, VoiceStatusMessage,
    decode_voice_input_audio, decode_voice_status, encode_voice_control, encode_voice_status,
    read_frame, write_frame,
};
mod calls;
mod config;
mod hardware;
#[allow(dead_code)]
mod voice_workers;

use voice_workers::{
    CommandSpec, CommandSpeechToText, CommandTextClassifier, ConversationTurn, DialogueGenerator,
    PersistentCommandDialogueGenerator, PersistentPocketTtsCommand, ResponseContext, SpeechToText,
    SubscriberProfile, TextClassifier, TextToSpeech, VoiceError,
};
mod stories;

use calls::ActiveCall;
use config::{GameConfig, LINES, SubscriberConfig};
use hardware::*;
const MAX_AUDIO_PACKETS: usize = 4096;
static DIALOGUE_WORKER: OnceLock<Mutex<Option<PersistentCommandDialogueGenerator>>> =
    OnceLock::new();
static POCKET_TTS_WORKER: OnceLock<Mutex<Option<PersistentPocketTtsCommand>>> = OnceLock::new();
static TEXT_TURN_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub struct Backend {
    config: GameConfig,
    state: StateOutput,
    revision: u64,
    sequence: Option<u64>,
    clock_started: Instant,
    debug_elapsed: u64,
    rng: u64,
    calls: Vec<ActiveCall>,
    call_history: Vec<DebugCallRecord>,
    line_limit: u8,
    call_target: usize,
    shift_started_elapsed_seconds: u64,
    next_call_arrival_elapsed_seconds: u64,
    resolved: u8,
    earned: i32,
    deductions: i32,
    money: i32,
    completed: u32,
    missed: u32,
    failed: u32,
    conversation_seconds: u64,
    run_generation: u64,
    last_request: Option<InputMessage>,
    last_response: Option<StateMessage>,
    audio_queue: VecDeque<Vec<u8>>,
    audio_call: Option<(u8, u8)>,
    audio_replay: Vec<Vec<u8>>,
    audio_replay_call: Option<(u8, u8)>,
    audio_sequence: u16,
    audio_timestamp: u32,
    audio_tap_was_active: bool,
    pending_tts: Vec<(u8, u8, bool)>,
    tts_prepared: bool,
    last_ptt: bool,
    voice_peer: Option<SocketAddr>,
    voice_session_id: u64,
    voice_state_revision: u64,
    pending_voice_control: Option<VoiceControlMessage>,
    voice_turn_id: u64,
    next_voice_turn_id: u64,
    voice_subscriber_line: Option<u8>,
    voice_callee_line: Option<u8>,
    voice_status: Option<VoiceStatus>,
    voice_transcript: Option<String>,
    voice_response_text: Option<String>,
    voice_speaker_active: bool,
    cancelled_voice_turn: Option<u64>,
    voice_conversations: Vec<DebugVoiceConversation>,
    next_voice_conversation_id: u64,
    last_input_json: Option<String>,
    last_output_json: Option<String>,
    story_beat: stories::shapla_apartments::Beat,
    neel_story_beat: stories::neel_university::Beat,
    story_controls: HeldControls,
    story_enabled: bool,
    story_started_elapsed_seconds: u64,
    voice_turn_controls: HeldControls,
    story_reward_paid: bool,
    story_followup_pending: bool,
    story_followup_call_started: bool,
    shapla_story_completed: bool,
    story_completed: bool,
    story_thread: String,
    story_conversations: HashMap<u8, Vec<ConversationTurn>>,
    ring_active_line: i16,
}

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend {
    fn subscriber(&self, line: u8) -> &SubscriberConfig {
        self.config
            .subscribers
            .iter()
            .find(|subscriber| subscriber.line == line)
            .unwrap_or_else(|| panic!("exchange config has no subscriber for line {line}"))
    }

    fn neel_story_active(&self) -> bool {
        self.story_enabled
    }

    fn shapla_story_active(&self) -> bool {
        self.story_enabled
    }

    fn is_neel_caller(&self, caller: u8) -> bool {
        self.neel_story_active()
            && matches!(
                caller,
                stories::neel_university::NEEL_LINE | stories::neel_university::SHADHIN_LINE
            )
    }

    pub fn new() -> Self {
        Self::new_exchange()
    }
    pub fn new_exchange() -> Self {
        let config = GameConfig::load();
        let backend = Self {
            config: config.clone(),
            state: initial_state(),
            revision: 0,
            sequence: None,
            clock_started: Instant::now(),
            debug_elapsed: 0,
            rng: config.story_seed,
            calls: Vec::new(),
            call_history: Vec::new(),
            line_limit: LINES,
            call_target: config.active_calls,
            shift_started_elapsed_seconds: 0,
            next_call_arrival_elapsed_seconds: 0,
            resolved: 0,
            earned: 0,
            deductions: 0,
            money: 0,
            completed: 0,
            missed: 0,
            failed: 0,
            conversation_seconds: 0,
            run_generation: 0,
            last_request: None,
            last_response: None,
            audio_queue: VecDeque::new(),
            audio_call: None,
            audio_replay: Vec::new(),
            audio_replay_call: None,
            audio_sequence: 0,
            audio_timestamp: 0,
            audio_tap_was_active: false,
            pending_tts: Vec::new(),
            tts_prepared: false,
            last_ptt: false,
            voice_peer: None,
            voice_session_id: 0,
            voice_state_revision: 0,
            pending_voice_control: None,
            voice_turn_id: 0,
            next_voice_turn_id: 1,
            voice_subscriber_line: None,
            voice_callee_line: None,
            voice_status: None,
            voice_transcript: None,
            voice_response_text: None,
            voice_speaker_active: false,
            cancelled_voice_turn: None,
            voice_conversations: Vec::new(),
            next_voice_conversation_id: 1,
            last_input_json: None,
            last_output_json: None,
            story_beat: stories::shapla_apartments::Beat::EmergencyCall,
            neel_story_beat: stories::neel_university::Beat::ProfessorRouting,
            story_controls: HeldControls::default(),
            story_enabled: true,
            story_started_elapsed_seconds: 0,
            voice_turn_controls: HeldControls::default(),
            story_reward_paid: false,
            story_followup_pending: false,
            story_followup_call_started: false,
            shapla_story_completed: false,
            story_completed: false,
            story_thread: "intertwined".into(),
            story_conversations: HashMap::new(),
            ring_active_line: -1,
        };
        backend
    }
    pub fn new_simple_hardware_demo() -> Self {
        let mut backend = Self::new_exchange();
        backend.call_target = backend.config.active_calls;
        backend.line_limit = 6;
        backend.tts_prepared = true;
        backend.story_enabled = false;
        backend.calls.clear();
        backend.next_call_arrival_elapsed_seconds = backend.elapsed_seconds() as u64;
        backend.config.call_arrival_interval_seconds = 0;
        backend.refill_calls(2);
        backend.state.calls = backend
            .calls
            .iter()
            .map(|call| CallStatus {
                caller_line: call.caller,
                requested_callee_line: call.callee,
                phase: call.phase.clone(),
            })
            .collect();
        backend.state.line_lamps = lamps(&backend.calls, -1);
        backend.state.shift.active_call_count = backend.calls.len() as u8;
        backend
    }
    pub fn new_simple_hardware_demo_with_printer_stress(_: bool) -> Self {
        Self::new_simple_hardware_demo()
    }
    pub fn new_hardware_demo() -> Self {
        let mut backend = Self::new_exchange();
        backend.tts_prepared = true;
        backend
    }
    pub fn new_hardware_demo_with_printer_stress(_: bool) -> Self {
        Self::new_hardware_demo()
    }
    pub fn frontend_state(&self) -> &StateOutput {
        &self.state
    }
    pub fn money(&self) -> i32 {
        self.money
    }

    fn take_voice_control(&mut self) -> Option<(VoiceControlMessage, SocketAddr)> {
        Some((self.pending_voice_control.take()?, self.voice_peer?))
    }

    fn set_voice_peer(&mut self, peer: SocketAddr, message: &VoiceStatusMessage) -> bool {
        if message.session_id != 1 {
            return false;
        }
        if let Some(existing) = self.voice_peer {
            if existing != peer {
                return false;
            }
        }
        self.voice_peer = Some(peer);
        self.voice_session_id = message.session_id;
        self.voice_status = Some(message.status);
        true
    }

    fn voice_input_is_current(&self, input: &exchange_protocol::VoiceInputAudioMessage) -> bool {
        self.voice_peer.is_some()
            && input.session_id == self.voice_session_id
            && input.turn_id == self.voice_turn_id
            && input.state_revision == self.voice_state_revision
            && !self.state.tap_bridge_audio_active
    }

    fn voice_context(&self, caller: u8, callee: u8) -> ResponseContext {
        let caller_profile = self.subscriber(caller);
        let callee_profile = self.subscriber(callee);
        ResponseContext {
            profile: SubscriberProfile {
                subscriber_id: caller,
                name: caller_profile.name.clone(),
                voice_id: caller_profile.voice_id.clone(),
                personality: caller_profile.role.clone(),
                baseline_goals: Vec::new(),
                initial_perspective: String::new(),
                permitted_actions: Vec::new(),
            },
            caller_place: caller_profile.place.clone(),
            requested_place: callee_profile.place.clone(),
            known_places: self
                .config
                .subscribers
                .iter()
                .map(|s| s.place.clone())
                .collect(),
            subscriber_goal: String::new(),
            call_premise: String::new(),
            call_guidance: if self.is_neel_caller(caller) {
                format!(
                    "{}\nStory place: {}",
                    if caller == stories::neel_university::NEEL_LINE {
                        stories::neel_university::Beat::ProfessorRouting.dialogue_prompt()
                    } else {
                        stories::neel_university::Beat::ArnabDirectory.dialogue_prompt()
                    },
                    self.subscriber(caller).place,
                )
            } else if self.shapla_story_active()
                && caller == stories::shapla_apartments::CALLER_LINE
            {
                format!(
                    "{}\nStory place: {}\nOpening dialogue: {}",
                    self.story_beat.dialogue_prompt(),
                    stories::shapla_apartments::PLACE,
                    self.story_beat.opening_dialogue().unwrap_or(""),
                )
            } else {
                String::new()
            },
            permitted_knowledge: vec![],
            recent_conversation: self
                .story_conversations
                .get(&caller)
                .cloned()
                .unwrap_or_default(),
            current_input: None,
        }
    }

    fn story_caller(&self) -> u8 {
        stories::shapla_apartments::CALLER_LINE
    }

    fn story_requested_callee(&self) -> u8 {
        self.neel_story_active()
            .then(|| stories::neel_university::requested_callee_for_beat(self.neel_story_beat))
            .unwrap_or(0)
    }

    pub fn reset_run(&mut self) {
        self.run_generation = self.run_generation.wrapping_add(1);
        self.state = initial_state();
        self.revision = 0;
        self.sequence = None;
        self.clock_started = Instant::now();
        self.debug_elapsed = 0;
        self.calls.clear();
        self.call_history.clear();
        self.resolved = 0;
        self.earned = 0;
        self.deductions = 0;
        self.money = 0;
        self.completed = 0;
        self.missed = 0;
        self.failed = 0;
        self.conversation_seconds = 0;
        self.shift_started_elapsed_seconds = 0;
        self.next_call_arrival_elapsed_seconds = 0;
        self.audio_queue.clear();
        self.audio_call = None;
        self.audio_replay.clear();
        self.audio_replay_call = None;
        self.audio_sequence = 0;
        self.audio_timestamp = 0;
        self.audio_tap_was_active = false;
        self.pending_tts.clear();
        self.last_ptt = false;
        self.pending_voice_control = None;
        self.voice_turn_id = 0;
        self.next_voice_turn_id = 1;
        self.voice_subscriber_line = None;
        self.voice_callee_line = None;
        self.voice_status = None;
        self.voice_session_id = 0;
        self.voice_state_revision = 0;
        self.voice_transcript = None;
        self.voice_response_text = None;
        self.voice_speaker_active = false;
        self.story_beat = stories::shapla_apartments::Beat::EmergencyCall;
        self.neel_story_beat = stories::neel_university::Beat::ProfessorRouting;
        self.story_controls = HeldControls::default();
        self.story_started_elapsed_seconds = self.elapsed_seconds() as u64;
        self.voice_turn_controls = HeldControls::default();
        self.story_reward_paid = false;
        self.story_followup_pending = false;
        self.story_followup_call_started = false;
        self.shapla_story_completed = false;
        self.story_completed = false;
        self.story_conversations.clear();
        self.ring_active_line = -1;
        self.cancelled_voice_turn = None;
        self.voice_conversations.clear();
        self.next_voice_conversation_id = 1;
        self.refill_calls(self.call_target);
        self.state.calls = self
            .calls
            .iter()
            .map(|call| CallStatus {
                caller_line: call.caller,
                requested_callee_line: call.callee,
                phase: call.phase.clone(),
            })
            .collect();
        self.state.line_lamps = lamps(&self.calls, -1);
        self.sync_story_lamp();
        self.state.shift.active_call_count = self.calls.len() as u8;
        self.state.game_phase = GamePhase::Shift;
    }

    pub fn debug_snapshot(&self) -> DebugSnapshot {
        let calls = self.state.calls.clone();
        let now = self.elapsed_seconds() as u64;
        let active_calls = self
            .calls
            .iter()
            .map(|call| DebugActiveCall {
                caller_line: call.caller,
                requested_callee_line: call.callee,
                phase: call.phase.clone(),
                started_elapsed_seconds: call.started_elapsed_seconds,
                patience_deadline_elapsed_seconds: call.deadline,
                patience_remaining_seconds: call.deadline.saturating_sub(now),
                ring_started_elapsed_seconds: call.ring_started_at,
                connected_elapsed_seconds: call.connected_elapsed_seconds,
                audio_duration_seconds: call.audio_duration_seconds,
            })
            .collect();
        let subscribers = (0..LINES)
            .map(|line| {
                let active = self.state.line_lamps[line as usize];
                let subscriber = self.subscriber(line);
                DebugSubscriberState {
                    id: format!("line_{line}"),
                    name: subscriber.name.clone(),
                    line: Some(line),
                    status: if active { "off_hook" } else { "on_hook" }.into(),
                    availability: if active { "off_hook" } else { "on_hook" }.into(),
                    pressure: u32::from(active),
                    current_goal: None,
                    status_flags: vec![subscriber.role.clone()],
                }
            })
            .collect();
        DebugSnapshot {
            story_thread: self.story_thread.clone(),
            story_beat: self.current_story_beat_name().into(),
            shapla_story_beat: self.shapla_story_beat_name().into(),
            neel_story_beat: self.neel_story_beat.name().into(),
            story_completed: self.story_completed,
            money: self.money,
            run: DebugRunState {
                number: self.run_generation as u32 + 1,
                state_revision: self.revision,
                elapsed_seconds: self.elapsed_seconds(),
                game_phase: self.state.game_phase.clone(),
                godmode: false,
                bypass_restrictions: false,
            },
            shift: self.state.shift.clone(),
            calls,
            active_calls,
            call_history: self.call_history.clone(),
            subscribers,
            counters: DebugCounters {
                completed_routings: self.completed,
                completed_service_calls: 0,
                required_service_calls: 0,
                service_errors: 0,
                active_call_count: self.state.shift.active_call_count,
            },
            voice: exchange_protocol::DebugVoiceState {
                status: self.voice_status,
                speaker_active: self.voice_speaker_active,
                session_id: (self.voice_session_id != 0).then_some(self.voice_session_id),
                turn_id: (self.voice_turn_id != 0).then_some(self.voice_turn_id),
                transcript: self.voice_transcript.clone(),
                response_text: self.voice_response_text.clone(),
                conversations: self.voice_conversations.clone(),
            },
            frontend: DebugFrontendState {
                firmware_version: None,
                transport_connected: false,
                device_faults: vec![],
                last_input_json: self.last_input_json.clone(),
                last_output_json: self.last_output_json.clone(),
            },
            recent_errors: self.state.debug.messages.clone(),
        }
    }

    fn current_story_beat_name(&self) -> &'static str {
        "Intertwined"
    }

    fn shapla_story_beat_name(&self) -> &'static str {
        match self.story_beat {
            stories::shapla_apartments::Beat::EmergencyCall => "EmergencyCall",
            stories::shapla_apartments::Beat::HappyFollowup => "HappyFollowup",
            stories::shapla_apartments::Beat::NeutralFollowup => "NeutralFollowup",
            stories::shapla_apartments::Beat::BadFollowup => "BadFollowup",
        }
    }

    fn destination_is_allowed(&self, call: &ActiveCall, selected: u16) -> bool {
        if self.neel_story_active()
            && self.neel_story_beat == stories::neel_university::Beat::ArnabDirectory
            && call.caller == stories::neel_university::SHADHIN_LINE
        {
            return stories::neel_university::is_bela_destination(selected)
                || directory_line(selected).is_some_and(|line| {
                    line == stories::neel_university::BELA_DOG_LINE
                        || line == stories::neel_university::BELA_CAT_LINE
                });
        }
        selected == u16::from(call.callee)
    }

    fn story_direct_destination_allowed(&self, input: &InputState, caller: u8) -> bool {
        self.neel_story_active()
            && self.neel_story_beat == stories::neel_university::Beat::ArnabDirectory
            && caller == stories::neel_university::SHADHIN_LINE
            && [
                stories::neel_university::BELA_DOG_LINE,
                stories::neel_university::BELA_CAT_LINE,
            ]
            .iter()
            .any(|line| direct(&input.cord_topology, caller, *line))
    }

    pub fn apply_input_message(&mut self, message: InputMessage) -> StateMessage {
        if self.last_request.as_ref() == Some(&message)
            && let Some(response) = self.last_response.clone()
        {
            return response;
        }
        let result = self.apply_input(message.clone());
        self.last_input_json = serde_json::to_string(&message).ok();
        self.last_output_json = serde_json::to_string(&result).ok();
        self.last_request = Some(message);
        self.last_response = Some(result.clone());
        result
    }

    fn apply_input(&mut self, message: InputMessage) -> StateMessage {
        if message.protocol_version != PROTOCOL_VERSION {
            return rejected(
                message.input_sequence,
                "protocol_version",
                "unsupported protocol version",
                self.revision,
                &self.state,
            );
        }
        if self
            .sequence
            .is_some_and(|last| message.input_sequence <= last)
        {
            return rejected(
                message.input_sequence,
                "input_sequence",
                "input sequence must increase",
                self.revision,
                &self.state,
            );
        }
        if message.expected_state_revision != self.revision {
            return rejected(
                message.input_sequence,
                "state_revision",
                "state revision mismatch",
                self.revision,
                &self.state,
            );
        }
        self.sequence = Some(message.input_sequence);
        let input = &message.input;
        let talk_buttons = u8::from(input.held_controls.ptt)
            + u8::from(input.held_controls.police)
            + u8::from(input.held_controls.ems);
        if talk_buttons > 1 {
            return rejected(
                message.input_sequence,
                "multiple_talk_buttons",
                "press exactly one of PTT, Police, or EMS",
                self.revision,
                &self.state,
            );
        }
        self.debug_elapsed = self.debug_elapsed.saturating_add(0);
        if self.state.shift.phase == ShiftPhase::Ready && self.calls.is_empty() {
            self.refill_calls(self.call_target);
        }
        if self.state.shift.phase == ShiftPhase::Settled
            && self.state.shift.number < 3
            && self.state.game_phase != GamePhase::Ended
        {
            self.refill_calls(self.call_target);
        }
        self.expire_calls();
        self.expire_story();
        self.refill_calls(self.call_target);
        self.update_ring_activation(input);
        self.connect_ready_direct_calls(input);
        if self.state.shift.phase == ShiftPhase::Active
            && self.elapsed_seconds() as u64
                >= self.shift_started_elapsed_seconds + self.config.shift_duration_seconds
            && !self
                .calls
                .iter()
                .any(|call| matches!(call.phase, CallPhase::Held | CallPhase::Connected))
        {
            self.settle_shift();
        }
        let selected = directory_id(input.directory_digits);
        let focused = operator_line(&input.cord_topology)
            .or_else(|| self.state.call.as_ref().map(|call| call.caller_line))
            .or_else(|| {
                self.calls
                    .iter()
                    .find(|call| call.phase == CallPhase::Connected)
                    .map(|call| call.caller)
            });
        let mut error = None;
        if let Some(call) = self.calls.iter().find(|call| Some(call.caller) == focused) {
            if direct(&input.cord_topology, call.caller, call.callee)
                && !self.destination_is_allowed(call, selected)
            {
                error = Some((
                    "directory_selection_required",
                    "select the requested destination before routing",
                ));
            }
            if error.is_none()
                && has_wrong_direct_circuit(input, call.caller, call.callee)
                && !self.story_direct_destination_allowed(input, call.caller)
            {
                error = Some((
                    "wrong_destination",
                    "the direct circuit must use the requested destination line",
                ));
            }
            if error.is_none()
                && call.phase == CallPhase::Connected
                && !input.cord_topology.is_empty()
                && !valid_connected_circuit(input, call.caller, call.callee)
            {
                error = Some((
                    "invalid_cord_topology",
                    "a connected call permits only the direct circuit and optional Tap Bridge cords",
                ));
            }
        }
        if error.is_none() {
            error = self.advance(input, focused, selected);
        }
        if error.is_some_and(|(code, _)| code == "wrong_destination")
            && let Some(line) = focused
            && let Some(index) = self.calls.iter().position(|call| call.caller == line)
        {
            self.fail_call(index, "wrong_destination");
        }
        self.refill_calls(self.call_target);
        self.revision = self.revision.wrapping_add(1);
        self.state.clock.elapsed_seconds = self.elapsed_seconds();
        self.state.directory_pages = directory_pages(input.directory_digits);
        self.state.calls = self
            .calls
            .iter()
            .map(|c| CallStatus {
                caller_line: c.caller,
                requested_callee_line: c.callee,
                phase: c.phase.clone(),
            })
            .collect();
        self.state.call = focused.and_then(|line| {
            self.state
                .calls
                .iter()
                .find(|c| c.caller_line == line)
                .cloned()
        });
        self.state.line_lamps = lamps(&self.calls, self.effective_ring_line(input));
        self.story_controls = input.held_controls.clone();
        self.sync_story_lamp();
        self.state.shift.active_call_count = self.calls.len() as u8;
        self.state.tap_bridge_monitoring = tap_monitor(input, &self.state);
        self.state.tap_bridge_audio_active = self.state.tap_bridge_monitoring.is_some();
        self.update_voice_control(input, self.revision);
        self.cancel_voice_if_operator_disconnected(input);
        self.state.speaker_active = (input.held_controls.ptt
            && !self.state.tap_bridge_audio_active)
            || self.voice_speaker_active;
        self.state.game_phase = if self.state.game_phase == GamePhase::Ended {
            GamePhase::Ended
        } else if self.state.shift.phase == ShiftPhase::Settled {
            GamePhase::Ready
        } else {
            GamePhase::Shift
        };
        if let Some((code, text)) = error {
            return rejected(
                message.input_sequence,
                code,
                text,
                self.revision,
                &self.state,
            );
        }
        StateMessage {
            protocol_version: PROTOCOL_VERSION,
            input_sequence: message.input_sequence,
            accepted: true,
            error: None,
            state_revision: self.revision,
            output: self.state.clone(),
        }
    }

    fn update_voice_control(&mut self, input: &InputState, revision: u64) {
        let talking =
            input.held_controls.ptt || input.held_controls.police || input.held_controls.ems;
        let caller = operator_line(&input.cord_topology)
            .or_else(|| self.state.call.as_ref().map(|call| call.caller_line));
        if let Some(caller) = caller {
            self.voice_subscriber_line = Some(caller);
            self.voice_callee_line = self
                .calls
                .iter()
                .find(|call| call.caller == caller)
                .map(|call| call.callee)
                .or_else(|| {
                    (caller == stories::shapla_apartments::CALLER_LINE)
                        .then_some(stories::shapla_apartments::CALLER_LINE)
                });
        }
        if talking == self.last_ptt {
            return;
        }
        self.last_ptt = talking;
        if talking && self.state.tap_bridge_audio_active {
            return;
        }
        if talking {
            self.voice_turn_controls = input.held_controls.clone();
            self.voice_turn_id = self.next_voice_turn_id;
            self.next_voice_turn_id = self.next_voice_turn_id.wrapping_add(1);
            println!(
                "[TRANSITION] voice start caller={:?} turn={} ptt={} police={} ems={} tap={}",
                caller,
                self.voice_turn_id,
                self.voice_turn_controls.ptt,
                self.voice_turn_controls.police,
                self.voice_turn_controls.ems,
                self.voice_turn_controls.tap
            );
            self.cancelled_voice_turn = None;
        }
        let voice_id = caller
            .map(|line| self.subscriber(line).voice_id.clone())
            .unwrap_or_else(|| self.config.subscribers[0].voice_id.clone());
        self.pending_voice_control = Some(VoiceControlMessage {
            protocol_version: VOICE_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: self.voice_turn_id.max(1),
            state_revision: revision,
            voice_id,
            control: if talking {
                VoiceControl::StartPtt
            } else {
                VoiceControl::ReleasePtt
            },
        });
        if !talking {
            println!(
                "[TRANSITION] voice release caller={:?} turn={} service_turn={} captured_controls_police={} captured_controls_ems={}",
                caller,
                self.voice_turn_id,
                self.voice_turn_controls.police || self.voice_turn_controls.ems,
                self.voice_turn_controls.police,
                self.voice_turn_controls.ems
            );
        }
        self.voice_state_revision = revision;
    }

    fn sync_story_lamp(&mut self) {
        if !self.story_enabled {
            return;
        }
        if self.shapla_story_active() {
            self.state.line_lamps[stories::shapla_apartments::CALLER_LINE as usize] = true;
        }
    }

    fn update_ring_activation(&mut self, input: &InputState) {
        let physical_line = physical_ring_line(input);
        if physical_line < 0 || input.ring_line != physical_line {
            self.ring_active_line = -1;
            return;
        }
        let now = self.elapsed_seconds() as u64;
        let delay = 1 + (self.rng % 3);
        let Some(call) = self.calls.iter_mut().find(|call| {
            call.phase == CallPhase::Ringing && i16::from(call.callee) == physical_line
        }) else {
            self.ring_active_line = -1;
            return;
        };
        let ready_at = *call.ring_ready_at.get_or_insert(now + delay);
        if now >= ready_at {
            self.ring_active_line = physical_line;
            call.ring_activated = true;
        }
    }

    fn effective_ring_line(&self, input: &InputState) -> i16 {
        let physical_line = physical_ring_line(input);
        (self.ring_active_line == physical_line && input.ring_line == physical_line)
            .then_some(physical_line)
            .unwrap_or(-1)
    }

    fn expire_story(&mut self) {
        // Story callers are retained independently and do not expire while
        // the other story is being played.
    }

    fn cancel_voice_if_operator_disconnected(&mut self, input: &InputState) {
        let Some(line) = self.voice_subscriber_line else {
            return;
        };
        if operator_line(&input.cord_topology) == Some(line) {
            return;
        }
        let active = self.voice_speaker_active
            || self
                .voice_status
                .is_some_and(|status| !status.is_terminal());
        if !active {
            return;
        }
        self.cancelled_voice_turn = Some(self.voice_turn_id);
        if self.story_beat == stories::shapla_apartments::Beat::EmergencyCall {
            self.story_beat = stories::shapla_apartments::Beat::BadFollowup;
            self.money -= 100;
            self.deductions += 100;
            append_printer(
                &mut self.state,
                &format!(
                    "MONEY // -$100 abandoned Shapla emergency call // balance ${}",
                    self.money
                ),
            );
        }
        self.voice_status = Some(VoiceStatus::Cancelled);
        self.voice_speaker_active = false;
        self.audio_queue.clear();
        self.audio_call = None;
        let finished_elapsed_seconds = self.elapsed_seconds();
        for conversation in &mut self.voice_conversations {
            if conversation.session_id == self.voice_session_id
                && conversation.turn_id == self.voice_turn_id
                && !conversation
                    .status
                    .is_some_and(|status| status.is_terminal())
            {
                conversation.status = Some(VoiceStatus::Cancelled);
                conversation.finished_elapsed_seconds = Some(finished_elapsed_seconds);
            }
        }
        self.pending_voice_control = Some(VoiceControlMessage {
            protocol_version: VOICE_PROTOCOL_VERSION,
            session_id: self.voice_session_id,
            turn_id: self.voice_turn_id.max(1),
            state_revision: self.revision,
            voice_id: self.subscriber(line).voice_id.clone(),
            control: VoiceControl::Cancel,
        });
    }

    fn apply_story_classification(
        &mut self,
        classification: stories::shapla_apartments::Classification,
    ) {
        let controls = self.voice_turn_controls.clone();
        let police = controls.police;
        let ems = controls.ems;
        println!(
            "[TRANSITION] story classification={:?} police={} ems={} beat_before={:?}",
            classification, police, ems, self.story_beat
        );
        if let Some(next) =
            stories::shapla_apartments::next_beat(self.story_beat, classification, police, ems)
        {
            println!(
                "[TRANSITION] story beat {:?} -> {:?}",
                self.story_beat, next
            );
            self.story_beat = next;
            self.story_followup_pending = true;
            self.story_started_elapsed_seconds = self.elapsed_seconds() as u64;
            append_printer(&mut self.state, &format!("STORY // {:?}", next));
        }
    }

    fn complete_story_followup(&mut self) {
        if matches!(
            self.story_beat,
            stories::shapla_apartments::Beat::HappyFollowup
                | stories::shapla_apartments::Beat::NeutralFollowup
        ) {
            self.shapla_story_completed = true;
        }
        if self.story_beat == stories::shapla_apartments::Beat::HappyFollowup
            && !self.story_reward_paid
        {
            self.money += 100;
            self.earned += 100;
            self.story_reward_paid = true;
            append_printer(&mut self.state, "STORY // caller sent $100");
        }
    }

    fn record_story_turn(&mut self, speaker: &str, text: &str) {
        let Some(caller) = self.voice_subscriber_line else {
            return;
        };
        if !self.story_enabled
            || !matches!(
                caller,
                stories::shapla_apartments::CALLER_LINE
                    | stories::neel_university::NEEL_LINE
                    | stories::neel_university::SHADHIN_LINE
            )
        {
            return;
        }
        let conversation = self.story_conversations.entry(caller).or_default();
        conversation.push(ConversationTurn {
            speaker: speaker.into(),
            text: text.into(),
        });
        if conversation.len() > 6 {
            conversation.remove(0);
        }
    }

    fn advance(
        &mut self,
        input: &InputState,
        focused: Option<u8>,
        selected: u16,
    ) -> Option<(&'static str, &'static str)> {
        let line = focused?;
        let index = self.calls.iter().position(|c| c.caller == line)?;
        if line == stories::shapla_apartments::CALLER_LINE && input.cord_topology.is_empty() {
            if self.story_beat == stories::shapla_apartments::Beat::EmergencyCall
                && self
                    .story_conversations
                    .get(&line)
                    .is_some_and(|conversation| conversation.len() >= 2)
            {
                self.story_beat = stories::shapla_apartments::Beat::BadFollowup;
                self.story_followup_pending = true;
                self.story_started_elapsed_seconds = self.elapsed_seconds() as u64;
                append_printer(&mut self.state, "STORY // operator abandoned the caller");
            }
            if self.story_beat == stories::shapla_apartments::Beat::EmergencyCall {
                self.calls.remove(index);
                return None;
            }
            self.calls.remove(index);
            if self.story_followup_call_started {
                self.shapla_story_completed = true;
                self.story_completed =
                    self.neel_story_beat == stories::neel_university::Beat::Completed;
            }
            return None;
        }
        let mut finish = false;
        let mut connect = false;
        let mut error = None;
        let now = self.elapsed_seconds() as u64;
        let active_ring_line = self.effective_ring_line(input);
        let requested_ring_line = effective_ring_line(input);
        let ring_delay = 1 + (self.rng % 3);
        let neel_arnab_beat = self.neel_story_active()
            && self.neel_story_beat == stories::neel_university::Beat::ArnabDirectory;
        let neel_arnab_destination = neel_arnab_beat
            && [
                stories::neel_university::BELA_DOG_LINE,
                stories::neel_university::BELA_CAT_LINE,
            ]
            .iter()
            .any(|line| selected == u16::from(*line) || directory_line(selected) == Some(*line));
        let requires_ring = !neel_arnab_beat;
        let call = &mut self.calls[index];
        let call_caller = call.caller;
        let call_callee = call.callee;
        if neel_arnab_beat && call.caller == stories::neel_university::SHADHIN_LINE {
            for target in [
                stories::neel_university::BELA_DOG_LINE,
                stories::neel_university::BELA_CAT_LINE,
            ] {
                if (selected == u16::from(target) || directory_line(selected) == Some(target))
                    && direct(&input.cord_topology, call.caller, target)
                {
                    call.callee = target;
                }
            }
        }
        let operator = has_cord(input, PortId::Subscriber(line), PortId::Operator);
        let ring_requested = requested_ring_line == i16::from(call.callee);
        let ring = active_ring_line == i16::from(call.callee);
        let direct_route = direct(&input.cord_topology, call.caller, call.callee);
        let tap_route =
            input.held_controls.tap && valid_tap_circuit(input, call.caller, call.callee);
        match call.phase {
            CallPhase::Waiting if tap_route && selected == u16::from(call.callee) => {
                connect = true;
            }
            CallPhase::Waiting if operator => {
                call.phase = CallPhase::OperatorSession;
            }
            CallPhase::OperatorSession | CallPhase::AwaitingRouting => {
                if tap_route && selected == u16::from(call.callee) {
                    connect = true;
                } else if (direct_route
                    && (neel_arnab_destination || selected == u16::from(call.callee))
                    && call.phase == CallPhase::AwaitingRouting)
                    && requires_ring
                    || (has_direct_circuit_for_caller(input, line) && requires_ring)
                {
                    error = Some((
                        "premature_direct_routing",
                        "ring the requested destination before connecting the caller directly",
                    ));
                } else if ring_requested {
                    if valid_ringing_circuit(input, call.caller, call.callee) {
                        call.ring_started_at = Some(now);
                        call.ring_ready_at = Some(now + ring_delay);
                        call.phase = CallPhase::Ringing;
                    } else {
                        error = Some((
                            "invalid_cord_topology",
                            "ringing requires exactly the caller-to-Operator and callee-to-Ring Generator cords",
                        ));
                    }
                } else if !requires_ring
                    && direct_route
                    && (neel_arnab_destination || selected == u16::from(call.callee))
                {
                    connect = true;
                } else if input.cord_topology.is_empty() {
                    call.phase = CallPhase::AwaitingRouting;
                }
            }
            CallPhase::Ringing if ring_requested => {}
            CallPhase::Ringing if direct_route && selected == u16::from(call.callee) => {
                if ring_requested {
                    error = Some((
                        "ring_generator_connected",
                        "disconnect the Ring Generator before completing the direct circuit",
                    ));
                } else if !call.ring_activated {
                    error = Some((
                        "ringing_not_ready",
                        "keep the Ring Generator connected until the line LED lights",
                    ));
                } else if call.ring_started_at.is_some() {
                    if valid_direct_circuit(input, call.caller, call.callee) {
                        connect = true;
                    } else {
                        error = Some((
                            "invalid_cord_topology",
                            "direct routing requires exactly one Caller-to-Callee cord",
                        ));
                    }
                } else {
                    error = Some((
                        "premature_direct_routing",
                        "ring the requested destination before connecting the Caller",
                    ));
                }
            }
            CallPhase::Ringing if !ring && !direct_route => {
                if call
                    .ring_started_at
                    .is_none_or(|started| now > started + self.config.ring_grace_seconds)
                {
                    call.phase = CallPhase::AwaitingRouting;
                    call.ring_started_at = None;
                }
            }
            CallPhase::Ringing if direct_route => {
                error = Some((
                    "wrong_destination",
                    "the direct circuit must use the requested destination line",
                ));
            }
            CallPhase::Connected => {
                if valid_connected_circuit(input, call.caller, call.callee) {
                    call.disconnected_at = None;
                    if call.connected_elapsed_seconds.is_some_and(|started| {
                        now.saturating_sub(started) >= call.audio_duration_seconds
                    }) {
                        finish = true;
                    }
                } else {
                    let disconnected_at = *call.disconnected_at.get_or_insert(now);
                    if now.saturating_sub(disconnected_at) >= 5 {
                        finish = true;
                    }
                }
            }
            _ => {}
        }
        if call.phase == CallPhase::OperatorSession && input.cord_topology.is_empty() {
            call.phase = CallPhase::AwaitingRouting;
        }
        if finish {
            self.finish_call(index, false);
        }
        if connect {
            let tap_audio =
                input.held_controls.tap && valid_tap_circuit(input, call_caller, call_callee);
            self.connect_call(index, tap_audio);
        }
        error
    }

    fn connect_call(&mut self, index: usize, tap_audio: bool) {
        let connected_elapsed_seconds = self.elapsed_seconds() as u64;
        let Some((caller, callee)) = self.calls.get(index).map(|call| (call.caller, call.callee))
        else {
            return;
        };
        let neel_story = self.neel_story_active();
        let neel_audio =
            neel_story && stories::neel_university::audio_path(caller, callee).is_some();
        let has_opening_audio = if neel_story {
            tap_audio && neel_audio
        } else {
            self.story_enabled
                && (Self::opening_dialogue(caller).is_some()
                    || stories::neel_university::audio_path(caller, callee).is_some())
        };
        let Some(call) = self.calls.get_mut(index) else {
            return;
        };
        println!(
            "[TRANSITION] call {} -> {} phase={:?} connect_requested has_opening_audio={} story_thread={}",
            caller, callee, call.phase, has_opening_audio, self.story_thread
        );
        if neel_story && !has_opening_audio {
            call.phase = CallPhase::Connected;
            call.connected_at = Some(Instant::now());
            call.connected_elapsed_seconds = Some(connected_elapsed_seconds);
            call.audio_duration_seconds =
                stories::neel_university::audio_duration_seconds(caller, callee).unwrap_or(1);
        } else if self.tts_prepared && !has_opening_audio {
            call.phase = CallPhase::Connected;
            call.connected_at = Some(Instant::now());
            call.connected_elapsed_seconds = Some(connected_elapsed_seconds);
            call.audio_duration_seconds =
                if stories::neel_university::audio_path(caller, callee).is_some() {
                    stories::neel_university::audio_duration_seconds(caller, callee).unwrap_or(1)
                } else {
                    2
                };
        } else {
            eprintln!(
                "[VOICE-DEBUG] queueing opening audio caller={caller} callee={callee} has_opening_audio={has_opening_audio} tts_prepared={}",
                self.tts_prepared
            );
            call.phase = CallPhase::Held;
            call.connected_at = None;
            call.audio_duration_seconds = 0;
            self.pending_tts.push((caller, callee, tap_audio));
        }
    }

    fn install_generated_audio(&mut self, caller: u8, callee: u8, samples: Vec<i16>) {
        let connected_elapsed_seconds = self.elapsed_seconds() as u64;
        let Some(call) = self.calls.iter_mut().find(|call| {
            call.caller == caller && call.callee == callee && call.phase == CallPhase::Held
        }) else {
            return;
        };
        let duration = (samples.len() as u64)
            .div_ceil(u64::from(VOICE_AUDIO_SAMPLE_RATE))
            .max(1);
        eprintln!(
            "[VOICE-DEBUG] opening audio generated caller={caller} callee={callee} samples={} packets={}",
            samples.len(),
            samples.len().div_ceil(VOICE_AUDIO_PACKET_SAMPLES)
        );
        call.audio_duration_seconds = duration;
        call.phase = CallPhase::Connected;
        call.connected_at = Some(Instant::now());
        call.connected_elapsed_seconds = Some(connected_elapsed_seconds);
        self.sync_call_state();
        self.audio_call = Some((caller, callee));
        let sequence = self.audio_sequence;
        let timestamp = self.audio_timestamp;
        let mut packets = Vec::new();
        for (index, chunk) in samples.chunks(VOICE_AUDIO_PACKET_SAMPLES).enumerate() {
            packets.push(
                RtpL16Packet {
                    marker: index == 0,
                    sequence: sequence.wrapping_add(index as u16),
                    timestamp: timestamp.wrapping_add((index * VOICE_AUDIO_PACKET_SAMPLES) as u32),
                    ssrc: 0x4e45_5554,
                    samples: chunk.to_vec(),
                }
                .encode(),
            );
        }
        self.audio_replay = packets.clone();
        self.audio_replay_call = Some((caller, callee));
        for packet in packets {
            if self.audio_queue.len() >= MAX_AUDIO_PACKETS {
                self.audio_queue.pop_front();
            }
            self.audio_queue.push_back(packet);
        }
        self.audio_sequence =
            sequence.wrapping_add(samples.len().div_ceil(VOICE_AUDIO_PACKET_SAMPLES) as u16);
        self.audio_timestamp = timestamp.wrapping_add(samples.len() as u32);
    }

    fn take_pending_tts(&mut self) -> Vec<(u8, u8, bool)> {
        std::mem::take(&mut self.pending_tts)
    }

    fn generate_call_audio(
        config: GameConfig,
        caller: u8,
        callee: u8,
        tap_audio: bool,
    ) -> Result<Vec<i16>, VoiceError> {
        let caller_profile = config
            .subscribers
            .iter()
            .find(|subscriber| subscriber.line == caller)
            .ok_or_else(|| {
                VoiceError::new("subscriber_not_configured", "caller is not configured")
            })?;
        let _callee_profile = config
            .subscribers
            .iter()
            .find(|subscriber| subscriber.line == callee)
            .ok_or_else(|| {
                VoiceError::new("subscriber_not_configured", "callee is not configured")
            })?;
        if tap_audio
            && let Some(path) = stories::neel_university::audio_path(caller, callee)
            && path.is_file()
        {
            let output = Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-i",
                    path.to_string_lossy().as_ref(),
                    "-f",
                    "s16le",
                    "-ar",
                    "24000",
                    "-ac",
                    "1",
                    "-",
                ])
                .output()
                .map_err(|error| VoiceError::new("story_audio_decode_failed", error.to_string()))?;
            if output.status.success() {
                let samples = output
                    .stdout
                    .chunks_exact(2)
                    .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                    .collect::<Vec<_>>();
                if !samples.is_empty() {
                    return Ok(samples);
                }
            }
        }
        let Some(text) = Self::opening_dialogue(caller) else {
            eprintln!("[VOICE-DEBUG] no defined opening dialogue caller={caller} callee={callee}");
            return Ok(Vec::new());
        };
        eprintln!("[VOICE-DEBUG] synthesizing opening dialogue caller={caller} text={text:?}");
        let samples =
            with_persistent_pocket_tts(|tts| tts.synthesize(&caller_profile.voice_id, text))?;
        if samples.is_empty() {
            return Err(VoiceError::new(
                "tts_empty_output",
                "TTS returned no audio samples",
            ));
        }
        Ok(samples)
    }

    fn opening_dialogue(caller: u8) -> Option<&'static str> {
        match caller {
            stories::shapla_apartments::CALLER_LINE => {
                Some(stories::shapla_apartments::OPENING_DIALOGUE)
            }
            _ => None,
        }
    }

    fn finish_call(&mut self, index: usize, missed: bool) {
        let call = self.calls.remove(index);
        println!(
            "[TRANSITION] call {} -> {} phase={:?} -> {}",
            call.caller,
            call.callee,
            call.phase,
            if missed { "Missed" } else { "Completed" }
        );
        self.call_history.push(DebugCallRecord {
            caller_line: call.caller,
            requested_callee_line: call.callee,
            started_elapsed_seconds: call.started_elapsed_seconds,
            patience_deadline_elapsed_seconds: call.deadline,
            final_phase: if missed {
                CallPhase::Missed
            } else {
                CallPhase::Completed
            },
            outcome: if missed { "missed" } else { "completed" }.into(),
            reason: if missed {
                "caller patience expired".into()
            } else {
                "direct circuit completed".into()
            },
            finished_elapsed_seconds: self.elapsed_seconds(),
        });
        if self.audio_call == Some((call.caller, call.callee)) {
            self.audio_queue.clear();
            self.audio_call = None;
            self.audio_replay.clear();
            self.audio_replay_call = None;
        }
        self.resolved = self.resolved.saturating_add(1);
        if missed {
            self.missed += 1;
            self.deductions += 4;
            self.money -= 4;
            append_printer(
                &mut self.state,
                &format!(
                    "MONEY // -$4 missed call line {} -> {} // balance ${}",
                    call.caller, call.callee, self.money
                ),
            );
        } else {
            self.completed += 1;
            let seconds = call.audio_duration_seconds.max(1);
            self.conversation_seconds += seconds;
            self.earned += 5;
            self.money += 5;
            append_printer(
                &mut self.state,
                &format!(
                    "MONEY // +$5 completed connection line {} -> {} // balance ${}",
                    call.caller, call.callee, self.money
                ),
            );
            if self.neel_story_active()
                && let Some(next) = stories::neel_university::next_beat_after_connection(
                    self.neel_story_beat,
                    call.caller,
                    call.callee,
                )
            {
                println!(
                    "[TRANSITION] Neel beat {:?} -> {:?} after call {} -> {}",
                    self.neel_story_beat, next, call.caller, call.callee
                );
                self.neel_story_beat = next;
                match next {
                    stories::neel_university::Beat::ArnabDirectory => {
                        self.story_followup_pending = true;
                    }
                    stories::neel_university::Beat::Completed => {
                        self.story_completed = self.story_beat
                            == stories::shapla_apartments::Beat::HappyFollowup
                            && self.story_followup_call_started;
                        self.earned += 100;
                        self.money += 100;
                        append_printer(
                            &mut self.state,
                            &format!(
                                "MONEY // +$100 Arnab connected to Bela Bose 1032 // balance ${}",
                                self.money
                            ),
                        );
                    }
                    stories::neel_university::Beat::BadEnding => {
                        self.story_completed = false;
                    }
                    stories::neel_university::Beat::ProfessorRouting => {}
                }
            }
        }
        self.state.shift.completed_routings = self.completed;
    }

    fn fail_call(&mut self, index: usize, reason: &str) {
        let call = self.calls.remove(index);
        self.call_history.push(DebugCallRecord {
            caller_line: call.caller,
            requested_callee_line: call.callee,
            started_elapsed_seconds: call.started_elapsed_seconds,
            patience_deadline_elapsed_seconds: call.deadline,
            final_phase: CallPhase::Failed,
            outcome: "failed".into(),
            reason: reason.into(),
            finished_elapsed_seconds: self.elapsed_seconds(),
        });
        self.resolved = self.resolved.saturating_add(1);
        self.failed += 1;
        self.deductions += 4;
        self.money -= 4;
        append_printer(
            &mut self.state,
            &format!(
                "MONEY // -$4 failed connection line {} -> {} ({reason}) // balance ${}",
                call.caller, call.callee, self.money
            ),
        );
    }

    fn fail_generated_call(&mut self, caller: u8, callee: u8, error: &VoiceError) {
        self.state.debug.messages.push(BackendDiagnostic {
            code: error.code.clone(),
            message: format!("opening audio {caller}->{callee}: {}", error.message),
        });
        if let Some(index) = self
            .calls
            .iter()
            .position(|call| call.caller == caller && call.callee == callee)
        {
            self.fail_call(index, "tts_generation_failed");
        }
    }

    fn sync_call_state(&mut self) {
        let focused_caller = self.state.call.as_ref().map(|call| call.caller_line);
        self.state.calls = self
            .calls
            .iter()
            .map(|call| CallStatus {
                caller_line: call.caller,
                requested_callee_line: call.callee,
                phase: call.phase.clone(),
            })
            .collect();
        self.state.call = focused_caller.and_then(|caller| {
            self.state
                .calls
                .iter()
                .find(|call| call.caller_line == caller)
                .cloned()
        });
        self.state.line_lamps = lamps(&self.calls, -1);
        self.sync_story_lamp();
        self.state.shift.active_call_count = self.calls.len() as u8;
    }

    fn connect_ready_direct_calls(&mut self, input: &InputState) {
        let ready = self
            .calls
            .iter()
            .enumerate()
            .filter(|(_, call)| {
                call.phase == CallPhase::Ringing
                    && self.effective_ring_line(input) < 0
                    && call.ring_started_at.is_some()
                    && valid_direct_circuit(input, call.caller, call.callee)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for index in ready {
            self.connect_call(index, false);
        }
    }

    fn expire_calls(&mut self) {
        let now = self.elapsed_seconds() as u64;
        while let Some(index) = self
            .calls
            .iter()
            .position(|c| c.deadline <= now && c.phase == CallPhase::Waiting)
        {
            self.finish_call(index, true);
            if self.state.game_phase == GamePhase::Ended {
                break;
            }
        }
    }

    fn settle_shift(&mut self) {
        let shift_number = self.state.shift.number;
        let finished_elapsed_seconds = self.elapsed_seconds();
        for call in self.calls.drain(..) {
            self.call_history.push(DebugCallRecord {
                caller_line: call.caller,
                requested_callee_line: call.callee,
                started_elapsed_seconds: call.started_elapsed_seconds,
                patience_deadline_elapsed_seconds: call.deadline,
                final_phase: call.phase,
                outcome: "shift_ended".into(),
                reason: "shift duration ended before this Call resolved".into(),
                finished_elapsed_seconds,
            });
        }
        self.audio_queue.clear();
        self.audio_call = None;
        append_printer(
            &mut self.state,
            &format!(
                "SHIFT {shift_number} SUMMARY\nCOMPLETED CALLS // {}\nMISSED CALLS // {}\nFAILED CALLS // {}\nCONVERSATION SECONDS // {}\nEARNED +${}\nDEDUCTIONS -${}\nCURRENT MONEY // ${}",
                self.completed,
                self.missed,
                self.failed,
                self.conversation_seconds,
                self.earned,
                self.deductions,
                self.money
            ),
        );
        if self.state.shift.number >= 3 {
            append_printer(
                &mut self.state,
                &format!(
                    "RETIREMENT // THREE SHIFTS COMPLETE\nFINAL MONEY // ${}",
                    self.money
                ),
            );
            self.state.game_phase = GamePhase::Ended;
        }
        self.state.shift.phase = ShiftPhase::Settled;
        self.state.calls.clear();
        self.state.call = None;
        self.state.line_lamps = [false; 12];
    }

    fn refill_calls(&mut self, _count: usize) {
        self.ensure_story_call();
        let target_count = if self.story_enabled {
            2.min(self.call_target)
        } else {
            _count.min(self.call_target)
        };
        let starting_shift = self.state.shift.phase != ShiftPhase::Active;
        if self.state.shift.phase == ShiftPhase::Settled {
            self.state.shift.number = self.state.shift.number.saturating_add(1);
            self.state.clock.shift = self.state.shift.number;
            self.state.shift.phase = ShiftPhase::Ready;
            self.resolved = 0;
            self.completed = 0;
            self.missed = 0;
            self.failed = 0;
            self.earned = 0;
            self.deductions = 0;
            self.conversation_seconds = 0;
        }
        if starting_shift {
            self.shift_started_elapsed_seconds = self.elapsed_seconds() as u64;
            self.next_call_arrival_elapsed_seconds = self.shift_started_elapsed_seconds;
        }
        let now = self.elapsed_seconds() as u64;
        while self.calls.len() < target_count
            && self.state.shift.number <= 3
            && now >= self.next_call_arrival_elapsed_seconds
        {
            let (caller, callee) = self.next_call();
            let deadline = now + self.random_patience();
            self.calls.push(ActiveCall {
                caller,
                callee,
                phase: CallPhase::Waiting,
                deadline,
                started_elapsed_seconds: now,
                connected_at: None,
                connected_elapsed_seconds: None,
                ring_started_at: None,
                ring_ready_at: None,
                ring_activated: false,
                disconnected_at: None,
                audio_duration_seconds: 0,
            });
            self.next_call_arrival_elapsed_seconds =
                now.saturating_add(self.config.call_arrival_interval_seconds);
        }
        self.state.shift.phase = ShiftPhase::Active;
        self.state.game_phase = GamePhase::Shift;
    }

    fn ensure_story_call(&mut self) {
        if !self.story_enabled {
            return;
        }
        let now = self.elapsed_seconds() as u64;
        if !self.shapla_story_completed
            && self.story_beat != stories::shapla_apartments::Beat::BadFollowup
            && !self
                .calls
                .iter()
                .any(|call| call.caller == stories::shapla_apartments::CALLER_LINE)
        {
            if self.story_beat == stories::shapla_apartments::Beat::HappyFollowup {
                self.story_followup_call_started = true;
            }
            self.calls.push(ActiveCall {
                caller: stories::shapla_apartments::CALLER_LINE,
                callee: 0,
                phase: CallPhase::Waiting,
                deadline: now + u64::MAX / 2,
                started_elapsed_seconds: now,
                connected_at: None,
                connected_elapsed_seconds: None,
                ring_started_at: None,
                ring_ready_at: None,
                ring_activated: false,
                disconnected_at: None,
                audio_duration_seconds: 0,
            });
        }
        if !stories::neel_university::is_terminal(self.neel_story_beat)
            && !self
                .calls
                .iter()
                .any(|call| self.is_neel_caller(call.caller))
        {
            let caller = self.story_caller_for_neel();
            self.calls.push(ActiveCall {
                caller,
                callee: self.story_requested_callee(),
                phase: CallPhase::Waiting,
                deadline: now + self.neel_story_beat.patience_seconds(),
                started_elapsed_seconds: now,
                connected_at: None,
                connected_elapsed_seconds: None,
                ring_started_at: None,
                ring_ready_at: None,
                ring_activated: false,
                disconnected_at: None,
                audio_duration_seconds: 0,
            });
        }
    }

    fn story_caller_for_neel(&self) -> u8 {
        stories::neel_university::caller_for_beat(self.neel_story_beat)
    }

    fn next_call(&mut self) -> (u8, u8) {
        loop {
            self.rng = self
                .rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let caller = (self.rng % u64::from(self.line_limit)) as u8;
            let callee = ((self.rng >> 3) % u64::from(self.line_limit)) as u8;
            if caller != callee
                && (!self.story_enabled
                    || (caller != self.story_caller()
                        && callee != self.story_caller()
                        && !(self.neel_story_active()
                            && [
                                stories::neel_university::NEEL_LINE,
                                stories::neel_university::SHADHIN_LINE,
                                stories::neel_university::BELA_DOG_LINE,
                                stories::neel_university::BELA_CAT_LINE,
                            ]
                            .contains(&caller))
                        && !(self.neel_story_active()
                            && [
                                stories::neel_university::NEEL_LINE,
                                stories::neel_university::SHADHIN_LINE,
                                stories::neel_university::BELA_DOG_LINE,
                                stories::neel_university::BELA_CAT_LINE,
                            ]
                            .contains(&callee))))
                && self.calls.iter().all(|c| {
                    c.caller != caller
                        && c.callee != caller
                        && c.caller != callee
                        && c.callee != callee
                })
            {
                return (caller, callee);
            }
        }
    }
    fn random_patience(&mut self) -> u64 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.config.patience_min_seconds
            + self.rng % (self.config.patience_max_seconds - self.config.patience_min_seconds + 1)
    }
    fn elapsed_seconds(&self) -> u32 {
        (self.clock_started.elapsed().as_secs() + self.debug_elapsed).min(u32::MAX as u64) as u32
    }

    pub fn apply_debug_command(&mut self, request: DebugRequest) -> DebugResponse {
        if request.protocol_version != DEBUG_PROTOCOL_VERSION {
            return DebugResponse {
                protocol_version: DEBUG_PROTOCOL_VERSION,
                accepted: false,
                error: Some(ProtocolError {
                    code: "protocol_version".into(),
                    message: "unsupported debug protocol version".into(),
                }),
                snapshot: self.debug_snapshot(),
                audio: None,
            };
        }
        if let DebugCommand::SelectStoryThread { ref thread_id } = request.command
            && thread_id != "intertwined"
        {
            return DebugResponse {
                protocol_version: DEBUG_PROTOCOL_VERSION,
                accepted: false,
                error: Some(ProtocolError {
                    code: "unknown_story_thread".into(),
                    message: format!("unknown story thread: {thread_id}"),
                }),
                snapshot: self.debug_snapshot(),
                audio: None,
            };
        }
        match request.command {
            DebugCommand::ResetRun => self.reset_run(),
            DebugCommand::SelectStoryThread { thread_id } => {
                println!(
                    "[TRANSITION] story thread {} -> {}",
                    self.story_thread, thread_id
                );
                self.story_thread = thread_id;
                self.reset_run();
            }
            DebugCommand::AdvanceTime { seconds } => {
                self.debug_elapsed = self.debug_elapsed.saturating_add(u64::from(seconds));
                self.expire_calls();
                self.expire_story();
                self.refill_calls(self.call_target);
            }
            DebugCommand::InjectCall {
                caller_line,
                callee_line,
            } if caller_line < self.line_limit
                && callee_line < self.line_limit
                && caller_line != callee_line =>
            {
                let available = self.calls.len() < self.call_target
                    && self.calls.iter().all(|call| {
                        call.caller != caller_line
                            && call.callee != caller_line
                            && call.caller != callee_line
                            && call.callee != callee_line
                    });
                if available {
                    self.calls.push(ActiveCall {
                        caller: caller_line,
                        callee: callee_line,
                        phase: CallPhase::Waiting,
                        deadline: self.elapsed_seconds() as u64 + 30,
                        started_elapsed_seconds: self.elapsed_seconds() as u64,
                        connected_at: None,
                        connected_elapsed_seconds: None,
                        ring_started_at: None,
                        ring_ready_at: None,
                        ring_activated: false,
                        disconnected_at: None,
                        audio_duration_seconds: 0,
                    })
                }
            }
            _ => {}
        }
        DebugResponse {
            protocol_version: DEBUG_PROTOCOL_VERSION,
            accepted: true,
            error: None,
            snapshot: self.debug_snapshot(),
            audio: None,
        }
    }
}

fn with_persistent_pocket_tts<T>(
    operation: impl FnOnce(&mut PersistentPocketTtsCommand) -> Result<T, VoiceError>,
) -> Result<T, VoiceError> {
    let worker = POCKET_TTS_WORKER.get_or_init(|| Mutex::new(None));
    let mut worker = worker
        .lock()
        .map_err(|_| VoiceError::new("tts_worker_lock_failed", "PocketTTS worker lock poisoned"))?;
    if worker.is_none() {
        let command =
            CommandSpec::from_words(&env::var("NN_VOICE_TTS_COMMAND").map_err(|_| {
                VoiceError::new(
                    "voice_worker_not_configured",
                    "NN_VOICE_TTS_COMMAND is not configured",
                )
            })?)?;
        *worker = Some(PersistentPocketTtsCommand::new(command)?);
    }
    let result = operation(worker.as_mut().expect("PocketTTS worker was initialized"));
    if result.is_err() {
        *worker = None;
    }
    result
}

fn with_persistent_dialogue<T>(
    operation: impl FnOnce(&mut PersistentCommandDialogueGenerator) -> Result<T, VoiceError>,
) -> Result<T, VoiceError> {
    let worker = DIALOGUE_WORKER.get_or_init(|| Mutex::new(None));
    let mut worker = worker.lock().map_err(|_| {
        VoiceError::new(
            "dialogue_worker_lock_failed",
            "dialogue worker lock poisoned",
        )
    })?;
    if worker.is_none() {
        let command =
            CommandSpec::from_words(&env::var("NN_VOICE_DIALOGUE_COMMAND").map_err(|_| {
                VoiceError::new(
                    "voice_worker_not_configured",
                    "NN_VOICE_DIALOGUE_COMMAND is not configured",
                )
            })?)?;
        *worker = Some(PersistentCommandDialogueGenerator::new(command)?);
    }
    let result = operation(worker.as_mut().expect("dialogue worker was initialized"));
    if result.is_err() {
        *worker = None;
    }
    result
}
pub fn serve_with_voice_and_debug_engine(
    listener: TcpListener,
    voice_socket: Option<UdpSocket>,
    debug_listener: Option<TcpListener>,
) -> io::Result<()> {
    serve_with_voice_debug_and_text(listener, voice_socket, debug_listener, None)
}

pub fn serve_with_voice_debug_and_text(
    listener: TcpListener,
    voice_socket: Option<UdpSocket>,
    debug_listener: Option<TcpListener>,
    text_listener: Option<TcpListener>,
) -> io::Result<()> {
    let mut initial = Backend::new_exchange();
    if text_listener.is_some() {
        initial.tts_prepared = true;
    }
    let backend = Arc::new(Mutex::new(initial));
    if let Some(socket) = voice_socket {
        let voice_backend = Arc::clone(&backend);
        thread::spawn(move || {
            let _ = serve_voice(socket, voice_backend);
        });
    }
    if let Some(listener) = debug_listener {
        let debug_backend = Arc::clone(&backend);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let connection_backend = Arc::clone(&debug_backend);
                thread::spawn(move || {
                    if let Err(error) = handle_debug_connection(stream, connection_backend) {
                        eprintln!("debug connection closed with error: {error}");
                    }
                });
            }
        });
    }
    if let Some(listener) = text_listener {
        let text_backend = Arc::clone(&backend);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let connection_backend = Arc::clone(&text_backend);
                thread::spawn(move || {
                    if let Err(error) = handle_text_connection(stream, connection_backend) {
                        eprintln!("text connection closed with error: {error}");
                    }
                });
            }
        });
    }
    for stream in listener.incoming() {
        let stream = stream?;
        let connection_backend = Arc::clone(&backend);
        thread::spawn(move || {
            if let Err(error) = handle_connection(stream, connection_backend) {
                eprintln!("frontend connection closed with error: {error}");
            }
        });
    }
    Ok(())
}
pub fn serve(listener: TcpListener) -> io::Result<()> {
    serve_with_voice_and_debug_engine(listener, None, None)
}
pub fn serve_with_voice(listener: TcpListener, voice: Option<UdpSocket>) -> io::Result<()> {
    serve_with_voice_and_debug_engine(listener, voice, None)
}
pub fn serve_voice(socket: UdpSocket, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_millis(10)))?;
    let mut buffer = [0_u8; 65_535];
    let mut input_samples = Vec::new();
    let mut next_chunk = 0_u32;
    loop {
        if let Ok((length, address)) = socket.recv_from(&mut buffer) {
            if let Ok(status) = decode_voice_status(&buffer[..length]) {
                if status.protocol_version == VOICE_PROTOCOL_VERSION {
                    if let Ok(mut state) = backend.lock() {
                        let _ = state.set_voice_peer(address, &status);
                    }
                }
            } else if let Ok(input) = decode_voice_input_audio(&buffer[..length]) {
                if input.protocol_version != VOICE_PROTOCOL_VERSION {
                    continue;
                }
                if !backend
                    .lock()
                    .ok()
                    .is_some_and(|state| state.voice_input_is_current(&input))
                {
                    continue;
                }
                if input.chunk_index == 0 {
                    input_samples.clear();
                    next_chunk = 0;
                }
                if input.chunk_index != next_chunk {
                    continue;
                }
                input_samples.extend_from_slice(&input.samples);
                next_chunk = next_chunk.saturating_add(1);
                if input.complete {
                    let samples = std::mem::take(&mut input_samples);
                    next_chunk = 0;
                    let context = backend.lock().ok().and_then(|state| {
                        state
                            .voice_subscriber_line
                            .zip(state.voice_callee_line)
                            .map(|(caller, callee)| (state.voice_context(caller, callee), caller))
                    });
                    let Some((context, caller)) = context else {
                        continue;
                    };
                    let callee = backend
                        .lock()
                        .ok()
                        .and_then(|state| state.voice_callee_line)
                        .unwrap_or(1);
                    let worker_socket = socket.try_clone()?;
                    let worker_backend = Arc::clone(&backend);
                    let service_turn = backend.lock().ok().is_some_and(|state| {
                        state.voice_turn_controls.police || state.voice_turn_controls.ems
                    });
                    let conversation_id = if let Ok(mut state) = backend.lock() {
                        let id = state.next_voice_conversation_id;
                        state.next_voice_conversation_id =
                            state.next_voice_conversation_id.wrapping_add(1);
                        let (caller_name, _, _) = directory_user(caller);
                        let started_elapsed_seconds = state.elapsed_seconds();
                        state.voice_conversations.push(DebugVoiceConversation {
                            id,
                            session_id: input.session_id,
                            turn_id: input.turn_id,
                            state_revision: input.state_revision,
                            caller_name: caller_name.into(),
                            caller_place: simple_place(caller).into(),
                            status: Some(VoiceStatus::Transcribing),
                            started_elapsed_seconds,
                            finished_elapsed_seconds: None,
                            captured_samples: samples.len() as u32,
                            tts_samples: 0,
                            transcript: None,
                            response_text: None,
                            error: None,
                        });
                        Some(id)
                    } else {
                        None
                    };
                    if let Ok(mut state) = backend.lock() {
                        state.voice_status = Some(VoiceStatus::Transcribing);
                    }
                    thread::spawn(move || {
                        let transcript_backend = Arc::clone(&worker_backend);
                        let result = generate_operator_response(
                            context,
                            caller,
                            callee,
                            samples,
                            service_turn,
                            &|transcript| {
                                if let Ok(mut state) = transcript_backend.lock() {
                                    state.voice_status = if service_turn {
                                        Some(VoiceStatus::Completed)
                                    } else {
                                        Some(VoiceStatus::GeneratingResponse)
                                    };
                                    state.voice_transcript = Some(transcript.to_string());
                                    println!(
                                        "[TRANSITION] voice transcript caller={} service_turn={} controls_police={} controls_ems={} text={:?}",
                                        caller,
                                        service_turn,
                                        state.voice_turn_controls.police,
                                        state.voice_turn_controls.ems,
                                        transcript
                                    );
                                    if service_turn
                                        && caller == stories::shapla_apartments::CALLER_LINE
                                    {
                                        let service = if state.voice_turn_controls.police {
                                            exchange_protocol::ServiceKind::Police
                                        } else {
                                            exchange_protocol::ServiceKind::Ems
                                        };
                                        match classify_story(transcript, service) {
                                            Ok(classification) => {
                                                println!(
                                                    "[TRANSITION] service classification service={:?} result={:?}",
                                                    service, classification
                                                );
                                                state.apply_story_classification(classification)
                                            }
                                            Err(error) => {
                                                println!(
                                                    "[TRANSITION] service classification failed service={:?} code={} message={}",
                                                    service, error.code, error.message
                                                );
                                                append_printer(
                                                    &mut state.state,
                                                    &format!(
                                                        "STORY CLASSIFIER ERROR // {}",
                                                        error.message
                                                    ),
                                                )
                                            }
                                        }
                                    }
                                    if let Some(id) = conversation_id {
                                        if let Some(conversation) = state
                                            .voice_conversations
                                            .iter_mut()
                                            .find(|conversation| conversation.id == id)
                                        {
                                            conversation.status =
                                                Some(VoiceStatus::GeneratingResponse);
                                            conversation.transcript = Some(transcript.to_string());
                                        }
                                    }
                                }
                            },
                        );
                        let (transcript, response, audio) = match result {
                            Ok(value) => value,
                            Err(error) => {
                                if let Ok(mut state) = worker_backend.lock() {
                                    state.voice_status = Some(VoiceStatus::Failed);
                                    state.voice_speaker_active = false;
                                    let finished_elapsed_seconds = state.elapsed_seconds();
                                    if let Some(id) = conversation_id {
                                        if let Some(conversation) = state
                                            .voice_conversations
                                            .iter_mut()
                                            .find(|conversation| conversation.id == id)
                                        {
                                            conversation.status = Some(VoiceStatus::Failed);
                                            conversation.finished_elapsed_seconds =
                                                Some(finished_elapsed_seconds);
                                            conversation.error = Some(ProtocolError {
                                                code: error.code.clone(),
                                                message: error.message.clone(),
                                            });
                                        }
                                    }
                                }
                                let message = VoiceStatusMessage {
                                    protocol_version: VOICE_PROTOCOL_VERSION,
                                    session_id: input.session_id,
                                    turn_id: input.turn_id,
                                    state_revision: input.state_revision,
                                    status: VoiceStatus::Failed,
                                    transcript: None,
                                    response_text: None,
                                    error: Some(ProtocolError {
                                        code: error.code,
                                        message: error.message,
                                    }),
                                };
                                if let Ok(datagram) = encode_voice_status(&message) {
                                    let _ = worker_socket.send_to(&datagram, address);
                                }
                                return;
                            }
                        };
                        if transcript.trim().is_empty() {
                            let completed = VoiceStatusMessage {
                                protocol_version: VOICE_PROTOCOL_VERSION,
                                session_id: input.session_id,
                                turn_id: input.turn_id,
                                state_revision: input.state_revision,
                                status: VoiceStatus::Completed,
                                transcript: None,
                                response_text: None,
                                error: None,
                            };
                            if let Ok(mut state) = worker_backend.lock() {
                                state.voice_status = Some(VoiceStatus::Completed);
                                state.voice_speaker_active = false;
                                let finished_elapsed_seconds = state.elapsed_seconds();
                                if let Some(id) = conversation_id {
                                    if let Some(conversation) = state
                                        .voice_conversations
                                        .iter_mut()
                                        .find(|conversation| conversation.id == id)
                                    {
                                        conversation.status = Some(VoiceStatus::Completed);
                                        conversation.finished_elapsed_seconds =
                                            Some(finished_elapsed_seconds);
                                        conversation.transcript = None;
                                    }
                                }
                            }
                            if let Ok(datagram) = encode_voice_status(&completed) {
                                let _ = worker_socket.send_to(&datagram, address);
                            }
                            return;
                        }
                        if voice_turn_cancelled(&worker_backend, input.turn_id) {
                            return;
                        }
                        let has_response = !response.is_empty();
                        let transcript_for_history = transcript.clone();
                        let response_for_history = response.clone();
                        let status = VoiceStatusMessage {
                            protocol_version: VOICE_PROTOCOL_VERSION,
                            session_id: input.session_id,
                            turn_id: input.turn_id,
                            state_revision: input.state_revision,
                            status: if audio.is_empty() {
                                VoiceStatus::Completed
                            } else {
                                VoiceStatus::Playing
                            },
                            transcript: Some(transcript),
                            response_text: (!response.is_empty()).then_some(response),
                            error: None,
                        };
                        if let Ok(mut state) = worker_backend.lock() {
                            if has_response {
                                if !service_turn {
                                    state.record_story_turn("player", &transcript_for_history);
                                    state.record_story_turn("caller", &response_for_history);
                                }
                                state.complete_story_followup();
                            }
                            state.voice_status = Some(VoiceStatus::Playing);
                            state.voice_speaker_active = true;
                            state.voice_transcript = status.transcript.clone();
                            state.voice_response_text = status.response_text.clone();
                            if let Some(id) = conversation_id {
                                if let Some(conversation) = state
                                    .voice_conversations
                                    .iter_mut()
                                    .find(|conversation| conversation.id == id)
                                {
                                    conversation.status = Some(VoiceStatus::Playing);
                                    conversation.response_text = status.response_text.clone();
                                    conversation.tts_samples = audio.len() as u32;
                                }
                            }
                        }
                        if let Ok(datagram) = encode_voice_status(&status) {
                            let _ = worker_socket.send_to(&datagram, address);
                        }
                        for (index, chunk) in audio.chunks(VOICE_AUDIO_PACKET_SAMPLES).enumerate() {
                            if voice_turn_cancelled(&worker_backend, input.turn_id) {
                                return;
                            }
                            let packet = RtpL16Packet {
                                marker: index == 0,
                                sequence: index as u16,
                                timestamp: (index * VOICE_AUDIO_PACKET_SAMPLES) as u32,
                                ssrc: 0x4e45_5554,
                                samples: chunk.to_vec(),
                            };
                            let _ = worker_socket.send_to(&packet.encode(), address);
                        }
                        if voice_turn_cancelled(&worker_backend, input.turn_id) {
                            return;
                        }
                        let completed = VoiceStatusMessage {
                            protocol_version: VOICE_PROTOCOL_VERSION,
                            session_id: input.session_id,
                            turn_id: input.turn_id,
                            state_revision: input.state_revision,
                            status: VoiceStatus::Completed,
                            transcript: None,
                            response_text: None,
                            error: None,
                        };
                        if let Ok(datagram) = encode_voice_status(&completed) {
                            let _ = worker_socket.send_to(&datagram, address);
                        }
                        if let Ok(mut state) = worker_backend.lock() {
                            state.voice_status = Some(VoiceStatus::Completed);
                            state.voice_speaker_active = false;
                            let finished_elapsed_seconds = state.elapsed_seconds();
                            if let Some(id) = conversation_id {
                                if let Some(conversation) = state
                                    .voice_conversations
                                    .iter_mut()
                                    .find(|conversation| conversation.id == id)
                                {
                                    conversation.status = Some(VoiceStatus::Completed);
                                    conversation.finished_elapsed_seconds =
                                        Some(finished_elapsed_seconds);
                                }
                            }
                        }
                    });
                }
            }
        }
        if let Some((control, address)) = backend
            .lock()
            .ok()
            .and_then(|mut state| state.take_voice_control())
        {
            let datagram = encode_voice_control(&control)
                .map_err(|error| io::Error::other(error.to_string()))?;
            socket.send_to(&datagram, address)?;
        }
        let (packets, audio_peer) = backend
            .lock()
            .ok()
            .map(|mut state| {
                let peer = state.voice_peer;
                if !state.state.tap_bridge_audio_active {
                    state.audio_tap_was_active = false;
                } else if !state.audio_tap_was_active {
                    if state.audio_queue.is_empty()
                        && state.audio_call.is_some()
                        && state.audio_replay_call == state.audio_call
                    {
                        println!(
                            "[TRANSITION] tap replay call={:?} packets={}",
                            state.audio_call,
                            state.audio_replay.len()
                        );
                        let replay = state.audio_replay.clone();
                        state.audio_queue.extend(replay);
                    } else {
                        println!(
                            "[TRANSITION] tap active call={:?} queued_packets={} replay_packets={}",
                            state.audio_call,
                            state.audio_queue.len(),
                            state.audio_replay.len()
                        );
                    }
                    state.audio_tap_was_active = true;
                }
                let packets = if peer.is_some() {
                    state.audio_queue.drain(..).collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                (packets, peer)
            })
            .unwrap_or_default();
        if let Some(address) = audio_peer {
            if !packets.is_empty() {
                eprintln!(
                    "[VOICE-DEBUG] sending {} queued RTP packets to {address}",
                    packets.len()
                );
            }
            for packet in packets {
                socket.send_to(&packet, address)?;
            }
        }
    }
}

fn generate_operator_response(
    context: ResponseContext,
    caller: u8,
    _callee: u8,
    samples: Vec<i16>,
    service_turn: bool,
    transcript_sink: &dyn Fn(&str),
) -> Result<(String, String, Vec<i16>), VoiceError> {
    if samples.is_empty() {
        return Ok((String::new(), String::new(), Vec::new()));
    }
    let stt_command =
        CommandSpec::from_words(&env::var("NN_VOICE_STT_COMMAND").map_err(|_| {
            VoiceError::new(
                "voice_worker_not_configured",
                "NN_VOICE_STT_COMMAND is not configured",
            )
        })?)?;
    let mut stt = CommandSpeechToText::new(stt_command);
    let transcript = stt.transcribe(&samples)?;
    transcript_sink(&transcript);
    if service_turn {
        return Ok((transcript, String::new(), Vec::new()));
    }
    let response = generate_dialogue(&context, &transcript)?;
    let audio = with_persistent_pocket_tts(|tts| {
        tts.synthesize(&format!("pocket-line-{caller}"), &response)
    })?;
    if audio.is_empty() {
        return Err(VoiceError::new(
            "tts_empty_output",
            "TTS returned no audio samples",
        ));
    }
    Ok((transcript, response, audio))
}

fn generate_dialogue(context: &ResponseContext, transcript: &str) -> Result<String, VoiceError> {
    Ok(with_persistent_dialogue(|dialogue| dialogue.generate(context, transcript))?.dialogue)
}

fn classify_story(
    transcript: &str,
    service: exchange_protocol::ServiceKind,
) -> Result<stories::shapla_apartments::Classification, VoiceError> {
    let command = env::var("NN_STORY_CLASSIFIER_COMMAND").map_err(|_| {
        VoiceError::new(
            "story_classifier_not_configured",
            "NN_STORY_CLASSIFIER_COMMAND is not configured",
        )
    })?;
    let mut classifier = CommandTextClassifier::new(CommandSpec::from_words(&command)?);
    let prompt = match service {
        exchange_protocol::ServiceKind::Ems => stories::shapla_apartments::EMS_CLASSIFIER_PROMPT,
        exchange_protocol::ServiceKind::Police => {
            stories::shapla_apartments::POLICE_CLASSIFIER_PROMPT
        }
    };
    let word = classifier.classify(prompt, transcript)?;
    Ok(stories::shapla_apartments::Classification::parse(&word))
}

fn text_error(request: &TextInputMessage, code: &str, message: &str) -> TextResponseMessage {
    TextResponseMessage {
        protocol_version: TEXT_PROTOCOL_VERSION,
        session_id: request.session_id,
        turn_id: request.turn_id,
        state_revision: request.state_revision,
        status: TextStatus::Failed,
        classification: None,
        response_text: None,
        error: Some(ProtocolError {
            code: code.into(),
            message: message.into(),
        }),
    }
}

fn handle_text_connection(mut stream: TcpStream, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    loop {
        let _turn_lock = TEXT_TURN_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| io::Error::other("text turn lock poisoned"))?;
        let request: TextInputMessage = match read_frame(&mut stream) {
            Ok(request) => request,
            Err(FrameError::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(io::Error::new(ErrorKind::InvalidData, error.to_string())),
        };

        let (context, error) = {
            let state = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            if request.protocol_version != TEXT_PROTOCOL_VERSION {
                (
                    None,
                    Some(("protocol_version", "unsupported text protocol version")),
                )
            } else if request.state_revision != state.revision {
                (
                    None,
                    Some(("state_revision", "text state revision mismatch")),
                )
            } else if request.session_id == 0 || request.turn_id == 0 {
                (
                    None,
                    Some(("turn", "session and turn ids must be non-zero")),
                )
            } else if request.text.trim().is_empty() {
                (None, Some(("text", "text input cannot be empty")))
            } else {
                match state.voice_subscriber_line.zip(state.voice_callee_line) {
                    Some((caller, callee)) => (Some(state.voice_context(caller, callee)), None),
                    None => (
                        None,
                        Some(("no_active_call", "connect a caller before sending text")),
                    ),
                }
            }
        };

        let mut classification_word = None;
        if error.is_none()
            && let Ok(mut state) = backend.lock()
        {
            state.story_controls = request.held_controls.clone();
            // The text frontend represents a complete PTT turn. Mirror the
            // controls captured at PTT start so story classification follows
            // the same authoritative path as the voice worker.
            state.voice_turn_controls = request.held_controls.clone();
            if state.voice_subscriber_line == Some(stories::shapla_apartments::CALLER_LINE) {
                let service = if request.held_controls.police {
                    exchange_protocol::ServiceKind::Police
                } else {
                    exchange_protocol::ServiceKind::Ems
                };
                match classify_story(&request.text, service) {
                    Ok(classification) => {
                        classification_word = Some(classification.as_str().to_string());
                        state.apply_story_classification(classification);
                    }
                    Err(error) => append_printer(
                        &mut state.state,
                        &format!("STORY CLASSIFIER ERROR // {}", error.message),
                    ),
                }
            }
        }
        let service_turn = request.held_controls.police || request.held_controls.ems;
        let response = if let Some((code, message)) = error {
            text_error(&request, code, message)
        } else {
            let generated = if service_turn {
                Ok(String::new())
            } else {
                generate_dialogue(&context.expect("validated text context"), &request.text)
            };
            match generated {
                Ok(response_text) => {
                    if let Ok(mut state) = backend.lock() {
                        if !service_turn {
                            state.record_story_turn("player", &request.text);
                            state.record_story_turn("caller", &response_text);
                        }
                        if !response_text.is_empty() {
                            state.complete_story_followup();
                        }
                        state.voice_status = Some(VoiceStatus::Completed);
                        state.voice_transcript = Some(request.text.clone());
                        state.voice_response_text = Some(response_text.clone());
                        let conversation_id = state.next_voice_conversation_id;
                        state.next_voice_conversation_id =
                            state.next_voice_conversation_id.wrapping_add(1);
                        let caller = state.voice_subscriber_line.unwrap_or(0);
                        let now = state.elapsed_seconds();
                        let caller_name = state.subscriber(caller).name.clone();
                        state.voice_conversations.push(DebugVoiceConversation {
                            id: conversation_id,
                            session_id: request.session_id,
                            turn_id: request.turn_id,
                            state_revision: request.state_revision,
                            caller_name,
                            caller_place: simple_place(caller).into(),
                            status: Some(VoiceStatus::Completed),
                            started_elapsed_seconds: now,
                            finished_elapsed_seconds: Some(now),
                            captured_samples: 0,
                            tts_samples: 0,
                            transcript: Some(request.text.clone()),
                            response_text: Some(response_text.clone()),
                            error: None,
                        });
                    }
                    TextResponseMessage {
                        protocol_version: TEXT_PROTOCOL_VERSION,
                        session_id: request.session_id,
                        turn_id: request.turn_id,
                        state_revision: request.state_revision,
                        status: TextStatus::Completed,
                        classification: classification_word,
                        response_text: (!response_text.is_empty()).then_some(response_text),
                        error: None,
                    }
                }
                Err(error) => text_error(&request, &error.code, &error.message),
            }
        };
        write_frame(&mut stream, &response)
            .map_err(|error| io::Error::other(format!("text response write failed: {error}")))?;
    }
}

fn voice_turn_cancelled(backend: &Arc<Mutex<Backend>>, turn_id: u64) -> bool {
    backend
        .lock()
        .ok()
        .is_some_and(|state| state.cancelled_voice_turn == Some(turn_id))
}

pub fn handle_connection(mut stream: TcpStream, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    loop {
        let request: InputMessage = match read_frame(&mut stream) {
            Ok(request) => request,
            Err(FrameError::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(io::Error::new(ErrorKind::InvalidData, error.to_string())),
        };
        let (response, pending_tts, config) = {
            let mut state = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            let response = state.apply_input_message(request);
            (response, state.take_pending_tts(), state.config.clone())
        };
        for (caller, callee, tap_audio) in pending_tts {
            println!(
                "[VOICE-DEBUG] dispatching opening audio caller={caller} callee={callee} tap_audio={tap_audio}"
            );
            let worker_backend = Arc::clone(&backend);
            let call_config = config.clone();
            thread::spawn(move || {
                match Backend::generate_call_audio(call_config, caller, callee, tap_audio) {
                    Ok(samples) => {
                        if let Ok(mut state) = worker_backend.lock() {
                            state.install_generated_audio(caller, callee, samples);
                        }
                    }
                    Err(error) => {
                        if let Ok(mut state) = worker_backend.lock() {
                            state.fail_generated_call(caller, callee, &error);
                        }
                    }
                }
            });
        }
        write_frame(&mut stream, &response)
            .map_err(|e| io::Error::other(format!("state response write failed: {e}")))?;
    }
}
fn handle_debug_connection(mut stream: TcpStream, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    loop {
        let request: DebugRequest = match read_frame(&mut stream) {
            Ok(request) => request,
            Err(FrameError::Io(error))
                if matches!(
                    error.kind(),
                    ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(io::Error::new(ErrorKind::InvalidData, error.to_string())),
        };
        let response = backend
            .lock()
            .map_err(|_| io::Error::other("backend state lock poisoned"))?
            .apply_debug_command(request);
        write_frame(&mut stream, &response)
            .map_err(|e| io::Error::other(format!("debug response write failed: {e}")))?;
    }
}
