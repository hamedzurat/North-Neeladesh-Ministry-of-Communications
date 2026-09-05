use std::env;
use std::error::Error;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use exchange_backend::{Backend, handle_connection};
use exchange_protocol::{
    CordConnection, CrankState, FrontendDiagnostics, FrontendIdentity, FrontendKind, HeldControls,
    InputMessage, InputSnapshot, MessageKind, PROTOCOL_VERSION, PortId, StateMessage, TuningState,
    read_frame, write_frame,
};

fn main() -> Result<(), Box<dyn Error>> {
    if let Some(address) = connect_address() {
        let mut stream = TcpStream::connect(&address)?;
        run_sequence(&mut stream)?;
        println!("protocol client passed against {address}");
        return Ok(());
    }

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let backend = Arc::new(Mutex::new(Backend::new()));
    let server_backend = Arc::clone(&backend);
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept()?;
        handle_connection(stream, server_backend)?;
        Ok::<(), std::io::Error>(())
    });

    let mut stream = TcpStream::connect(address)?;
    run_sequence(&mut stream)?;

    drop(stream);
    server
        .join()
        .map_err(|_| "protocol server thread panicked")??;
    println!(
        "protocol harness passed: complete snapshots, reset, and rejection verified over TCP/CBOR"
    );
    Ok(())
}

fn run_sequence(stream: &mut TcpStream) -> Result<(), Box<dyn Error>> {
    let first = exchange(stream, message(1, 1, 0, [0, 0, 0, 1], false))?;
    assert!(
        first.accepted,
        "initial snapshot was rejected: {:?}",
        first.error
    );
    assert_eq!(first.state.frontend.instance_id, "harness-odin");
    assert_eq!(first.state.state_revision, 1);
    assert!(first.state.diagnostics.frontend.transport_connected);
    assert_eq!(
        first.state.diagnostics.frontend.firmware_version.as_deref(),
        Some("harness")
    );
    assert_eq!(first.state.call.as_ref().unwrap().caller_line, 0);
    assert!(first.state.line_lamps[0]);

    let second = exchange(stream, message(2, 2, 1, [0, 0, 0, 2], false))?;
    assert!(
        second.accepted,
        "directory update was rejected: {:?}",
        second.error
    );
    assert_eq!(second.state.directory_digits, [0, 0, 0, 2]);
    assert_eq!(second.state.directory_pages[0].heading, "VIRA DHAL");
    assert_eq!(second.state.printer_output.len(), 1);

    let connected = exchange(
        stream,
        physical_message(
            3,
            3,
            2,
            vec![cord(PortId::Subscriber(0), PortId::Operator)],
            CrankState::default(),
        ),
    )?;
    assert!(connected.accepted);
    assert_eq!(
        connected.state.game_phase,
        exchange_protocol::GamePhase::Shift
    );

    let ringing = exchange(
        stream,
        physical_message(
            4,
            4,
            3,
            vec![
                cord(PortId::Subscriber(0), PortId::Operator),
                cord(PortId::Subscriber(1), PortId::RingGenerator),
            ],
            CrankState {
                rotation_count: 1,
                speed: 1,
            },
        ),
    )?;
    assert!(ringing.accepted);
    assert_eq!(
        ringing.state.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Ringing
    );

    let routed = exchange(
        stream,
        physical_message(
            5,
            5,
            4,
            vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
            CrankState::default(),
        ),
    )?;
    assert!(routed.accepted);
    assert_eq!(
        routed.state.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Connected
    );
    assert_eq!(routed.state.shift.completed_routings, 1);
    assert!(
        routed
            .state
            .printer_output
            .last()
            .unwrap()
            .text
            .contains("ROUTING")
    );

    let completed = exchange(
        stream,
        physical_message(
            6,
            6,
            5,
            vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
            CrankState::default(),
        ),
    )?;
    assert!(completed.accepted);
    assert_eq!(
        completed.state.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Completed
    );

    let cleared = exchange(
        stream,
        physical_message(7, 7, 6, Vec::new(), CrankState::default()),
    )?;
    assert!(cleared.accepted);
    assert!(cleared.state.call.is_none());

    let reset = exchange(stream, message(8, 8, 7, [0, 0, 0, 2], true))?;
    assert!(reset.accepted, "reset was rejected: {:?}", reset.error);
    assert!(reset.state.reset_applied);
    assert_eq!(reset.state.clock.elapsed_seconds, 0);
    assert!(reset.state.call.is_none());
    assert_eq!(reset.state.printer_output.len(), 1);

    let invalid = exchange(stream, message(9, 9, 8, [0, 0, 0, 12], false))?;
    assert!(!invalid.accepted);
    assert_eq!(invalid.error.unwrap().code, "invalid_directory_digits");
    assert_eq!(invalid.state.state_revision, reset.state.state_revision);
    assert_eq!(invalid.state.directory_digits, reset.state.directory_digits);

    let restarted = exchange(stream, message(10, 10, 8, [0, 0, 0, 2], false))?;
    assert!(restarted.accepted);
    assert!(restarted.state.line_lamps[0]);

    let reconnected = exchange(
        stream,
        physical_message(
            11,
            11,
            9,
            vec![cord(PortId::Subscriber(0), PortId::Operator)],
            CrankState::default(),
        ),
    )?;
    assert!(reconnected.accepted);

    let reringing = exchange(
        stream,
        physical_message(
            12,
            12,
            10,
            vec![
                cord(PortId::Subscriber(0), PortId::Operator),
                cord(PortId::Subscriber(2), PortId::RingGenerator),
            ],
            CrankState {
                rotation_count: 1,
                speed: 1,
            },
        ),
    )?;
    assert!(reringing.accepted);

    let rerouted = exchange(
        stream,
        physical_message(
            13,
            13,
            11,
            vec![cord(PortId::Subscriber(0), PortId::Subscriber(2))],
            CrankState::default(),
        ),
    )?;
    assert!(rerouted.accepted);
    assert_eq!(rerouted.state.shift.completed_routings, 1);

    Ok(())
}

