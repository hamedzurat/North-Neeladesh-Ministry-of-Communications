use exchange_backend::Backend;
use exchange_protocol::{
    DebugCommand, DebugRequest, HeldControls, InputDebug, InputMessage, InputState,
    PROTOCOL_VERSION, TuningState,
};

fn first_input(backend: &Backend) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: 1,
        expected_state_revision: backend.debug_snapshot().run.state_revision,
        input: InputState {
            cord_topology: Vec::new(),
            held_controls: HeldControls::default(),
            directory_digits: [0, 0, 0, 1],
            ring_line: -1,
            tuning: TuningState::default(),
            debug: InputDebug::default(),
        },
    }
}

fn next_input(backend: &Backend) -> InputMessage {
    let mut input = first_input(backend);
    input.input_sequence = 2;
    input
}

#[test]
fn production_exchange_starts_three_callers() {
    let mut backend = Backend::new_exchange();
    let response = backend.apply_input_message(first_input(&backend));

    assert!(response.accepted);
    assert_eq!(response.output.calls.len(), 3);

    let response = backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::AdvanceTime { seconds: 8 },
    });
    assert_eq!(response.snapshot.active_calls.len(), 3);
}

#[test]
fn reset_preserves_demo_call_capacity() {
    let mut backend = Backend::new_simple_hardware_demo();
    backend.reset_run();

    assert_eq!(backend.frontend_state().calls.len(), 3);
}

#[test]
fn patience_starts_when_each_call_is_shown() {
    let mut backend = Backend::new_exchange();
    let first = backend.apply_input_message(first_input(&backend));
    assert_eq!(first.output.calls.len(), 3);

    backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::AdvanceTime { seconds: 44 },
    });
    let response = backend.apply_input_message(next_input(&backend));

    assert!(response.accepted);
    assert_eq!(backend.money(), -2);
    assert_eq!(response.output.calls.len(), 3);
}
