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
    directory_id: u16,
    _crank: [u64; 4],
) -> InputMessage {
    let ring_line = cords
        .iter()
        .find_map(|cord| match (&cord.first, &cord.second) {
            (PortId::RingGenerator, PortId::Subscriber(line))
            | (PortId::Subscriber(line), PortId::RingGenerator) => Some(i16::from(*line)),
            _ => None,
        });
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        expected_state_revision: backend.debug_snapshot().run.state_revision,
        input: InputState {
            cord_topology: cords,
            topology_revision: 0,
            held_controls: HeldControls {
                ptt: true,
                ..HeldControls::default()
            },
            directory_digits: [
                (directory_id / 1000) as u8,
                ((directory_id / 100) % 10) as u8,
                ((directory_id / 10) % 10) as u8,
                (directory_id % 10) as u8,
            ],
            ring_line: ring_line.unwrap_or(-1),
            crank_active: false,
            tuning: TuningState::default(),
            debug: InputDebug::default(),
        },
    }
}

fn cord(first: PortId, second: PortId) -> CordConnection {
    CordConnection { first, second }
}

fn directory_id(line: u8) -> u16 {
    match line {
        0 => 1021,
        1 => 1022,
        2 => 1023,
        3 => 1024,
        4 => 1031,
        5 => 1032,
        _ => panic!("line {line} is outside the demo"),
    }
}

#[test]
fn live_hardware_loop_keeps_three_calls_on_lines_zero_through_five() {
    let mut backend = Backend::new_simple_hardware_demo();
    let waiting = backend.apply_input_message(input(&backend, 1, vec![], 1021, [0; 4]));
    assert_eq!(waiting.output.calls.len(), 3);
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
    assert_eq!(waiting.output.shift.active_call_count, 3);
    assert!(
        waiting.output.directory_pages[0]
            .lines
            .iter()
            .any(|line| line.starts_with("NAME //"))
    );
    assert_eq!(
        waiting
            .output
            .line_lamps
            .iter()
            .filter(|lamp| **lamp)
            .count(),
        3
    );

    let first = waiting.output.calls[0].clone();
    let operator = backend.apply_input_message(input(
        &backend,
        2,
        vec![cord(
            PortId::Subscriber(first.caller_line),
            PortId::Operator,
        )],
        directory_id(first.requested_callee_line),
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
        directory_id((first.requested_callee_line + 1) % 6),
        [0, 0, 0, 100],
    ));
    assert!(wrong_directory.accepted);
    assert_eq!(
        wrong_directory.output.call.unwrap().phase,
        CallPhase::Ringing
    );

    let connected = backend.apply_input_message(input(
        &backend,
        4,
        vec![cord(
            PortId::Subscriber(first.caller_line),
            PortId::Subscriber(first.requested_callee_line),
        )],
        directory_id(first.requested_callee_line),
        [0; 4],
    ));
    assert_eq!(connected.output.call.unwrap().phase, CallPhase::Connected);
    assert!(
        !connected
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
        9,
        vec![cord(
            PortId::Subscriber(first.caller_line),
            PortId::Subscriber(first.requested_callee_line),
        )],
        directory_id(first.requested_callee_line),
        [0; 4],
    ));
    assert_eq!(next.output.calls.len(), 3);
    assert_eq!(next.output.shift.active_call_count, 3);
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
    let waiting = backend.apply_input_message(input(&backend, 1, vec![], 1021, [0; 4]));
    assert_eq!(waiting.output.calls.len(), 3);
    backend.apply_debug_command(exchange_protocol::DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::AdvanceTime { seconds: 65 },
    });
    let failed = backend.apply_input_message(input(&backend, 2, vec![], 1021, [0; 4]));

    assert!(
        !failed
            .output
            .printer_output
            .iter()
            .any(|entry| entry.text.contains("COST -$2"))
    );
}
