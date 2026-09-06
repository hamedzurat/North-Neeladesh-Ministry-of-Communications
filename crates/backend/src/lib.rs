use std::env;
use std::io::{self, ErrorKind};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use exchange_protocol::{
    BackendDiagnostic, ClockState, CordConnection, DirectoryPage, GamePhase, InputMessage,
    InputState, OutputDebug, PROTOCOL_VERSION, PortId, PrinterEntry, ProtocolError,
    ServiceCallPhase, ServiceCallStatus, ServiceErrorCount, ServiceErrorKind, ServiceKind,
    ShiftPhase, ShiftStatus, StateMessage, StateOutput, TuningState, VoiceControl,
    VoiceControlMessage, VoiceStatus, decode_voice_status, encode_voice_control, read_frame,
    write_frame,
};

pub mod story;

use story::{
    AuthoredContent, CompiledStoryGraph, GraphCompileError, StoryEligibilityState, StoryNodeKind,
    StoryPathSelection,
};

const MAX_FAULTS: usize = 16;
const MAX_CORDS: usize = 8;
const STRESS_PRINTER_ENTRY_COUNT: usize = 48;
const VOICE_IDS: [&str; 9] = [
    "Vivian", "Serena", "Uncle_Fu", "Dylan", "Eric", "Ryan", "Aiden", "Ono_Anna", "Sohee",
];

pub struct Backend {
    state: StateOutput,
    story: CompiledStoryGraph,
    story_node_id: String,
    state_revision: u64,
    last_request: Option<InputMessage>,
    last_response: Option<StateMessage>,
    clock_started: Instant,
    last_crank_rotation_timestamps: [u64; 4],
    voice_speaker_active: bool,
    last_ptt: bool,
    voice_peer: Option<SocketAddr>,
    voice_session_id: Option<u64>,
    voice_turn_id: Option<u64>,
    voice_state_revision: Option<u64>,
    voice_request_voice_id: Option<String>,
    pending_voice_control: Option<VoiceControlMessage>,
    last_ems: bool,
    service_error_recorded: bool,
}

impl Backend {
    pub fn new() -> Self {
        Self::new_with_printer_stress(false)
    }

    pub fn new_with_printer_stress(printer_stress: bool) -> Self {
        let story = AuthoredContent::demo()
            .compile()
            .expect("built-in authored Story Graph must compile");
        Self::with_story(story, printer_stress)
    }

    pub fn new_with_story(content: AuthoredContent) -> Result<Self, GraphCompileError> {
        Self::new_with_story_and_printer_stress(content, false)
    }

    pub fn new_with_story_and_printer_stress(
        content: AuthoredContent,
        printer_stress: bool,
    ) -> Result<Self, GraphCompileError> {
        Ok(Self::with_story(content.compile()?, printer_stress))
    }

    fn with_story(story: CompiledStoryGraph, printer_stress: bool) -> Self {
        let story_node_id = story.start_node_id().to_string();
        Self {
            state: initial_state(printer_stress),
            story,
            story_node_id,
            state_revision: 0,
            last_request: None,
            last_response: None,
            clock_started: Instant::now(),
            last_crank_rotation_timestamps: [0; 4],
            voice_speaker_active: false,
            last_ptt: false,
            voice_peer: None,
            voice_session_id: None,
            voice_turn_id: None,
            voice_state_revision: None,
            voice_request_voice_id: None,
            pending_voice_control: None,
            last_ems: false,
            service_error_recorded: false,
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
        }
        selection
    }

    pub fn reset_run(&mut self) {
        let printer_stress = self.state.printer_output.len() == STRESS_PRINTER_ENTRY_COUNT;
        self.state = initial_state(printer_stress);
        self.story_node_id = self.story.start_node_id().to_string();
        self.state_revision = 0;
        self.last_request = None;
        self.last_response = None;
        self.clock_started = Instant::now();
        self.last_crank_rotation_timestamps = [0; 4];
        self.voice_speaker_active = false;
        self.last_ptt = false;
        self.voice_peer = None;
        self.voice_session_id = None;
        self.voice_turn_id = None;
        self.voice_state_revision = None;
        self.voice_request_voice_id = None;
        self.pending_voice_control = None;
        self.last_ems = false;
        self.service_error_recorded = false;
    }

