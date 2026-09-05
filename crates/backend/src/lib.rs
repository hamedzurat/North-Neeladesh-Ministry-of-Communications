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
        let previous_state = self.state.clone();
        let reset_state = if reset {
            initial_state()
        } else {
            previous_state
        };
        let printer_output = if reset {
            let mut output = self.state.printer_output.clone();
            output.push(PrinterEntry {
                entry_id: output.len() as u64 + 1,
                text: "RUN RESET".to_string(),
            });
            output
        } else {
            self.state.printer_output.clone()
        };

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
            line_lamps: reset_state.line_lamps,
            game_phase: reset_state.game_phase,
            clock: reset_state.clock,
            directory_pages: directory_pages(input.directory_digits),
            printer_output,
            call: reset_state.call,
            shift: reset_state.shift,
            diagnostics: DiagnosticState {
                frontend: input.diagnostics.clone(),
                messages: reset_state.diagnostics.messages,
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
