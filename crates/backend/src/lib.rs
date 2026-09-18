use std::collections::VecDeque;
use std::env;
use std::io::{self, ErrorKind};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use exchange_protocol::{
    CallPhase, CallStatus, ClockState, CordConnection, DEBUG_PROTOCOL_VERSION, DebugCommand,
    DebugCounters, DebugFrontendState, DebugRequest, DebugResponse, DebugRunState, DebugSnapshot,
    DebugStoryState, DebugSubscriberState, FrameError, GamePhase, HeldControls, InputMessage,
    InputState, OutputDebug, PROTOCOL_VERSION, PortId, PrinterEntry, ProtocolError, RtpL16Packet,
    ShiftPhase, ShiftStatus, StateMessage, StateOutput, TuningState, VOICE_AUDIO_PACKET_SAMPLES,
    VOICE_AUDIO_SAMPLE_RATE, VOICE_PROTOCOL_VERSION, VoiceControl, VoiceStatus, VoiceStatusMessage,
    decode_voice_control, encode_voice_status, read_frame, write_frame,
};
use exchange_voice_daemon::{
    CommandSpec, PersistentPocketTtsCommand, PersistentQwen3TtsCommand, Qwen3TtsCommand,
    TextToSpeech, VoiceError,
};

const LINES: u8 = 6;
const MAX_CALLS: usize = 3;
const MAX_AUDIO_PACKETS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsEngine {
    Qwen,
    Pocket,
}

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
    tts_engine: TtsEngine,
    audio_queue: VecDeque<Vec<u8>>,
    audio_sequence: u16,
    audio_timestamp: u32,
    pending_tts: Vec<(u8, u8)>,
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
            tts_engine: TtsEngine::Qwen,
            audio_queue: VecDeque::new(),
            audio_sequence: 0,
            audio_timestamp: 0,
            pending_tts: Vec::new(),
        };
        backend.quota = backend.random_quota();
        backend.refill_calls(MAX_CALLS);
        backend
    }
    pub fn new_simple_hardware_demo() -> Self {
        let mut backend = Self::new_exchange();
        backend.call_target = 2;
        backend.calls.truncate(2);
        backend.state.calls.truncate(2);
        backend.state.shift.active_call_count = 2;
        backend
    }
    pub fn new_simple_hardware_demo_with_printer_stress(_: bool) -> Self {
        Self::new_simple_hardware_demo()
    }
    pub fn new_hardware_demo() -> Self {
        Self::new_exchange()
    }
    pub fn new_hardware_demo_with_printer_stress(_: bool) -> Self {
        Self::new_exchange()
    }
    pub fn frontend_state(&self) -> &StateOutput {
        &self.state
    }
    pub fn money(&self) -> i32 {
        self.money
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
        self.audio_sequence = 0;
        self.audio_timestamp = 0;
        self.pending_tts.clear();
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
                status: None,
                speaker_active: false,
                session_id: None,
                turn_id: None,
                transcript: None,
                response_text: None,
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
        call.phase = CallPhase::Connected;
        call.connected_at = Some(Instant::now());
        call.audio_duration_seconds = 2;
        self.pending_tts.push((caller, callee));
    }

    fn install_generated_audio(&mut self, caller: u8, callee: u8, samples: Vec<i16>) {
        let Some(call) = self.calls.iter_mut().find(|call| {
            call.caller == caller && call.callee == callee && call.phase == CallPhase::Connected
        }) else {
            return;
        };
        let duration = (samples.len() as u64)
            .div_ceil(u64::from(VOICE_AUDIO_SAMPLE_RATE))
            .max(1);
        call.audio_duration_seconds = duration;
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

    fn generate_call_audio(
        engine: TtsEngine,
        caller: u8,
        callee: u8,
    ) -> Result<Vec<i16>, VoiceError> {
        let text = format!(
            "{}: Please connect me to {}. {}: Of course, I am at {}.",
            directory_user(caller).0,
            simple_place(callee),
            directory_user(callee).0,
            simple_place(callee),
        );
        let mut tts = configured_tts(engine)?;
        let samples = tts.synthesize(&format!("neutral-line-{caller}"), &text)?;
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
            let caller = (self.rng % u64::from(LINES)) as u8;
            let callee = ((self.rng >> 3) % u64::from(LINES)) as u8;
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
            } if caller_line < LINES && callee_line < LINES && caller_line != callee_line => {
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
                format!("USER // {name}"),
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
                "SELECT A LINE FROM 0000 THROUGH 0005".into(),
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
        _ => "MINISTRY DESK",
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
        _ => ("KAVI ORAN", "exchange clerk", "keeps the line register"),
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
}
fn configured_tts(engine: TtsEngine) -> Result<Box<dyn TextToSpeech>, VoiceError> {
    let command = CommandSpec::from_words(&env::var("NN_VOICE_TTS_COMMAND").map_err(|_| {
        VoiceError::new(
            "voice_worker_not_configured",
            "NN_VOICE_TTS_COMMAND is not configured",
        )
    })?)?;
    match engine {
        TtsEngine::Qwen if env::var_os("NN_VOICE_TTS_PERSISTENT").is_some() => {
            Ok(Box::new(PersistentQwen3TtsCommand::new(command)?))
        }
        TtsEngine::Qwen => Ok(Box::new(Qwen3TtsCommand::new(command))),
        TtsEngine::Pocket => Ok(Box::new(PersistentPocketTtsCommand::new(command)?)),
    }
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
    tts: TtsEngine,
) -> io::Result<()> {
    let mut initial = Backend::new_exchange();
    initial.tts_engine = tts;
    let backend = Arc::new(Mutex::new(initial));
    if let Some(socket) = voice_socket {
        let voice_backend = Arc::clone(&backend);
        thread::spawn(move || {
            let _ = serve_voice(socket, voice_backend, tts);
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
    serve_with_voice_and_debug_engine(listener, None, None, TtsEngine::Qwen)
}
pub fn serve_with_voice(listener: TcpListener, voice: Option<UdpSocket>) -> io::Result<()> {
    serve_with_voice_and_debug_engine(listener, voice, None, TtsEngine::Qwen)
}
pub fn serve_voice(
    socket: UdpSocket,
    backend: Arc<Mutex<Backend>>,
    _tts: TtsEngine,
) -> io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_millis(10)))?;
    let mut buffer = [0_u8; 65_535];
    let mut peer = None;
    let mut session = None;
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((length, address)) => {
                if let Ok(control) = decode_voice_control(&buffer[..length]) {
                    if peer.is_some_and(|known| known != address) {
                        continue;
                    }
                    if session.is_some_and(|known| known != (control.session_id, control.turn_id)) {
                        continue;
                    }
                    peer = Some(address);
                    session = Some((control.session_id, control.turn_id));
                    let status = VoiceStatusMessage {
                        protocol_version: VOICE_PROTOCOL_VERSION,
                        session_id: control.session_id,
                        turn_id: control.turn_id,
                        state_revision: control.state_revision,
                        status: match control.control {
                            VoiceControl::Cancel => VoiceStatus::Cancelled,
                            VoiceControl::StartPtt | VoiceControl::ReleasePtt => VoiceStatus::Ready,
                        },
                        transcript: None,
                        response_text: None,
                        error: None,
                    };
                    let datagram = encode_voice_status(&status)
                        .map_err(|error| io::Error::other(error.to_string()))?;
                    socket.send_to(&datagram, address)?;
                }
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => return Err(error),
        }
        let packets = {
            let mut state = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            if state.state.tap_bridge_audio_active && peer.is_some() {
                state.audio_queue.drain(..).collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        if let Some(address) = peer {
            for packet in packets {
                socket.send_to(&packet, address)?;
            }
        }
    }
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
        let (response, pending_tts, tts_engine) = {
            let mut state = backend
                .lock()
                .map_err(|_| io::Error::other("backend state lock poisoned"))?;
            let response = state.apply_input_message(request);
            (response, state.take_pending_tts(), state.tts_engine)
        };
        for (caller, callee) in pending_tts {
            let worker_backend = Arc::clone(&backend);
            thread::spawn(move || {
                if let Ok(samples) = Backend::generate_call_audio(tts_engine, caller, callee) {
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