    pub fn apply_input_message(&mut self, message: InputMessage) -> StateMessage {
        trace_input(&message);
        let response = self.apply_input_message_inner(message);
        trace_state(&response);
        response
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

        let input = &message.input;
        let ptt = input.held_controls.ptt;
        let ems = input.held_controls.ems;
        let crank_rotation_timestamps = input.crank_rotation_timestamps;
        let mut next_state = self.state.clone();
        let authored_call = if self.state.call.is_none() {
            self.prepare_story_call(input.directory_digits, &input.cord_topology)
        } else {
            self.story_call_for_node(&self.story_node_id, input.directory_digits)
        };
        let authored_competing_call = operator_caller_line(&input.cord_topology)
            .and_then(|line| self.story.authored_call_for_caller_line(line));
        let transition = advance_calls(
            &self.state,
            input,
            self.last_crank_rotation_timestamps,
            authored_call,
            authored_competing_call,
        );
        if let Some(outcome) = transition.story_outcome {
            self.advance_story_outcome(outcome);
        }
        next_state.call = transition.call;
        next_state.calls = transition.calls;
        next_state.line_lamps = transition.line_lamps;
        next_state.game_phase = transition.game_phase;
        next_state.shift = transition.shift;
        if transition.story_event_complete {
            next_state.shift.phase = ShiftPhase::Settled;
            next_state.game_phase = GamePhase::Ended;
        }
        apply_service_transition(
            self,
            &mut next_state,
            input,
            transition.story_event_complete,
        );
        next_state.tap_bridge_monitoring = tap_bridge_monitoring(input, &next_state);
        let routing_receipt = transition.routing_receipt;

        if let Some(text) = routing_receipt {
            append_printer(&mut next_state, &text);
        }

        if transition.story_event_complete {
            self.settle_story_event(&next_state);
        }

        self.last_crank_rotation_timestamps = crank_rotation_timestamps;
        next_state.clock.elapsed_seconds =
            self.clock_started.elapsed().as_secs().min(u32::MAX as u64) as u32;
        self.state_revision += 1;
        let speaker_active = speaker_is_active(input, &next_state) || self.voice_speaker_active;
        self.state = StateOutput {
            line_lamps: next_state.line_lamps,
            game_phase: next_state.game_phase,
            clock: next_state.clock,
            speaker_active,
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
        self.last_ems = ems;
        if ptt != self.last_ptt {
            self.last_ptt = ptt;
            let voice_id = if ptt {
                let caller_line = self.state.call.as_ref().map_or(0, |call| call.caller_line);
                let voice_id = select_voice_id(caller_line);
                self.voice_request_voice_id = Some(voice_id.clone());
                voice_id
            } else {
                self.voice_request_voice_id
                    .take()
                    .unwrap_or_else(|| "Ryan".to_string())
            };
            self.pending_voice_control = Some(VoiceControlMessage {
                protocol_version: exchange_protocol::VOICE_PROTOCOL_VERSION,
                session_id: 1,
                turn_id: 1,
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

    pub fn apply_voice_datagram(&mut self, datagram: &[u8]) -> bool {
        self.apply_voice_datagram_from(datagram, None)
    }

    pub fn apply_voice_datagram_from(&mut self, datagram: &[u8], peer: Option<SocketAddr>) -> bool {
        if let Ok(message) = decode_voice_status(datagram) {
            if message.protocol_version != exchange_protocol::VOICE_PROTOCOL_VERSION {
                return false;
            }
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
            if message.status == VoiceStatus::Ready {
                if let Some(peer) = peer {
                    self.voice_peer = Some(peer);
                }
                self.voice_session_id = Some(message.session_id);
                self.voice_turn_id = Some(message.turn_id);
            } else if self.voice_session_id.is_none() {
                return false;
            }
            self.voice_state_revision = Some(
                self.voice_state_revision
                    .unwrap_or(message.state_revision)
                    .max(message.state_revision),
            );
            self.voice_speaker_active = matches!(message.status, VoiceStatus::Playing);
            if message.status == VoiceStatus::Failed {
                self.state.debug.messages.push(BackendDiagnostic {
                    code: "voice_failed".to_string(),
                    message: message.error.map_or_else(
                        || "voice daemon failed without details".to_string(),
                        |error| format!("{}: {}", error.code, error.message),
                    ),
                });
                if self.state.debug.messages.len() > MAX_FAULTS {
                    self.state.debug.messages.remove(0);
                }
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
}

fn select_voice_id(caller_line: u8) -> String {
    if env::var("NN_VOICE_RANDOM_SPEAKER").is_ok_and(|value| value == "0") {
        return "Ryan".to_string();
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.subsec_nanos() as usize);
    VOICE_IDS[(nanos + caller_line as usize) % VOICE_IDS.len()].to_string()
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

#[derive(Debug, Clone, Copy)]
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

    if state.service_call.as_ref().is_some_and(|service| {
        service.service == ServiceKind::Ems && service.phase == ServiceCallPhase::Active
    }) && input.held_controls.ems
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

fn apply_service_transition(
    backend: &mut Backend,
    state: &mut StateOutput,
    input: &InputState,
    settling: bool,
) {
    if input.held_controls.ems && !backend.last_ems && state.shift.phase == ShiftPhase::Active {
        if state.shift.completed_service_calls == 0 {
            state.service_call = Some(ServiceCallStatus {
                service: ServiceKind::Ems,
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
    } else if !input.held_controls.ems
        && backend.last_ems
        && state.service_call.as_ref().is_some_and(|call| {
            call.service == ServiceKind::Ems && call.phase == ServiceCallPhase::Active
        })
    {
        state.service_call = Some(ServiceCallStatus {
            service: ServiceKind::Ems,
            phase: ServiceCallPhase::Completed,
        });
        state.shift.completed_service_calls += 1;
        append_printer(state, "SERVICE EMS COMPLETED");
    }

    if settling
        && !backend.service_error_recorded
        && state.shift.completed_service_calls < state.shift.required_service_calls
    {
        state.shift.service_errors += 1;
        if let Some(error) = state
            .shift
            .service_error_counts
            .iter_mut()
            .find(|error| error.kind == ServiceErrorKind::MissedRequiredServiceCall)
        {
            error.count += 1;
        } else {
            state.shift.service_error_counts.push(ServiceErrorCount {
                kind: ServiceErrorKind::MissedRequiredServiceCall,
                count: 1,
            });
        }
        backend.service_error_recorded = true;
        append_printer(state, "SERVICE ERROR EMS REQUIRED");
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
    serve_with_voice(listener, None)
}

pub fn serve_with_voice(listener: TcpListener, voice_socket: Option<UdpSocket>) -> io::Result<()> {
    let backend = Arc::new(Mutex::new(Backend::new_with_printer_stress(
        printer_stress_enabled(),
    )));
    let voice_socket = voice_socket.map(Arc::new);
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
        thread::spawn(move || {
            if let Err(error) = handle_connection_with_voice(stream, backend, voice_socket) {
                eprintln!("frontend connection ended: {error}");
            }
        });
    }
    Ok(())
}

pub fn serve_voice(socket: UdpSocket, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
    let mut datagram = [0_u8; 65_535];
    loop {
        let (length, peer) = socket.recv_from(&mut datagram)?;
        let mut backend = backend
            .lock()
            .map_err(|_| io::Error::other("backend state lock poisoned"))?;
        backend.apply_voice_datagram_from(&datagram[..length], Some(peer));
    }
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

fn backend_trace_enabled() -> bool {
    matches!(env::var("NN_BACKEND_TRACE").as_deref(), Ok("1"))
}

fn trace_input(message: &InputMessage) {
    if backend_trace_enabled() {
        eprintln!("[backend <- frontend] {message:?}");
    }
}

fn trace_state(message: &StateMessage) {
    if backend_trace_enabled() {
        eprintln!("[backend -> frontend] {message:?}");
    }
}

fn initial_state(printer_stress: bool) -> StateOutput {
    StateOutput {
        line_lamps: [false; 16],
        game_phase: GamePhase::Ready,
        clock: ClockState {
            shift: 1,
            elapsed_seconds: 0,
        },
        speaker_active: false,
        tuning: TuningState::default(),
        directory_pages: directory_pages([0, 0, 0, 1]),
        printer_output: initial_printer_output(printer_stress),
        call: None,
        calls: Vec::new(),
        service_call: None,
        tap_bridge_monitoring: None,
        shift: ShiftStatus {
            number: 1,
            phase: ShiftPhase::Ready,
            active_call_count: 0,
            completed_routings: 0,
            required_service_calls: 1,
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

fn initial_printer_output(printer_stress: bool) -> Vec<PrinterEntry> {
    if !printer_stress {
        return vec![
            PrinterEntry {
                entry_id: 1,
                text: "PROVINCIAL EXCHANGE READY".to_string(),
            },
            PrinterEntry {
                entry_id: 2,
                text: "SERVICE RULE // EMS REQUIRED THIS SHIFT".to_string(),
            },
        ];
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
        1 => Some(("TAREN KESH", "Railway dispatcher")),
        2 => Some(("VIRA DHAL", "Factory records clerk")),
        3 => Some(("DR. LEYA VARAN", "Emergency physician")),
        4 => Some(("CAPTAIN OREN VEY", "State Protection Directorate")),
        5 => Some(("NERI TAL", "Railway signal operator")),
        _ => None,
    };

    match record {
        Some((heading, role)) => vec![
            DirectoryPage {
                page_number: 1,
                heading: heading.to_string(),
                lines: vec![format!("SUBSCRIBER ID {id:04}"), role.to_string()],
            },
            DirectoryPage {
                page_number: 2,
                heading: "PROVINCIAL EXCHANGE".to_string(),
                lines: vec!["ACTIVE LISTING".to_string()],
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
