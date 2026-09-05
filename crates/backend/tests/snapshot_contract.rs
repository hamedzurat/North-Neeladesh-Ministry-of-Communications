use exchange_backend::Backend;
use exchange_protocol::{
    CordConnection, CrankState, FrontendDiagnostics, FrontendIdentity, FrontendKind, HeldControls,
    InputMessage, InputSnapshot, MessageKind, PROTOCOL_VERSION, PortId, ShiftPhase, TuningState,
};

fn input(sequence: u64, message_id: u64, digits: [u8; 4], reset: bool) -> InputMessage {
    input_with_revision(sequence, message_id, sequence - 1, digits, reset)
}

fn input_with_revision(
    sequence: u64,
    message_id: u64,
    expected_state_revision: u64,
    digits: [u8; 4],
    reset: bool,
) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        message_kind: MessageKind::InputSnapshot,
        session_id: "test-session".to_string(),
        message_id,
        expected_state_revision,
        input: InputSnapshot {
            frontend: FrontendIdentity {
                kind: FrontendKind::Odin,
                instance_id: "test-odin".to_string(),
            },
            input_sequence: sequence,
            cord_topology: Vec::new(),
            held_controls: HeldControls::default(),
            directory_digits: digits,
            crank: CrankState {
                rotation_count: 4,
                speed: 12,
            },
            tuning: TuningState {
                coarse: 400,
                fine: 600,
            },
            reset,
            diagnostics: FrontendDiagnostics {
                firmware_version: Some("test-firmware".to_string()),
                transport_connected: true,
                device_faults: vec!["printer_low_paper".to_string()],
            },
        },
    }
}

#[test]
fn complete_snapshot_round_trips_frontend_input_and_directory_state() {
    let mut backend = Backend::new();

    let response = backend.apply_input_snapshot(input(1, 1, [0, 0, 0, 1], false));

    assert!(response.accepted);
    assert_eq!(response.state.frontend.kind, FrontendKind::Odin);
    assert_eq!(response.state.input_sequence, 1);
    assert_eq!(response.state.directory_digits, [0, 0, 0, 1]);
    assert_eq!(response.state.crank.rotation_count, 4);
    assert_eq!(response.state.tuning.fine, 600);
    assert_eq!(
        response
            .state
            .diagnostics
            .frontend
            .firmware_version
            .as_deref(),
        Some("test-firmware")
    );
    assert!(!response.state.directory_pages.is_empty());
}

#[test]
fn invalid_input_returns_the_unchanged_complete_state() {
    let mut backend = Backend::new();
    let accepted = backend.apply_input_snapshot(input(1, 1, [0, 0, 0, 1], false));

    let rejected = backend.apply_input_snapshot(input_with_revision(2, 2, 1, [0, 0, 1, 10], false));

    assert!(!rejected.accepted);
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("invalid_directory_digits")
    );
    assert_eq!(rejected.state, accepted.state);
}

#[test]
fn reset_clears_game_state_and_printer_history() {
    let mut backend = Backend::new();
    backend.apply_input_snapshot(input(1, 1, [0, 0, 0, 2], false));

    let reset = backend.apply_input_snapshot(input_with_revision(2, 2, 1, [0, 0, 0, 2], true));

    assert!(reset.accepted);
    assert!(reset.state.reset_applied);
    assert_eq!(reset.state.clock.elapsed_seconds, 0);
    assert_eq!(reset.state.shift.phase, ShiftPhase::Ready);
    assert!(reset.state.call.is_none());
    assert!(reset.state.printer_output[0].text.contains("READY"));
    assert_eq!(reset.state.printer_output.len(), 1);
}

