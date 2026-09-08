use std::thread;
use std::time::Duration;

use exchange_backend::Backend;
use exchange_protocol::{
    CallPhase, CordConnection, DebugCommand, HeldControls, InputDebug, InputMessage, InputState,
    PROTOCOL_VERSION, PortId, TuningState,
};

fn input(
    backend: &Backend,
    sequence: u64,
    cords: Vec<CordConnection>,
    directory_line: u8,
    crank: [u64; 4],
) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        expected_state_revision: backend.debug_snapshot().run.state_revision,
        input: InputState {
            cord_topology: cords,
            held_controls: HeldControls::default(),
            directory_digits: [0, 0, 0, directory_line],
            crank_rotation_timestamps: crank,
            tuning: TuningState::default(),
            debug: InputDebug::default(),
        },
    }
}

fn cord(first: PortId, second: PortId) -> CordConnection {
    CordConnection { first, second }
}

#[test]
fn live_hardware_loop_keeps_two_calls_on_lines_zero_through_five() {
    let mut backend = Backend::new_simple_hardware_demo();
    let waiting = backend.apply_input_message(input(&backend, 1, vec![], 1, [0; 4]));
    assert_eq!(waiting.output.calls.len(), 2);
    assert!(
        waiting
            .output
            .calls
            .iter()
            .all(|call| call.caller_line < 6 && call.requested_callee_line < 6)
    );
    assert!(
        waiting
            .output
            .calls
            .iter()
            .all(|call| call.caller_line != call.requested_callee_line)
    );
    assert_eq!(waiting.output.shift.active_call_count, 2);
    assert!(
        waiting
            .output
            .printer_output
            .iter()
            .any(|entry| entry.text.starts_with("PAPER NOTE //"))
    );
    assert!(
        waiting.output.directory_pages[0]
            .lines
            .iter()
            .any(|line| line.starts_with("USER //"))
    );
    assert_eq!(
        waiting
            .output
            .line_lamps
            .iter()
            .filter(|lamp| **lamp)
            .count(),
        2
    );

    let first = waiting.output.calls[0].clone();
    let operator = backend.apply_input_message(input(
        &backend,
        2,
        vec![cord(
            PortId::Subscriber(first.caller_line),
            PortId::Operator,
        )],
        first.requested_callee_line,
        [0; 4],
    ));
    assert_eq!(
        operator.output.call.unwrap().phase,
        CallPhase::OperatorSession
    );

    let wrong_directory = backend.apply_input_message(input(
        &backend,
        3,
        vec![
            cord(PortId::Subscriber(first.caller_line), PortId::Operator),
            cord(
                PortId::Subscriber(first.requested_callee_line),
                PortId::RingGenerator,
            ),
        ],
        (first.requested_callee_line + 1) % 6,
        [0, 100, 200, 300],
    ));
    assert!(!wrong_directory.accepted);
    assert_eq!(
        wrong_directory
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("directory_selection_required")
    );

    let ringing = backend.apply_input_message(input(
        &backend,
        4,
        vec![
            cord(PortId::Subscriber(first.caller_line), PortId::Operator),
            cord(
                PortId::Subscriber(first.requested_callee_line),
                PortId::RingGenerator,
            ),
        ],
        first.requested_callee_line,
        [0, 100, 200, 300],
    ));
    assert_eq!(ringing.output.call.unwrap().phase, CallPhase::Ringing);
    assert!(ringing.output.line_lamps[first.requested_callee_line as usize]);

    let connected = backend.apply_input_message(input(
        &backend,
        5,
        vec![cord(
            PortId::Subscriber(first.caller_line),
            PortId::Subscriber(first.requested_callee_line),
        )],
        first.requested_callee_line,
        [0; 4],
    ));
    assert_eq!(connected.output.call.unwrap().phase, CallPhase::Connected);
    assert!(
        connected
            .output
            .printer_output
            .iter()
            .any(|entry| entry.text.contains("EARNED +$5"))
    );
    assert!(connected.output.line_lamps[first.caller_line as usize]);
    assert!(connected.output.line_lamps[first.requested_callee_line as usize]);

    thread::sleep(Duration::from_secs(3));
    let next = backend.apply_input_message(input(
        &backend,
        6,
        vec![cord(
            PortId::Subscriber(first.caller_line),
            PortId::Subscriber(first.requested_callee_line),
        )],
        first.requested_callee_line,
        [0; 4],
    ));
    assert_eq!(next.output.calls.len(), 2);
    assert_eq!(next.output.shift.active_call_count, 2);
    assert!(
        !next
            .output
            .calls
            .iter()
            .any(|call| call.caller_line == first.caller_line)
    );
}

#[test]
fn simple_hardware_failure_prints_a_cost() {
    let mut backend = Backend::new_simple_hardware_demo();
    let waiting = backend.apply_input_message(input(&backend, 1, vec![], 1, [0; 4]));
    assert_eq!(waiting.output.calls.len(), 2);
    backend.apply_debug_command(DebugCommand::AdvanceTime { seconds: 31 });
    let failed = backend.apply_input_message(input(&backend, 2, vec![], 1, [0; 4]));

    assert!(
        failed
            .output
            .printer_output
            .iter()
            .any(|entry| entry.text.contains("COST -$2"))
    );
}
