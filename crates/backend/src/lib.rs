use std::io::{self, ErrorKind};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use exchange_protocol::{
    BackendDiagnostic, ClockState, CordConnection, CrankState, DiagnosticState, DirectoryPage,
    FrontendDiagnostics, FrontendIdentity, GamePhase, HeldControls, InputMessage, MessageKind,
    PROTOCOL_VERSION, PortId, PrinterEntry, ProtocolError, ShiftPhase, ShiftStatus, StateMessage,
    StateSnapshot, TuningState, read_frame, write_frame,
};

const MAX_SESSION_ID_LENGTH: usize = 64;
const MAX_INSTANCE_ID_LENGTH: usize = 64;
const MAX_FAULTS: usize = 16;
const MAX_CORDS: usize = 13;

pub struct Backend {
    state: StateSnapshot,
    controller: Option<Controller>,
    last_request: Option<InputMessage>,
    last_response: Option<StateMessage>,
    last_message_id: Option<u64>,
}

#[derive(Clone, PartialEq, Eq)]
struct Controller {
    session_id: String,
    frontend: FrontendIdentity,
}

impl Backend {
    pub fn new() -> Self {
        Self {
            state: initial_state(),
            controller: None,
            last_request: None,
            last_response: None,
            last_message_id: None,
        }
    }