#[test]
fn complete_physical_input_is_preserved_in_the_state_snapshot() {
    let mut backend = Backend::new();
    let mut request = input(1, 1, [0, 0, 0, 1], false);
    request.input.cord_topology = vec![CordConnection {
        first: PortId::Subscriber(0),
        second: PortId::Operator,
    }];
    request.input.held_controls = HeldControls {
        ptt: true,
        tap_listen: [false, true, false, false],
        ..HeldControls::default()
    };

    let response = backend.apply_input_snapshot(request);

    assert_eq!(response.state.cord_topology.len(), 1);
    assert_eq!(response.state.cord_topology[0].first, PortId::Subscriber(0));
    assert!(response.state.held_controls.ptt);
    assert!(response.state.held_controls.tap_listen[1]);
}

#[test]
fn retrying_the_same_reset_message_is_idempotent() {
    let mut backend = Backend::new();
    backend.apply_input_snapshot(input(1, 1, [0, 0, 0, 1], false));
    let request = input_with_revision(2, 2, 1, [0, 0, 0, 1], true);

    let first = backend.apply_input_snapshot(request.clone());
    let retry = backend.apply_input_snapshot(request);

    assert_eq!(retry, first);
    assert_eq!(retry.state.printer_output.len(), 1);
}

#[test]
fn reusing_an_older_message_id_is_rejected_without_state_change() {
    let mut backend = Backend::new();
    let first = backend.apply_input_snapshot(input(1, 1, [0, 0, 0, 1], false));
    backend.apply_input_snapshot(input(2, 2, [0, 0, 0, 2], false));
    let mut reused = input_with_revision(3, 1, 2, [0, 0, 0, 3], false);
    reused.input.input_sequence = 3;

    let rejected = backend.apply_input_snapshot(reused);

    assert!(!rejected.accepted);
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("duplicate_message_id")
    );
    assert_ne!(rejected.state, first.state);
    assert_eq!(rejected.state.directory_digits, [0, 0, 0, 2]);
}

#[test]
fn valid_routing_requires_ringing_and_clears_into_a_call_ready_shift() {
    let mut backend = Backend::new();

    let waiting = backend.apply_input_snapshot(input(1, 1, [0, 0, 0, 1], false));
    assert!(waiting.accepted);
    assert_eq!(waiting.state.call.as_ref().unwrap().caller_line, 0);
    assert_eq!(
        waiting.state.call.as_ref().unwrap().requested_callee_line,
        1
    );
    assert_eq!(
        waiting.state.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Waiting
    );
    assert!(waiting.state.line_lamps[0]);

    let operator = backend.apply_input_snapshot(corded_input(
        2,
        2,
        1,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
    ));
    assert!(operator.accepted);
    assert_eq!(
        operator.state.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::OperatorSession
    );
    assert_eq!(
        operator.state.game_phase,
        exchange_protocol::GamePhase::Shift
    );
    assert_eq!(operator.state.shift.phase, ShiftPhase::Active);

    let awaiting = backend.apply_input_snapshot(corded_input(3, 3, 2, Vec::new()));
    assert!(awaiting.accepted);
    assert_eq!(
        awaiting.state.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::AwaitingRouting
    );

    let before_ring = backend.apply_input_snapshot(corded_input(
        4,
        4,
        3,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
    ));
    assert!(!before_ring.accepted);
    assert_eq!(
        before_ring.error.as_ref().unwrap().code,
        "routing_before_ringing"
    );
    assert_eq!(before_ring.state, awaiting.state);

    let mut ring_without_crank = corded_input(
        5,
        5,
        3,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
    );
    ring_without_crank.input.crank = CrankState::default();
    let ring_without_crank = backend.apply_input_snapshot(ring_without_crank);
    assert!(!ring_without_crank.accepted);
    assert_eq!(
        ring_without_crank.error.as_ref().unwrap().code,
        "ringing_requires_crank"
    );

    let ringing = backend.apply_input_snapshot(corded_input_with_crank(
        6,
        6,
        3,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
    ));
    assert!(ringing.accepted);
    assert_eq!(
        ringing.state.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Ringing
    );
    assert!(ringing.state.line_lamps[1]);

    let wrong_callee = backend.apply_input_snapshot(corded_input(
        7,
        7,
        4,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(2))],
    ));
    assert!(!wrong_callee.accepted);
    assert_eq!(wrong_callee.error.as_ref().unwrap().code, "invalid_routing");
    assert_eq!(wrong_callee.state, ringing.state);

    let connected = backend.apply_input_snapshot(corded_input(
        8,
        8,
        4,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
    ));
    assert!(connected.accepted);
    assert_eq!(
        connected.state.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Connected
    );
    assert_eq!(connected.state.shift.completed_routings, 1);
    assert!(
        connected
            .state
            .printer_output
            .last()
            .unwrap()
            .text
            .contains("ROUTING")
    );

    let completed = backend.apply_input_snapshot(corded_input(
        9,
        9,
        5,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
    ));
    assert!(completed.accepted);
    assert_eq!(
        completed.state.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Completed
    );
    assert_eq!(completed.state.shift.completed_routings, 1);
    assert_eq!(completed.state.printer_output.len(), 2);

    let cleared = backend.apply_input_snapshot(corded_input(10, 10, 6, Vec::new()));
    assert!(cleared.accepted);
    assert!(cleared.state.call.is_none());
    assert_eq!(cleared.state.shift.phase, ShiftPhase::Active);
    assert!(cleared.state.line_lamps.iter().all(|lamp| !lamp));
}