fn connect_address() -> Option<String> {
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--connect" {
            return arguments.next();
        }
    }
    None
}

fn exchange(stream: &mut TcpStream, message: InputMessage) -> Result<StateMessage, Box<dyn Error>> {
    write_frame(stream, &message)?;
    Ok(read_frame(stream)?)
}

fn cord(first: PortId, second: PortId) -> CordConnection {
    CordConnection { first, second }
}

fn physical_message(
    sequence: u64,
    message_id: u64,
    expected_state_revision: u64,
    cord_topology: Vec<CordConnection>,
    crank: CrankState,
) -> InputMessage {
    let mut message = message(
        sequence,
        message_id,
        expected_state_revision,
        [0, 0, 0, 2],
        false,
    );
    message.input.cord_topology = cord_topology;
    message.input.crank = crank;
    message
}

fn message(
    sequence: u64,
    message_id: u64,
    expected_state_revision: u64,
    digits: [u8; 4],
    reset: bool,
) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        message_kind: MessageKind::InputSnapshot,
        session_id: "harness-session".to_string(),
        message_id,
        expected_state_revision,
        input: InputSnapshot {
            frontend: FrontendIdentity {
                kind: FrontendKind::Odin,
                instance_id: "harness-odin".to_string(),
            },
            input_sequence: sequence,
            cord_topology: Vec::new(),
            held_controls: HeldControls::default(),
            directory_digits: digits,
            crank: CrankState::default(),
            tuning: TuningState::default(),
            reset,
            diagnostics: FrontendDiagnostics {
                firmware_version: Some("harness".to_string()),
                transport_connected: true,
                device_faults: Vec::new(),
            },
        },
    }
}