    pub fn apply_input_snapshot(&mut self, message: InputMessage) -> StateMessage {
        if self.last_request.as_ref() == Some(&message) {
            if let Some(response) = &self.last_response {
                return response.clone();
            }
        }

        if let Err(error) = validate_message(self, &message) {
            return rejected_response(message.message_id, error, &self.state);
        }

        let input = &message.input;
        let reset = input.reset;
        let mut next_state = if reset {
            initial_state()
        } else {
            self.state.clone()
        };
        let mut routing_receipt = None;

        if !reset {
            let transition = match advance_call(&self.state, input) {
                Ok(transition) => transition,
                Err(error) => return rejected_response(message.message_id, error, &self.state),
            };
            next_state.call = transition.call;
            next_state.line_lamps = transition.line_lamps;
            next_state.game_phase = transition.game_phase;
            next_state.shift = transition.shift;
            routing_receipt = transition.routing_receipt;
        }

        if let Some(text) = routing_receipt {
            let entry_id = next_state
                .printer_output
                .last()
                .map_or(1, |entry| entry.entry_id + 1);
            next_state
                .printer_output
                .push(PrinterEntry { entry_id, text });
        }

        let revision = self.state.state_revision + 1;
        self.state = StateSnapshot {
            protocol_version: PROTOCOL_VERSION,
            frontend: input.frontend.clone(),
            session_id: message.session_id.clone(),
            input_sequence: input.input_sequence,
            state_revision: revision,
            cord_topology: input.cord_topology.clone(),
            held_controls: input.held_controls.clone(),
            directory_digits: input.directory_digits,
            crank: input.crank.clone(),
            tuning: input.tuning.clone(),
            reset_applied: reset,
            line_lamps: next_state.line_lamps,
            game_phase: next_state.game_phase,
            clock: next_state.clock,
            directory_pages: directory_pages(input.directory_digits),
            printer_output: next_state.printer_output,
            call: next_state.call,
            shift: next_state.shift,
            diagnostics: DiagnosticState {
                frontend: input.diagnostics.clone(),
                messages: next_state.diagnostics.messages,
            },
        };
        self.controller = Some(Controller {
            session_id: message.session_id.clone(),
            frontend: input.frontend.clone(),
        });

        let response = StateMessage {
            protocol_version: PROTOCOL_VERSION,
            message_kind: MessageKind::StateSnapshot,
            message_id: message.message_id,
            accepted: true,
            error: None,
            state: self.state.clone(),
        };
        self.last_request = Some(message);
        self.last_response = Some(response.clone());
        self.last_message_id = Some(response.message_id);
        response
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
    state: &StateSnapshot,
    input: &exchange_protocol::InputSnapshot,
) -> Result<CallTransition, ProtocolError> {
    if state.call.is_none() {
        let Some((caller_line, callee)) = authored_call(input.directory_digits) else {
            if input.cord_topology.is_empty() {
                return Ok(CallTransition {
                    line_lamps: [false; 16],
                    call: None,
                    game_phase: state.game_phase.clone(),
                    shift: state.shift.clone(),
                    routing_receipt: None,
                });
            }
            return Err(protocol_error(
                "routing_before_ringing",
                "a new Call must be connected to the Operator Jack before routing",
            ));
        };
        let caller = PortId::Subscriber(caller_line);
        let operator_cord = has_exact_cords(&input.cord_topology, &[(&caller, &PortId::Operator)]);
        if !input.cord_topology.is_empty() && !operator_cord {
            return Err(protocol_error(
                "routing_before_ringing",
                "a new Call must be connected to the Operator Jack before routing",
            ));
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
        return Ok(CallTransition {
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
        });
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
            } else if direct_circuit {
                return Err(protocol_error(
                    "routing_before_ringing",
                    "the caller and callee cannot be connected before ringing",
                ));
            } else if !input.cord_topology.is_empty() {
                return Err(protocol_error(
                    "invalid_cord_arrangement",
                    "the waiting caller must be connected to the Operator Jack",
                ));
            }
        }
        exchange_protocol::CallPhase::OperatorSession => {
            if ring_generator {
                if input.crank.rotation_count == 0 || input.crank.speed == 0 {
                    return Err(protocol_error(
                        "ringing_requires_crank",
                        "ringing requires a Ring Generator connection and crank input",
                    ));
                }
                next_call.phase = exchange_protocol::CallPhase::Ringing;
            } else if direct_circuit {
                return Err(protocol_error(
                    "routing_before_ringing",
                    "the caller and callee cannot be connected before ringing",
                ));
            } else if input.cord_topology.is_empty() {
                next_call.phase = exchange_protocol::CallPhase::AwaitingRouting;
            } else if !caller_operator {
                return Err(protocol_error(
                    "invalid_cord_arrangement",
                    "the Operator Session requires the caller to be connected to the Operator Jack",
                ));
            }
        }
        exchange_protocol::CallPhase::AwaitingRouting => {
            if ring_generator {
                if input.crank.rotation_count == 0 || input.crank.speed == 0 {
                    return Err(protocol_error(
                        "ringing_requires_crank",
                        "ringing requires a Ring Generator connection and crank input",
                    ));
                }
                next_call.phase = exchange_protocol::CallPhase::Ringing;
            } else if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
            } else if direct_circuit {
                return Err(protocol_error(
                    "routing_before_ringing",
                    "the caller and callee cannot be connected before ringing",
                ));
            } else if !input.cord_topology.is_empty() {
                return Err(protocol_error(
                    "invalid_cord_arrangement",
                    "Awaiting Routing requires the Callee to be connected to the Ring Generator",
                ));
            }
        }
        exchange_protocol::CallPhase::Held => {
            if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
                next_game_phase = GamePhase::Shift;
                next_shift.phase = ShiftPhase::Active;
            } else if !input.cord_topology.is_empty() {
                return Err(protocol_error(
                    "invalid_cord_arrangement",
                    "a Held Caller can only be reconnected to the Operator Jack",
                ));
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
                if input.crank.rotation_count > 0 && input.crank.speed > 0 {
                    next_call.phase = exchange_protocol::CallPhase::Ringing;
                } else {
                    next_call.phase = exchange_protocol::CallPhase::AwaitingRouting;
                }
            } else if caller_operator {
                next_call.phase = exchange_protocol::CallPhase::OperatorSession;
            } else if input.cord_topology.is_empty() {
                next_call.phase = exchange_protocol::CallPhase::AwaitingRouting;
            } else {
                return Err(protocol_error(
                    "invalid_routing",
                    "the direct Circuit must connect the requested Callee",
                ));
            }
        }
        exchange_protocol::CallPhase::Connected => {
            if direct_circuit {
                next_call.phase = exchange_protocol::CallPhase::Completed;
            } else if input.cord_topology.is_empty() {
                next_shift.active_call_count = 0;
                return Ok(CallTransition {
                    call: None,
                    line_lamps: [false; 16],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                });
            } else {
                return Err(protocol_error(
                    "invalid_cord_arrangement",
                    "the connected Circuit must remain between the Caller and requested Callee",
                ));
            }
        }
        exchange_protocol::CallPhase::Completed => {
            if input.cord_topology.is_empty() {
                return Ok(CallTransition {
                    call: None,
                    line_lamps: [false; 16],
                    game_phase: next_game_phase,
                    shift: next_shift,
                    routing_receipt: None,
                });
            }
            if !direct_circuit {
                return Err(protocol_error(
                    "invalid_cord_arrangement",
                    "the completed Circuit must be cleared before changing cords",
                ));
            }
        }
        _ => {}
    }

    next_shift.active_call_count =
        u8::from(next_call.phase != exchange_protocol::CallPhase::Completed);
    Ok(CallTransition {
        line_lamps: lamps_for_call(Some(&next_call)),
        call: Some(next_call),
        game_phase: next_game_phase,
        shift: next_shift,
        routing_receipt,
    })
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
    let backend = Arc::new(Mutex::new(Backend::new()));
    for connection in listener.incoming() {
        let stream = connection?;
        let backend = Arc::clone(&backend);
        thread::spawn(move || {
            if let Err(error) = handle_connection(stream, backend) {
                eprintln!("frontend connection ended: {error}");
            }
        });
    }
    Ok(())
}

