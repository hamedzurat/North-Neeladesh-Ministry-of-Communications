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

    let response = backend.handle(input(1, 1, [0, 0, 0, 1], false));

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
    let accepted = backend.handle(input(1, 1, [0, 0, 0, 1], false));

    let rejected = backend.handle(input_with_revision(2, 2, 1, [0, 0, 1, 10], false));

    assert!(!rejected.accepted);
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("invalid_directory_digits")
    );
    assert_eq!(rejected.state, accepted.state);
}

#[test]
fn reset_clears_game_state_without_erasing_printer_history() {
    let mut backend = Backend::new();
    backend.handle(input(1, 1, [0, 0, 0, 2], false));

    let reset = backend.handle(input_with_revision(2, 2, 1, [0, 0, 0, 2], true));

    assert!(reset.accepted);
    assert!(reset.state.reset_applied);
    assert_eq!(reset.state.clock.elapsed_seconds, 0);
    assert_eq!(reset.state.shift.phase, ShiftPhase::Ready);
    assert_eq!(reset.state.printer_output.last().unwrap().text, "RUN RESET");
    assert_eq!(reset.state.printer_output.len(), 2);
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

    let response = backend.handle(request);

    assert_eq!(response.state.cord_topology.len(), 1);
    assert_eq!(response.state.cord_topology[0].first, PortId::Subscriber(0));
    assert!(response.state.held_controls.ptt);
    assert!(response.state.held_controls.tap_listen[1]);
}

#[test]
fn retrying_the_same_reset_message_is_idempotent() {
    let mut backend = Backend::new();
    backend.handle(input(1, 1, [0, 0, 0, 1], false));
    let request = input_with_revision(2, 2, 1, [0, 0, 0, 1], true);

    let first = backend.handle(request.clone());
    let retry = backend.handle(request);

    assert_eq!(retry, first);
    assert_eq!(retry.state.printer_output.len(), 2);
}
