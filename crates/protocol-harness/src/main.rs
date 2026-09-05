use std::env;
use std::error::Error;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use exchange_backend::{Backend, handle_connection};
use exchange_protocol::{
    CrankState, FrontendDiagnostics, FrontendIdentity, FrontendKind, HeldControls, InputMessage,
    InputSnapshot, MessageKind, PROTOCOL_VERSION, StateMessage, TuningState, read_frame,
    write_frame,
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

    let second = exchange(stream, message(2, 2, 1, [0, 0, 0, 2], false))?;
    assert!(
        second.accepted,
        "directory update was rejected: {:?}",
        second.error
    );
    assert_eq!(second.state.directory_digits, [0, 0, 0, 2]);
    assert_eq!(second.state.directory_pages[0].heading, "VIRA DHAL");
    assert_eq!(second.state.printer_output.len(), 1);

    let reset = exchange(stream, message(3, 3, 2, [0, 0, 0, 2], true))?;
    assert!(reset.accepted, "reset was rejected: {:?}", reset.error);
    assert!(reset.state.reset_applied);
    assert_eq!(reset.state.clock.elapsed_seconds, 0);
    assert_eq!(reset.state.printer_output.last().unwrap().text, "RUN RESET");

    let invalid = exchange(stream, message(4, 4, 3, [0, 0, 0, 12], false))?;
    assert!(!invalid.accepted);
    assert_eq!(invalid.error.unwrap().code, "invalid_directory_digits");
    assert_eq!(invalid.state.state_revision, reset.state.state_revision);
    assert_eq!(invalid.state.directory_digits, reset.state.directory_digits);

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
