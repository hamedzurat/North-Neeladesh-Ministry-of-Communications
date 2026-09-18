use std::collections::VecDeque;
use std::env;
use std::io::{self, ErrorKind};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use exchange_protocol::{
    CallPhase, CallStatus, ClockState, CordConnection, DEBUG_PROTOCOL_VERSION, DebugCommand,
    DebugCounters, DebugFrontendState, DebugRequest, DebugResponse, DebugRunState, DebugSnapshot,
    DebugStoryState, DebugSubscriberState, FrameError, GamePhase, HeldControls, InputMessage,
    InputState, OutputDebug, PROTOCOL_VERSION, PortId, PrinterEntry, ProtocolError, RtpL16Packet,
    ShiftPhase, ShiftStatus, StateMessage, StateOutput, TuningState, VOICE_AUDIO_PACKET_SAMPLES,
    VOICE_AUDIO_SAMPLE_RATE, VOICE_PROTOCOL_VERSION, VoiceControl, VoiceControlMessage,
    VoiceStatus, VoiceStatusMessage, decode_voice_input_audio, decode_voice_status,
    encode_voice_control, encode_voice_status, read_frame, write_frame,
};
use exchange_voice_daemon::{
    CommandDialogueGenerator, CommandSpec, CommandSpeechToText, DialogueGenerator,
    PersistentPocketTtsCommand, ResponseContext, SpeechToText, SubscriberProfile, TextToSpeech,
    VoiceError,
};

const LINES: u8 = 12;
const MAX_CALLS: usize = 3;
const MAX_AUDIO_PACKETS: usize = 4096;

#[derive(Debug, Clone)]
struct ActiveCall {
    caller: u8,
    callee: u8,
    phase: CallPhase,
    deadline: u64,
    connected_at: Option<Instant>,
    ring_started_at: Option<u64>,
    last_crank_timestamp: u64,
    crank_samples: u8,
    audio_duration_seconds: u64,
}

