use std::env;
use std::error::Error;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use exchange_backend::{Backend, handle_connection};
use exchange_protocol::{
    CordConnection, HeldControls, InputDebug, InputMessage, InputState, PROTOCOL_VERSION, PortId,
    StateMessage, TuningState, read_frame, write_frame,
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
        "protocol harness passed: simplified input/output contract, routing, and rejection verified over TCP/CBOR"
    );
    Ok(())
}

fn run_sequence(stream: &mut TcpStream) -> Result<(), Box<dyn Error>> {
    let first = exchange(stream, message(1, 0, [0, 0, 0, 1]))?;
    assert!(
        first.accepted,
        "initial input was rejected: {:?}",
        first.error
    );
    assert_eq!(first.input_sequence, 1);
    assert_eq!(first.state_revision, 1);
    assert_eq!(first.output.call.as_ref().unwrap().caller_line, 0);
    assert!(first.output.line_lamps[0]);

    let second = exchange(stream, message(2, 1, [0, 0, 0, 2]))?;
    assert!(
        second.accepted,
        "directory update was rejected: {:?}",
        second.error
    );
    assert_eq!(second.output.directory_pages[0].heading, "VIRA DHAL");
    assert_eq!(second.output.printer_output.len(), 2);

    let arbitrary = exchange(
        stream,
        physical_message(
            3,
            2,
            vec![cord(PortId::Subscriber(6), PortId::Subscriber(14))],
            [0; 4],
        ),
    )?;
    assert!(arbitrary.accepted);
    assert!(arbitrary.output.call.is_some());

    let operator = exchange(
        stream,
        physical_message(
            4,
            3,
            vec![cord(PortId::Subscriber(0), PortId::Operator)],
            [0; 4],
        ),
    )?;
    assert!(operator.accepted);
    assert_eq!(
        operator.output.game_phase,
        exchange_protocol::GamePhase::Shift
    );

    let ringing = exchange(
        stream,
        physical_message(
            5,
            4,
            vec![
                cord(PortId::Subscriber(0), PortId::Operator),
                cord(PortId::Subscriber(1), PortId::RingGenerator),
            ],
            [0, 1000, 1100, 1200],
        ),
    )?;
    assert!(ringing.accepted);
    assert_eq!(
        ringing.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Ringing
    );

    let routed = exchange(
        stream,
        physical_message(
            6,
            5,
            vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
            [0; 4],
        ),
    )?;
    assert!(routed.accepted);
    assert_eq!(
        routed.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Connected
    );
    assert_eq!(routed.output.shift.completed_routings, 1);
    assert!(
        routed
            .output
            .printer_output
            .last()
            .unwrap()
            .text
            .contains("ROUTING")
    );

    let completed = exchange(
        stream,
        physical_message(
            7,
            6,
            vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
            [0; 4],
        ),
    )?;
    assert!(completed.accepted);
    assert_eq!(
        completed.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Completed
    );

    let cleared = exchange(stream, physical_message(8, 7, vec![], [0; 4]))?;
    assert!(
        cleared.accepted,
        "clearing the circuit was rejected: {:?}",
        cleared.error
    );
    assert!(cleared.output.call.is_none());
    assert_eq!(cleared.output.printer_output.len(), 4);
    assert!(
        cleared
            .output
            .printer_output
            .last()
            .unwrap()
            .text
            .contains("SERVICE ERROR")
    );

    let invalid = exchange(stream, message(9, 8, [0, 0, 0, 12]))?;
    assert!(!invalid.accepted);
    assert_eq!(invalid.error.unwrap().code, "invalid_directory_digits");
    assert_eq!(invalid.state_revision, cleared.state_revision);

    let restarted = exchange(stream, message(10, cleared.state_revision, [0, 0, 0, 2]))?;
    assert!(restarted.accepted);
    assert!(restarted.output.call.is_none());
    assert!(restarted.output.calls.is_empty());

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
    revision: u64,
    cord_topology: Vec<CordConnection>,
    crank_rotation_timestamps: [u64; 4],
) -> InputMessage {
    let mut message = message(sequence, revision, [0, 0, 0, 2]);
    message.input.cord_topology = cord_topology;
    message.input.crank_rotation_timestamps = crank_rotation_timestamps;
    message
}

fn message(sequence: u64, revision: u64, digits: [u8; 4]) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        expected_state_revision: revision,
        input: InputState {
            cord_topology: Vec::new(),
            held_controls: HeldControls::default(),
            directory_digits: digits,
            crank_rotation_timestamps: [0; 4],
            tuning: TuningState::default(),
            debug: InputDebug {
                firmware_version: Some("harness".to_string()),
                transport_connected: true,
                device_faults: Vec::new(),
            },
        },
    }
}
