use std::env;
use std::io::{self, ErrorKind};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use exchange_protocol::{
    BackendDiagnostic, ClockState, CordConnection, DirectoryPage, GamePhase, InputMessage,
    InputState, OutputDebug, PROTOCOL_VERSION, PortId, PrinterEntry, ProtocolError, ShiftPhase,
    ShiftStatus, StateMessage, StateOutput, TuningState, VoiceControl, VoiceControlMessage,
    VoiceStatus, decode_voice_status, encode_voice_control, read_frame, write_frame,
};

const MAX_FAULTS: usize = 16;
const MAX_CORDS: usize = 8;
const STRESS_PRINTER_ENTRY_COUNT: usize = 48;

pub struct Backend {
    state: StateOutput,
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
    pending_voice_control: Option<VoiceControlMessage>,
}

impl Backend {
    pub fn new() -> Self {
        Self::new_with_printer_stress(false)
    }

    pub fn new_with_printer_stress(printer_stress: bool) -> Self {
        Self {
            state: initial_state(printer_stress),
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
            pending_voice_control: None,
        }
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
        let crank_rotation_timestamps = input.crank_rotation_timestamps;
        let mut next_state = self.state.clone();
        let transition = advance_call(&self.state, input, self.last_crank_rotation_timestamps);
        next_state.call = transition.call;
        next_state.line_lamps = transition.line_lamps;
        next_state.game_phase = transition.game_phase;
        next_state.shift = transition.shift;
        let routing_receipt = transition.routing_receipt;

        if let Some(text) = routing_receipt {
            let entry_id = next_state
                .printer_output
                .last()
                .map_or(1, |entry| entry.entry_id + 1);
            next_state
                .printer_output
                .push(PrinterEntry { entry_id, text });
        }

        self.last_crank_rotation_timestamps = crank_rotation_timestamps;
        next_state.clock.elapsed_seconds =
            self.clock_started.elapsed().as_secs().min(u32::MAX as u64) as u32;
        self.state_revision += 1;
        self.state = StateOutput {
            line_lamps: next_state.line_lamps,
            game_phase: next_state.game_phase,
            clock: next_state.clock,
            speaker_active: speaker_is_active(input) || self.voice_speaker_active,
            tuning: input.tuning.clone(),
            directory_pages: directory_pages(input.directory_digits),
            printer_output: next_state.printer_output,
            call: next_state.call,
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
        if ptt != self.last_ptt {
            self.last_ptt = ptt;
            self.pending_voice_control = Some(VoiceControlMessage {
                protocol_version: exchange_protocol::VOICE_PROTOCOL_VERSION,
                session_id: 1,
                turn_id: 1,
                state_revision: self.state_revision,
                control: if ptt {
                    VoiceControl::StartPtt
                } else {
                    VoiceControl::ReleasePtt
                },
            });
        }
        response
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

impl Default for Backend {
    fn default() -> Self {
        Self::new()
    }
}

struct CallTransition {
    call: Option<exchange_protocol::CallStatus>,
    line_lamps: [bool; 16],
    game_phase: GamePhase,
    shift: ShiftStatus,
    routing_receipt: Option<String>,
}

fn advance_call(
    state: &StateOutput,
    input: &InputState,
    previous_crank_rotation_timestamps: [u64; 4],
) -> CallTransition {
    if state.call.is_none() {
        let Some((caller_line, callee)) = authored_call(input.directory_digits) else {
            if input.cord_topology.is_empty() {
                return CallTransition {
                    line_lamps: [false; 16],
                    call: None,
                    game_phase: state.game_phase.clone(),
                    shift: state.shift.clone(),
                    routing_receipt: None,
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
        return CallTransition {
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
        };
    }

    let call = state.call.as_ref().expect("call checked above");
    let caller = PortId::Subscriber(call.caller_line);
    let callee = PortId::Subscriber(call.requested_callee_line);
    let caller_operator = has_exact_cords(&input.cord_topology, &[(&caller, &PortId::Operator)]);
    let ring_generator = has_ring_generator(&input.cord_topology, &caller, &callee);
    let direct_circuit = has_exact_cords(&input.cord_topology, &[(&caller, &callee)]);

    let mut next_call = call.clone();
    let mut next_game_phase = state.game_phase.clone();
    let mut next_shift = state.shift.clone();
    let mut routing_receipt = None;

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
            if ring_generator {
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
            if ring_generator {
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
                routing_receipt = Some(format!(
                    "ROUTING {} -> {}",
                    call.caller_line, call.requested_callee_line
                ));
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
                return CallTransition {
                    call: None,
                    line_lamps: [false; 16],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                };
            } else {
                return unchanged_transition(state);
            }
        }
        exchange_protocol::CallPhase::Completed => {
            if input.cord_topology.is_empty() {
                return CallTransition {
                    call: None,
                    line_lamps: [false; 16],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                };
            }
            if !direct_circuit {
                return unchanged_transition(state);
            }
        }
        _ => {}
    }

    next_shift.active_call_count =
        u8::from(next_call.phase != exchange_protocol::CallPhase::Completed);
    CallTransition {
        line_lamps: lamps_for_call(Some(&next_call)),
        call: Some(next_call),
        game_phase: next_game_phase,
        shift: next_shift,
        routing_receipt,
    }
}

fn unchanged_transition(state: &StateOutput) -> CallTransition {
    CallTransition {
        call: state.call.clone(),
        line_lamps: state.line_lamps,
        game_phase: state.game_phase.clone(),
        shift: state.shift.clone(),
        routing_receipt: None,
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

fn speaker_is_active(input: &InputState) -> bool {
    let held = &input.held_controls;
    let operator_active = held.ptt
        && input
            .cord_topology
            .iter()
            .any(|cord| cord.first == PortId::Operator || cord.second == PortId::Operator);
    let tap_active = (held.tap_1 && has_port(&input.cord_topology, &PortId::Tap(1)))
        || (held.tap_2 && has_port(&input.cord_topology, &PortId::Tap(2)));
    operator_active || held.police || held.ems || held.fire || tap_active
}

fn has_port(cords: &[CordConnection], port: &PortId) -> bool {
    cords
        .iter()
        .any(|cord| &cord.first == port || &cord.second == port)
}

fn authored_call(digits: [u8; 4]) -> Option<(u8, u8)> {
    let id = digits
        .iter()
        .fold(0_u16, |value, digit| value * 10 + *digit as u16);
    (1..=5).contains(&id).then_some((0, id as u8))
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
        shift: ShiftStatus {
            number: 1,
            phase: ShiftPhase::Ready,
            active_call_count: 0,
            completed_routings: 0,
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
        return vec![PrinterEntry {
            entry_id: 1,
            text: "PROVINCIAL EXCHANGE READY".to_string(),
        }];
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