pub struct Backend {
    state: StateOutput,
    revision: u64,
    sequence: Option<u64>,
    clock_started: Instant,
    debug_elapsed: u64,
    rng: u64,
    calls: Vec<ActiveCall>,
    line_limit: u8,
    call_target: usize,
    quota: u8,
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
    audio_sequence: u16,
    audio_timestamp: u32,
    pending_tts: Vec<(u8, u8)>,
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
}

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend {
    pub fn new() -> Self {
        Self::new_exchange()
    }
    pub fn new_exchange() -> Self {
        let mut backend = Self {
            state: initial_state(),
            revision: 0,
            sequence: None,
            clock_started: Instant::now(),
            debug_elapsed: 0,
            rng: 0x4e45_454c_4144_4553,
            calls: Vec::new(),
            line_limit: LINES,
            call_target: MAX_CALLS,
            quota: 4,
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
            audio_sequence: 0,
            audio_timestamp: 0,
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
        };
        backend.quota = backend.random_quota();
        backend.refill_calls(MAX_CALLS);
        backend
    }
    pub fn new_simple_hardware_demo() -> Self {
        let mut backend = Self::new_exchange();
        backend.call_target = 2;
        backend.line_limit = 6;
        backend.tts_prepared = true;
        backend.calls.clear();
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
        backend.state.line_lamps = lamps(&backend.calls);
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
        let (name, role, preference) = directory_user(caller);
        ResponseContext {
            profile: SubscriberProfile {
                subscriber_id: caller,
                name: name.into(),
                voice_id: format!("neutral-line-{caller}"),
                personality: role.into(),
                baseline_goals: vec![format!("Reach {}", simple_place(callee))],
                initial_perspective: format!("Calling from {}", simple_place(caller)),
                permitted_actions: vec!["request_routing".into()],
            },
            caller_place: simple_place(caller).into(),
            requested_place: simple_place(callee).into(),
            known_places: (0..LINES).map(simple_place).map(str::to_string).collect(),
            subscriber_goal: format!("Reach {}", simple_place(callee)),
            call_premise: format!("Request a connection to {}.", simple_place(callee)),
            call_guidance: format!(
                "Answer the Operator's question naturally. State the requested place when asked. You enjoy {}.",
                preference
            ),
            permitted_knowledge: vec![],
            recent_conversation: vec![],
            current_input: None,
        }
    }

    pub fn reset_run(&mut self) {
        self.run_generation = self.run_generation.wrapping_add(1);
        self.state = initial_state();
        self.revision = 0;
        self.sequence = None;
        self.clock_started = Instant::now();
        self.debug_elapsed = 0;
        self.calls.clear();
        self.resolved = 0;
        self.earned = 0;
        self.deductions = 0;
        self.money = 0;
        self.completed = 0;
        self.missed = 0;
        self.failed = 0;
        self.conversation_seconds = 0;
        self.quota = self.random_quota();
        self.audio_queue.clear();
        self.audio_call = None;
        self.audio_sequence = 0;
        self.audio_timestamp = 0;
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
        self.state.line_lamps = lamps(&self.calls);
        self.state.shift.active_call_count = self.calls.len() as u8;
        self.state.game_phase = GamePhase::Shift;
    }

    pub fn debug_snapshot(&self) -> DebugSnapshot {
        let calls = self.state.calls.clone();
        let subscribers = (0..LINES)
            .map(|line| {
                let active = self.state.line_lamps[line as usize];
                let (name, role, _) = directory_user(line);
                DebugSubscriberState {
                    id: format!("line_{line}"),
                    name: name.into(),
                    line: Some(line),
                    status: if active { "off_hook" } else { "on_hook" }.into(),
                    availability: if active { "off_hook" } else { "on_hook" }.into(),
                    pressure: u32::from(active),
                    current_goal: None,
                    status_flags: vec![role.into()],
                }
            })
            .collect();
        DebugSnapshot {
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
            subscribers,
            story: DebugStoryState {
                current_node_id: "neutral_exchange".into(),
                frontier: vec![],
                current_story_beat: None,
                interference_reduced: false,
                interference_level: 0,
                operator_knowledge: vec![],
                graph: vec![],
            },
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
                conversations: vec![],
            },
            frontend: DebugFrontendState {
                firmware_version: None,
                transport_connected: false,
                device_faults: vec![],
                last_input_json: None,
                last_output_json: None,
            },
            recent_errors: self.state.debug.messages.clone(),
        }
    }

    pub fn apply_input_message(&mut self, message: InputMessage) -> StateMessage {
        if self.last_request.as_ref() == Some(&message)
            && let Some(response) = self.last_response.clone()
        {
            return response;
        }
        let result = self.apply_input(message.clone());
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
        self.debug_elapsed = self.debug_elapsed.saturating_add(0);
        if self.state.shift.phase == ShiftPhase::Settled
            && self.state.shift.number < 3
            && self.state.game_phase != GamePhase::Ended
        {
            self.refill_calls(self.call_target);
        }
        self.expire_calls();
        let selected = directory_id(input.directory_digits);
        let focused = operator_line(&input.cord_topology)
            .or_else(|| self.state.call.as_ref().map(|call| call.caller_line));
        let mut error = None;
        if let Some(call) = self.calls.iter().find(|call| Some(call.caller) == focused) {
            if (direct(&input.cord_topology, call.caller, call.callee)
                || has_cord(
                    input,
                    PortId::Subscriber(call.callee),
                    PortId::RingGenerator,
                ))
                && selected != u16::from(call.callee)
            {
                error = Some((
                    "directory_selection_required",
                    "select the requested destination before routing",
                ));
            }
            if error.is_none() && has_wrong_direct_circuit(input, call.caller, call.callee) {
                error = Some((
                    "wrong_destination",
                    "the direct circuit must use the requested destination line",
                ));
            }
            if error.is_none()
                && call.phase == CallPhase::Connected
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
            self.fail_call(index);
        }
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
        self.state.line_lamps = lamps(&self.calls);
        self.state.shift.active_call_count = self.calls.len() as u8;
        self.state.tap_bridge_monitoring = tap_monitor(input, &self.state);
        self.state.tap_bridge_audio_active = self.state.tap_bridge_monitoring.is_some();
        self.update_voice_control(input, self.revision);
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
        let ptt = input.held_controls.ptt;
        if ptt == self.last_ptt {
            return;
        }
        self.last_ptt = ptt;
        if ptt && self.state.tap_bridge_audio_active {
            return;
        }
        let caller = operator_line(&input.cord_topology)
            .or_else(|| self.state.call.as_ref().map(|call| call.caller_line));
        if ptt {
            self.voice_subscriber_line = caller;
            self.voice_callee_line = caller.and_then(|line| {
                self.calls
                    .iter()
                    .find(|call| call.caller == line)
                    .map(|call| call.callee)
            });
            self.voice_turn_id = self.next_voice_turn_id;
            self.next_voice_turn_id = self.next_voice_turn_id.wrapping_add(1);
        }
        let voice_id = caller
            .map(|line| format!("pocket-line-{line}"))
            .unwrap_or_else(|| "pocket-line-0".into());
        self.pending_voice_control = Some(VoiceControlMessage {
            protocol_version: VOICE_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: self.voice_turn_id.max(1),
            state_revision: revision,
            voice_id,
            control: if ptt {
                VoiceControl::StartPtt
            } else {
                VoiceControl::ReleasePtt
            },
        });
        self.voice_state_revision = revision;
    }

    fn advance(
        &mut self,
        input: &InputState,
        focused: Option<u8>,
        selected: u16,
    ) -> Option<(&'static str, &'static str)> {
        let line = focused?;
        let index = self.calls.iter().position(|c| c.caller == line)?;
        let mut finish = false;
        let mut connect = false;
        let mut error = None;
        let now = self.elapsed_seconds() as u64;
        let call = &mut self.calls[index];
        let operator = has_cord(input, PortId::Subscriber(line), PortId::Operator);
        let ring = has_cord(
            input,
            PortId::Subscriber(call.callee),
            PortId::RingGenerator,
        );
        let direct_route = direct(&input.cord_topology, call.caller, call.callee);
        match call.phase {
            CallPhase::Waiting if operator && input.held_controls.ptt => {
                call.phase = CallPhase::OperatorSession
            }
            CallPhase::Waiting if operator => {
                error = Some((
                    "ptt_required",
                    "hold PTT while the caller is connected to the Operator",
                ));
            }
            CallPhase::OperatorSession | CallPhase::AwaitingRouting => {
                if (direct_route
                    && selected == u16::from(call.callee)
                    && call.phase == CallPhase::AwaitingRouting)
                    || has_any_direct_circuit(input)
                {
                    error = Some((
                        "premature_direct_routing",
                        "ring the requested destination before connecting the caller directly",
                    ));
                } else if ring && crank(input) {
                    if exact_cords(
                        &input.cord_topology,
                        &[
                            (PortId::Subscriber(call.caller), PortId::Operator),
                            (PortId::Subscriber(call.callee), PortId::RingGenerator),
                        ],
                    ) {
                        call.ring_started_at = Some(now);
                        call.last_crank_timestamp = input.crank_rotation_timestamps[3];
                        call.crank_samples = 1;
                        call.phase = CallPhase::Ringing;
                    } else {
                        error = Some((
                            "invalid_cord_topology",
                            "ringing requires exactly the caller-to-Operator and callee-to-Ring Generator cords",
                        ));
                    }
                } else if input.cord_topology.is_empty() {
                    call.phase = CallPhase::AwaitingRouting;
                }
            }
            CallPhase::Ringing if ring && crank(input) => {
                let timestamp = input.crank_rotation_timestamps[3];
                if timestamp > call.last_crank_timestamp {
                    call.last_crank_timestamp = timestamp;
                    call.crank_samples = call.crank_samples.saturating_add(1);
                }
            }
            CallPhase::Ringing if direct_route && selected == u16::from(call.callee) => {
                if ring {
                    error = Some((
                        "ring_generator_connected",
                        "disconnect the Ring Generator before completing the direct circuit",
                    ));
                } else if call
                    .ring_started_at
                    .is_some_and(|started| now >= started + 2)
                    && call.crank_samples >= 2
                {
                    if exact_cords(
                        &input.cord_topology,
                        &[(
                            PortId::Subscriber(call.caller),
                            PortId::Subscriber(call.callee),
                        )],
                    ) {
                        connect = true;
                    } else {
                        error = Some((
                            "invalid_cord_topology",
                            "direct routing requires exactly one Caller-to-Callee cord",
                        ));
                    }
                }
            }
            CallPhase::Ringing if !ring && !direct_route => {
                call.phase = CallPhase::AwaitingRouting;
                call.ring_started_at = None;
                call.last_crank_timestamp = 0;
                call.crank_samples = 0;
            }
            CallPhase::Ringing if direct_route => {
                error = Some((
                    "wrong_destination",
                    "the direct circuit must use the requested destination line",
                ));
            }
            CallPhase::Connected
                if input.cord_topology.is_empty()
                    || (direct_route
                        && call.connected_at.is_some_and(|started| {
                            started.elapsed() >= Duration::from_secs(call.audio_duration_seconds)
                        })) =>
            {
                finish = true
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
            self.connect_call(index);
        }
        error
    }

    fn connect_call(&mut self, index: usize) {
        let Some((caller, callee)) = self.calls.get(index).map(|call| (call.caller, call.callee))
        else {
            return;
        };
        let Some(call) = self.calls.get_mut(index) else {
            return;
        };
        if self.tts_prepared {
            call.phase = CallPhase::Connected;
            call.connected_at = Some(Instant::now());
            call.audio_duration_seconds = 2;
        } else {
            call.phase = CallPhase::Held;
            call.connected_at = None;
            call.audio_duration_seconds = 0;
            self.pending_tts.push((caller, callee));
        }
    }

    fn install_generated_audio(&mut self, caller: u8, callee: u8, samples: Vec<i16>) {
        let Some(call) = self.calls.iter_mut().find(|call| {
            call.caller == caller && call.callee == callee && call.phase == CallPhase::Held
        }) else {
            return;
        };
        let duration = (samples.len() as u64)
            .div_ceil(u64::from(VOICE_AUDIO_SAMPLE_RATE))
            .max(1);
        call.audio_duration_seconds = duration;
        call.phase = CallPhase::Connected;
        call.connected_at = Some(Instant::now());
        self.audio_call = Some((caller, callee));
        let sequence = self.audio_sequence;
        let timestamp = self.audio_timestamp;
        for (index, chunk) in samples.chunks(VOICE_AUDIO_PACKET_SAMPLES).enumerate() {
            if self.audio_queue.len() >= MAX_AUDIO_PACKETS {
                self.audio_queue.pop_front();
            }
            self.audio_queue.push_back(
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
        self.audio_sequence =
            sequence.wrapping_add(samples.len().div_ceil(VOICE_AUDIO_PACKET_SAMPLES) as u16);
        self.audio_timestamp = timestamp.wrapping_add(samples.len() as u32);
    }

    fn take_pending_tts(&mut self) -> Vec<(u8, u8)> {
        std::mem::take(&mut self.pending_tts)
    }

    fn generate_call_audio(caller: u8, callee: u8) -> Result<Vec<i16>, VoiceError> {
        let text = format!(
            "{}: Please connect me to {}. {}: Of course, I am at {}. {}: We can talk about {} while we wait.",
            directory_user(caller).0,
            simple_place(callee),
            directory_user(callee).0,
            simple_place(callee),
            directory_user(caller).0,
            directory_user(caller).2,
        );
        let mut tts = configured_tts()?;
        let samples = tts.synthesize(&format!("pocket-line-{caller}"), &text)?;
        if samples.is_empty() {
            return Err(VoiceError::new(
                "tts_empty_output",
                "TTS returned no audio samples",
            ));
        }
        Ok(samples)
    }

    fn finish_call(&mut self, index: usize, missed: bool) {
        let call = self.calls.remove(index);
        if self.audio_call == Some((call.caller, call.callee)) {
            self.audio_queue.clear();
            self.audio_call = None;
        }
        self.resolved = self.resolved.saturating_add(1);
        if missed {
            self.missed += 1;
            self.deductions += 2;
            self.money -= 2;
        } else {
            self.completed += 1;
            let seconds = call.audio_duration_seconds.max(1);
            self.conversation_seconds += seconds;
            self.earned += seconds as i32;
            self.money += seconds as i32;
        }
        self.state.shift.completed_routings = self.completed;
        if self.resolved >= self.quota {
            self.settle_shift();
        } else {
            self.refill_calls(self.call_target);
        }
    }

    fn fail_call(&mut self, index: usize) {
        self.calls.remove(index);
        self.resolved = self.resolved.saturating_add(1);
        self.failed += 1;
        self.deductions += 2;
        self.money -= 2;
        if self.resolved >= self.quota {
            self.settle_shift();
        } else {
            self.refill_calls(self.call_target);
        }
    }

    fn fail_generated_call(&mut self, caller: u8, callee: u8) {
        if let Some(index) = self
            .calls
            .iter()
            .position(|call| call.caller == caller && call.callee == callee)
        {
            self.fail_call(index);
        }
    }

    fn expire_calls(&mut self) {
        let now = self.elapsed_seconds() as u64;
        while let Some(index) = self
            .calls
            .iter()
            .position(|c| c.deadline <= now && c.phase != CallPhase::Connected)
        {
            self.finish_call(index, true);
            if self.state.game_phase == GamePhase::Ended {
                break;
            }
        }
    }

    fn settle_shift(&mut self) {
        let shift_number = self.state.shift.number;
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
        self.calls.clear();
        self.state.calls.clear();
        self.state.call = None;
        self.state.line_lamps = [false; 12];
    }

    fn refill_calls(&mut self, count: usize) {
        if self.state.shift.phase == ShiftPhase::Settled {
            self.state.shift.number = self.state.shift.number.saturating_add(1);
            self.state.clock.shift = self.state.shift.number;
            self.state.shift.phase = ShiftPhase::Ready;
            self.state.shift.completed_routings = 0;
            self.resolved = 0;
            self.completed = 0;
            self.missed = 0;
            self.failed = 0;
            self.earned = 0;
            self.deductions = 0;
            self.conversation_seconds = 0;
            self.quota = self.random_quota();
        }
        while self.calls.len() < count.min(self.call_target) && self.state.shift.number <= 3 {
            let (caller, callee) = self.next_call();
            let deadline = self.elapsed_seconds() as u64 + self.random_patience();
            self.calls.push(ActiveCall {
                caller,
                callee,
                phase: CallPhase::Waiting,
                deadline,
                connected_at: None,
                ring_started_at: None,
                last_crank_timestamp: 0,
                crank_samples: 0,
                audio_duration_seconds: 2,
            });
        }
        self.state.shift.phase = ShiftPhase::Active;
        self.state.game_phase = GamePhase::Shift;
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
        20 + self.rng % 41
    }
    fn random_quota(&mut self) -> u8 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        4 + (self.rng % 3) as u8
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
        match request.command {
            DebugCommand::ResetRun => self.reset_run(),
            DebugCommand::AdvanceTime { seconds } => {
                self.debug_elapsed = self.debug_elapsed.saturating_add(u64::from(seconds))
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
                        connected_at: None,
                        ring_started_at: None,
                        last_crank_timestamp: 0,
                        crank_samples: 0,
                        audio_duration_seconds: 2,
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

fn initial_state() -> StateOutput {
    StateOutput {
        line_lamps: [false; 12],
        game_phase: GamePhase::Ready,
        run_generation: 0,
        clock: ClockState {
            shift: 1,
            elapsed_seconds: 0,
        },
        speaker_active: false,
        interference_level: 0,
        tap_bridge_audio_active: false,
        tuning: TuningState::default(),
        directory_pages: directory_pages([0, 0, 0, 0]),
        printer_output: vec![PrinterEntry {
            entry_id: 1,
            text: "SHIFT 1 START // TELEPHONE EXCHANGE READY".into(),
        }],
        call: None,
        calls: vec![],
        service_call: None,
        tap_bridge_monitoring: None,
        shift: ShiftStatus {
            number: 1,
            phase: ShiftPhase::Ready,
            active_call_count: 0,
            completed_routings: 0,
            required_service_calls: 0,
            completed_service_calls: 0,
            service_errors: 0,
            service_error_counts: vec![],
        },
        debug: OutputDebug { messages: vec![] },
    }
}
fn append_printer(state: &mut StateOutput, text: &str) {
    let id = state.printer_output.last().map_or(1, |e| e.entry_id + 1);
    state.printer_output.push(PrinterEntry {
        entry_id: id,
        text: text.into(),
    });
}
fn rejected(
    sequence: u64,
    code: &str,
    message: &str,
    revision: u64,
    output: &StateOutput,
) -> StateMessage {
    StateMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        accepted: false,
        error: Some(ProtocolError {
            code: code.into(),
            message: message.into(),
        }),
        state_revision: revision,
        output: output.clone(),
    }
}
fn directory_id(d: [u8; 4]) -> u16 {
    d.into_iter().fold(0, |n, x| n * 10 + u16::from(x))
}
fn directory_pages(digits: [u8; 4]) -> Vec<exchange_protocol::DirectoryPage> {
    let id = directory_id(digits);
    if id < u16::from(LINES) {
        let (name, role, note) = directory_user(id as u8);
        vec![exchange_protocol::DirectoryPage {
            page_number: 1,
            heading: simple_place(id as u8).into(),
            lines: vec![
                format!("SUBSCRIBER ID {id:04}"),
                format!("SUBSCRIBER // {name}"),
                format!("ROLE // {role}"),
                format!("NOTE // {note}"),
                format!("DESTINATION // {}", simple_place(id as u8)),
            ],
        }]
    } else {
        vec![exchange_protocol::DirectoryPage {
            page_number: 1,
            heading: "NO RECORD".into(),
            lines: vec![
                format!("SUBSCRIBER ID {id:04}"),
                "SELECT A LINE FROM 0000 THROUGH 0011".into(),
            ],
        }]
    }
}
fn simple_place(line: u8) -> &'static str {
    match line {
        0 => "RAIL DISPATCH",
        1 => "KHARAD CLINIC",
        2 => "RATION OFFICE",
        3 => "BORDER DEPOT",
        4 => "FOUNDRY APTS",
        5 => "MINISTRY DESK",
        6 => "HOTEL MERIDIAN",
        7 => "MINING OFFICE",
        8 => "RATAN COLONY",
        9 => "SHAPLA APARTMENTS",
        10 => "OLD MARKET",
        _ => "CENTRAL STATION",
    }
}
fn directory_user(line: u8) -> (&'static str, &'static str, &'static str) {
    match line {
        0 => ("MARA KESH", "rail clerk", "handles relief-train manifests"),
        1 => (
            "DR. LEYA VARAN",
            "clinic registrar",
            "keeps the night ward ledger",
        ),
        2 => (
            "OMAR SEN",
            "ration clerk",
            "issues household allotment cards",
        ),
        3 => (
            "CAPTAIN OREN VEY",
            "depot officer",
            "on duty until the dawn bell",
        ),
        4 => (
            "NERI TAL",
            "foundry tenant",
            "repairs small motors after shift",
        ),
        5 => ("KAVI ORAN", "exchange clerk", "keeps the line register"),
        6 => ("TOMAS VALE", "hotel clerk", "keeps a camera by the desk"),
        7 => ("JAVED RAHMAN", "mining clerk", "checks freight manifests"),
        8 => (
            "PARO SEN",
            "colony organizer",
            "knows the night shift workers",
        ),
        9 => ("RAFI ALAM", "resident", "feeds a one-eyed cat"),
        10 => ("BIKRAM SEN", "market courier", "likes spiced tea"),
        _ => ("MIRA HALEK", "station worker", "collects old timetables"),
    }
}
fn operator_line(cords: &[CordConnection]) -> Option<u8> {
    cords.iter().find_map(|c| match (&c.first, &c.second) {
        (PortId::Subscriber(n), PortId::Operator) | (PortId::Operator, PortId::Subscriber(n)) => {
            Some(*n)
        }
        _ => None,
    })
}
fn has_cord(input: &InputState, a: PortId, b: PortId) -> bool {
    input
        .cord_topology
        .iter()
        .any(|c| (c.first == a && c.second == b) || (c.first == b && c.second == a))
}
fn direct(cords: &[CordConnection], caller: u8, callee: u8) -> bool {
    has_cord(
        &InputState {
            cord_topology: cords.to_vec(),
            held_controls: HeldControls::default(),
            directory_digits: [0; 4],
            crank_rotation_timestamps: [0; 4],
            tuning: TuningState::default(),
            debug: Default::default(),
        },
        PortId::Subscriber(caller),
        PortId::Subscriber(callee),
    )
}
fn has_any_direct_circuit(input: &InputState) -> bool {
    input.cord_topology.iter().any(|cord| {
        matches!(cord.first, PortId::Subscriber(_)) && matches!(cord.second, PortId::Subscriber(_))
    })
}
fn exact_cords(input: &[CordConnection], expected: &[(PortId, PortId)]) -> bool {
    input.len() == expected.len()
        && expected.iter().all(|(first, second)| {
            input.iter().any(|cord| {
                (&cord.first == first && &cord.second == second)
                    || (&cord.first == second && &cord.second == first)
            })
        })
}
fn has_wrong_direct_circuit(input: &InputState, caller: u8, callee: u8) -> bool {
    input.cord_topology.iter().any(|cord| {
        let (PortId::Subscriber(first), PortId::Subscriber(second)) = (&cord.first, &cord.second)
        else {
            return false;
        };
        !((*first == caller && *second == callee) || (*first == callee && *second == caller))
    })
}
fn crank(input: &InputState) -> bool {
    input.crank_rotation_timestamps[3] > 0
        && input
            .crank_rotation_timestamps
            .windows(2)
            .all(|timestamps| timestamps[1] > timestamps[0])
}
fn configured_tts() -> Result<Box<dyn TextToSpeech>, VoiceError> {
    let command = CommandSpec::from_words(&env::var("NN_VOICE_TTS_COMMAND").map_err(|_| {
        VoiceError::new(
            "voice_worker_not_configured",
            "NN_VOICE_TTS_COMMAND is not configured",
        )
    })?)?;
    Ok(Box::new(PersistentPocketTtsCommand::new(command)?))
}
fn lamps(calls: &[ActiveCall]) -> [bool; 12] {
    let mut result = [false; 12];
    for c in calls {
        if !matches!(
            c.phase,
            CallPhase::Missed | CallPhase::Failed | CallPhase::Completed
        ) {
            result[c.caller as usize] = true;
            if matches!(c.phase, CallPhase::Ringing | CallPhase::Connected) {
                result[c.callee as usize] = true;
            }
        }
    }
    result
}
fn tap_monitor(input: &InputState, state: &StateOutput) -> Option<u8> {
    if !input.held_controls.tap {
        return None;
    }
    state.calls.iter().find_map(|call| {
        if call.phase != CallPhase::Connected {
            return None;
        }
        exact_cords(
            &input.cord_topology,
            &[
                (
                    PortId::Subscriber(call.caller_line),
                    PortId::Subscriber(call.requested_callee_line),
                ),
                (PortId::Subscriber(call.caller_line), PortId::Tap(1)),
                (
                    PortId::Subscriber(call.requested_callee_line),
                    PortId::Tap(2),
                ),
            ],
        )
        .then_some(1)
    })
}
fn valid_connected_circuit(input: &InputState, caller: u8, callee: u8) -> bool {
    exact_cords(
        &input.cord_topology,
        &[(PortId::Subscriber(caller), PortId::Subscriber(callee))],
    ) || exact_cords(
        &input.cord_topology,
        &[
            (PortId::Subscriber(caller), PortId::Subscriber(callee)),
            (PortId::Subscriber(caller), PortId::Tap(1)),
            (PortId::Subscriber(callee), PortId::Tap(2)),
        ],
    )
}

pub fn serve_with_voice_and_debug_engine(
    listener: TcpListener,
    voice_socket: Option<UdpSocket>,
    debug_listener: Option<TcpListener>,
) -> io::Result<()> {
    let initial = Backend::new_exchange();
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
                    thread::spawn(move || {
                        let result = generate_operator_response(context, caller, callee, samples);
                        let (transcript, response, audio) = match result {
                            Ok(value) => value,
                            Err(error) => {
                                if let Ok(mut state) = worker_backend.lock() {
                                    state.voice_status = Some(VoiceStatus::Failed);
                                    state.voice_speaker_active = false;
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
                        let status = VoiceStatusMessage {
                            protocol_version: VOICE_PROTOCOL_VERSION,
                            session_id: input.session_id,
                            turn_id: input.turn_id,
                            state_revision: input.state_revision,
                            status: VoiceStatus::Playing,
                            transcript: Some(transcript),
                            response_text: Some(response),
                            error: None,
                        };
                        if let Ok(mut state) = worker_backend.lock() {
                            state.voice_status = Some(VoiceStatus::Playing);
                            state.voice_speaker_active = true;
                            state.voice_transcript = status.transcript.clone();
                            state.voice_response_text = status.response_text.clone();
                        }
                        if let Ok(datagram) = encode_voice_status(&status) {
                            let _ = worker_socket.send_to(&datagram, address);
                        }
                        for (index, chunk) in audio.chunks(VOICE_AUDIO_PACKET_SAMPLES).enumerate() {
                            let packet = RtpL16Packet {
                                marker: index == 0,
                                sequence: index as u16,
                                timestamp: (index * VOICE_AUDIO_PACKET_SAMPLES) as u32,
                                ssrc: 0x4e45_5554,
                                samples: chunk.to_vec(),
                            };
                            let _ = worker_socket.send_to(&packet.encode(), address);
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
                let packets = if state.state.tap_bridge_audio_active {
                    state.audio_queue.drain(..).collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                (packets, peer)
            })
            .unwrap_or_default();
        if let Some(address) = audio_peer {
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
) -> Result<(String, String, Vec<i16>), VoiceError> {
    let stt_command =
        CommandSpec::from_words(&env::var("NN_VOICE_STT_COMMAND").map_err(|_| {
            VoiceError::new(
                "voice_worker_not_configured",
                "NN_VOICE_STT_COMMAND is not configured",
            )
        })?)?;
    let dialogue_command =
        CommandSpec::from_words(&env::var("NN_VOICE_DIALOGUE_COMMAND").map_err(|_| {
            VoiceError::new(
                "voice_worker_not_configured",
                "NN_VOICE_DIALOGUE_COMMAND is not configured",
            )
        })?)?;
    let mut stt = CommandSpeechToText::new(stt_command);
    let transcript = stt.transcribe(&samples)?;
    let mut dialogue = CommandDialogueGenerator::new(dialogue_command);
    let response = dialogue.generate(&context, &transcript)?.dialogue;
    let tts_command =
        CommandSpec::from_words(&env::var("NN_VOICE_TTS_COMMAND").map_err(|_| {
            VoiceError::new(
                "voice_worker_not_configured",
                "NN_VOICE_TTS_COMMAND is not configured",
            )
        })?)?;
    let mut tts = PersistentPocketTtsCommand::new(tts_command)?;
    let audio = tts.synthesize(&format!("pocket-line-{caller}"), &response)?;
    if audio.is_empty() {
        return Err(VoiceError::new(
            "tts_empty_output",
            "TTS returned no audio samples",
        ));
    }
    Ok((transcript, response, audio))
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
        let (response, pending_tts) = {
            let mut state = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            let response = state.apply_input_message(request);
            (response, state.take_pending_tts())
        };
        for (caller, callee) in pending_tts {
            let worker_backend = Arc::clone(&backend);
            thread::spawn(move || {
                if let Ok(samples) = Backend::generate_call_audio(caller, callee) {
                    if let Ok(mut state) = worker_backend.lock() {
                        state.install_generated_audio(caller, callee, samples);
                    }
                } else if let Ok(mut state) = worker_backend.lock() {
                    state.fail_generated_call(caller, callee);
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