pub fn handle_connection(mut stream: TcpStream, backend: Arc<Mutex<Backend>>) -> io::Result<()> {
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
        let response = backend
            .lock()
            .map_err(|_| io::Error::other("backend state lock poisoned"))?
            .apply_input_snapshot(message);
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
    if message.message_kind != MessageKind::InputSnapshot {
        return Err(protocol_error(
            "unexpected_message_kind",
            "expected an input_snapshot message",
        ));
    }
    if message.session_id.is_empty() || message.session_id.len() > MAX_SESSION_ID_LENGTH {
        return Err(protocol_error(
            "invalid_session_id",
            "session_id must contain 1 to 64 bytes",
        ));
    }
    if message.message_id == 0 {
        return Err(protocol_error(
            "invalid_message_id",
            "message_id must be positive",
        ));
    }
    if let Some(last_message_id) = backend.last_message_id {
        if message.message_id <= last_message_id {
            return Err(protocol_error(
                "duplicate_message_id",
                "message_id must increase and must not be reused",
            ));
        }
    }
    if message.expected_state_revision != backend.state.state_revision {
        return Err(protocol_error(
            "stale_state_revision",
            format!(
                "expected state revision {}, received {}",
                backend.state.state_revision, message.expected_state_revision
            ),
        ));
    }

    let input = &message.input;
    if input.frontend.instance_id.is_empty()
        || input.frontend.instance_id.len() > MAX_INSTANCE_ID_LENGTH
    {
        return Err(protocol_error(
            "invalid_frontend_identity",
            "frontend instance_id must contain 1 to 64 bytes",
        ));
    }
    if input.input_sequence == 0 || input.input_sequence <= backend.state.input_sequence {
        return Err(protocol_error(
            "stale_input_sequence",
            "input_sequence must increase for every accepted snapshot",
        ));
    }
    if let Some(controller) = &backend.controller {
        if controller.session_id != message.session_id || controller.frontend != input.frontend {
            return Err(protocol_error(
                "controller_mismatch",
                "another session or frontend currently owns the controller",
            ));
        }
    }
    validate_cords(&input.cord_topology)?;
    if input.directory_digits.iter().any(|digit| *digit > 9) {
        return Err(protocol_error(
            "invalid_directory_digits",
            "directory digits must be decimal digits",
        ));
    }
    if input.crank.speed > 10_000 {
        return Err(protocol_error(
            "invalid_crank_speed",
            "crank speed is outside the supported range",
        ));
    }
    if input.tuning.coarse > 1_023 || input.tuning.fine > 1_023 {
        return Err(protocol_error(
            "invalid_tuning",
            "tuning values must be between 0 and 1023",
        ));
    }
    if input.diagnostics.device_faults.len() > MAX_FAULTS
        || input
            .diagnostics
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
            "a snapshot cannot contain more than 13 cords",
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
        PortId::Tap(index) if *index < 8 => Ok(()),
        PortId::Operator | PortId::RingGenerator => Ok(()),
        PortId::Subscriber(_) => Err(protocol_error(
            "invalid_port",
            "subscriber ports must be numbered 0 through 15",
        )),
        PortId::Tap(_) => Err(protocol_error(
            "invalid_port",
            "tap ports must be numbered 0 through 7",
        )),
    }
}

fn rejected_response(message_id: u64, error: ProtocolError, state: &StateSnapshot) -> StateMessage {
    StateMessage {
        protocol_version: PROTOCOL_VERSION,
        message_kind: MessageKind::StateSnapshot,
        message_id,
        accepted: false,
        error: Some(error),
        state: state.clone(),
    }
}

fn protocol_error(code: &str, message: impl Into<String>) -> ProtocolError {
    ProtocolError {
        code: code.to_string(),
        message: message.into(),
    }
}

fn initial_state() -> StateSnapshot {
    StateSnapshot {
        protocol_version: PROTOCOL_VERSION,
        frontend: FrontendIdentity {
            kind: exchange_protocol::FrontendKind::Odin,
            instance_id: "unassigned".to_string(),
        },
        session_id: String::new(),
        input_sequence: 0,
        state_revision: 0,
        cord_topology: Vec::new(),
        held_controls: HeldControls::default(),
        directory_digits: [0; 4],
        crank: CrankState::default(),
        tuning: TuningState::default(),
        reset_applied: false,
        line_lamps: [false; 16],
        game_phase: GamePhase::Ready,
        clock: ClockState {
            shift: 1,
            elapsed_seconds: 0,
        },
        directory_pages: directory_pages([0; 4]),
        printer_output: vec![PrinterEntry {
            entry_id: 1,
            text: "PROVINCIAL EXCHANGE READY".to_string(),
        }],
        call: None,
        shift: ShiftStatus {
            number: 1,
            phase: ShiftPhase::Ready,
            active_call_count: 0,
            completed_routings: 0,
        },
        diagnostics: DiagnosticState {
            frontend: FrontendDiagnostics::default(),
            messages: vec![BackendDiagnostic {
                code: "backend_ready".to_string(),
                message: "accepting complete frontend snapshots".to_string(),
            }],
        },
    }
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
