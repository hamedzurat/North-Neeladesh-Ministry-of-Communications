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
    assert_eq!(first.output.calls.len(), 3);
    let call = first.output.calls[0].clone();

    let directory = exchange(
        stream,
        message(
            2,
            first.state_revision,
            [0, 0, 0, call.requested_callee_line],
        ),
    )?;
    assert!(
        directory.accepted,
        "directory update was rejected: {:?}",
        directory.error
    );
    assert!(
        directory.output.directory_pages[0]
            .lines
            .iter()
            .any(|line| line.contains("DESTINATION"))
    );

    let operator = exchange(
        stream,
        physical_message(
            3,
            directory.state_revision,
            vec![cord(PortId::Subscriber(call.caller_line), PortId::Operator)],
            [0; 4],
        ),
    )?;
    assert_eq!(
        operator.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::OperatorSession
    );

    let premature = exchange(
        stream,
        physical_message(
            4,
            operator.state_revision,
            vec![cord(
                PortId::Subscriber(call.caller_line),
                PortId::Subscriber(call.requested_callee_line),
            )],
            [0; 4],
        ),
    )?;
    assert!(!premature.accepted);
    assert_eq!(
        premature.error.as_ref().unwrap().code,
        "premature_direct_routing"
    );

    let ringing = exchange(
        stream,
        physical_message(
            5,
            premature.state_revision,
            vec![
                cord(PortId::Subscriber(call.caller_line), PortId::Operator),
                cord(
                    PortId::Subscriber(call.requested_callee_line),
                    PortId::RingGenerator,
                ),
            ],
            [0, 100, 200, 300],
        ),
    )?;
    assert_eq!(
        ringing.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Ringing
    );

    std::thread::sleep(std::time::Duration::from_secs(2));
    let sustained = exchange(
        stream,
        physical_message(
            6,
            ringing.state_revision,
            vec![
                cord(PortId::Subscriber(call.caller_line), PortId::Operator),
                cord(
                    PortId::Subscriber(call.requested_callee_line),
                    PortId::RingGenerator,
                ),
            ],
            [0, 100, 200, 400],
        ),
    )?;
    assert_eq!(
        sustained.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Ringing
    );

    let connected = exchange(
        stream,
        physical_message(
            7,
            sustained.state_revision,
            vec![cord(
                PortId::Subscriber(call.caller_line),
                PortId::Subscriber(call.requested_callee_line),
            )],
            [0; 4],
        ),
    )?;
    assert_eq!(
        connected.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Connected
    );
    assert!(connected.output.line_lamps[call.caller_line as usize]);
    assert!(connected.output.line_lamps[call.requested_callee_line as usize]);

    std::thread::sleep(std::time::Duration::from_secs(2));
    let completed = exchange(
        stream,
        physical_message(
            8,
            connected.state_revision,
            vec![cord(
                PortId::Subscriber(call.caller_line),
                PortId::Subscriber(call.requested_callee_line),
            )],
            [0; 4],
        ),
    )?;
    assert!(completed.accepted);
    assert_eq!(completed.output.calls.len(), 3);
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
    _crank_rotation_timestamps: [u64; 4],
) -> InputMessage {
    let mut message = message(sequence, revision, [0, 0, 0, 2]);
    message.input.cord_topology = cord_topology;
    message.input.ring_line = message
        .input
        .cord_topology
        .iter()
        .find_map(|cord| match (&cord.first, &cord.second) {
            (PortId::RingGenerator, PortId::Subscriber(line))
            | (PortId::Subscriber(line), PortId::RingGenerator) => Some(i16::from(*line)),
            _ => None,
        })
        .unwrap_or(-1);
    message
}

fn message(sequence: u64, revision: u64, digits: [u8; 4]) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        expected_state_revision: revision,
        input: InputState {
            cord_topology: Vec::new(),
            held_controls: HeldControls {
                ptt: true,
                ..HeldControls::default()
            },
            directory_digits: digits,
            ring_line: -1,
            tuning: TuningState::default(),
            debug: InputDebug {
                firmware_version: Some("harness".to_string()),
                transport_connected: true,
                device_faults: Vec::new(),
            },
        },
    }
}