#[test]
fn reset_clears_the_completed_call_and_allows_the_authored_call_to_start_again() {
    let mut backend = Backend::new();

    let first = backend.apply_input_snapshot(input(1, 1, [0, 0, 0, 1], false));
    let reset = backend.apply_input_snapshot(input_with_revision(2, 2, 1, [0, 0, 0, 1], true));
    assert!(reset.accepted);
    assert!(reset.state.call.is_none());
    assert_eq!(reset.state.printer_output.len(), 1);
    assert_eq!(reset.state.shift.completed_routings, 0);

    let restarted = backend.apply_input_snapshot(input_with_revision(
        3,
        3,
        reset.state.state_revision,
        [0, 0, 0, 1],
        false,
    ));
    assert!(restarted.accepted);
    assert_eq!(restarted.state.call.as_ref().unwrap().caller_line, 0);
    assert!(restarted.state.line_lamps[0]);
    assert_ne!(restarted.state.state_revision, first.state.state_revision);
}

#[test]
fn clearing_a_connected_circuit_clears_the_active_call_count() {
    let mut backend = Backend::new();

    backend.apply_input_snapshot(input(1, 1, [0, 0, 0, 1], false));
    backend.apply_input_snapshot(corded_input(
        2,
        2,
        1,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
    ));
    backend.apply_input_snapshot(corded_input_with_crank(
        3,
        3,
        2,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
    ));
    let connected = backend.apply_input_snapshot(corded_input(
        4,
        4,
        3,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
    ));
    assert_eq!(
        connected.state.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Connected
    );

    let cleared = backend.apply_input_snapshot(corded_input(5, 5, 4, Vec::new()));
    assert!(cleared.accepted);
    assert!(cleared.state.call.is_none());
    assert_eq!(cleared.state.shift.active_call_count, 0);
}

fn cord(first: PortId, second: PortId) -> CordConnection {
    CordConnection { first, second }
}

fn corded_input(
    sequence: u64,
    message_id: u64,
    expected_state_revision: u64,
    cords: Vec<CordConnection>,
) -> InputMessage {
    let mut request = input_with_revision(
        sequence,
        message_id,
        expected_state_revision,
        [0, 0, 0, 1],
        false,
    );
    request.input.cord_topology = cords;
    request
}

fn corded_input_with_crank(
    sequence: u64,
    message_id: u64,
    expected_state_revision: u64,
    cords: Vec<CordConnection>,
) -> InputMessage {
    let mut request = corded_input(sequence, message_id, expected_state_revision, cords);
    request.input.crank = CrankState {
        rotation_count: 1,
        speed: 1,
    };
    request
}
