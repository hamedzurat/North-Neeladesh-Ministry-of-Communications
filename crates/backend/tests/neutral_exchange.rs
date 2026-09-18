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
            crank_rotation_timestamps: [0; 4],
            tuning: TuningState::default(),
            debug: InputDebug::default(),
        },
    }
}

#[test]
fn production_exchange_starts_with_three_non_conflicting_calls() {
    let mut backend = Backend::new_exchange();
    let response = backend.apply_input_message(first_input(&backend));

    assert!(response.accepted);
    assert_eq!(response.output.calls.len(), 3);
    for (index, call) in response.output.calls.iter().enumerate() {
        for other in response.output.calls.iter().skip(index + 1) {
            assert_ne!(call.caller_line, other.caller_line);
            assert_ne!(call.caller_line, other.requested_callee_line);
            assert_ne!(call.requested_callee_line, other.caller_line);
            assert_ne!(call.requested_callee_line, other.requested_callee_line);
        }
    }
}

#[test]
fn reset_preserves_demo_call_capacity() {
    let mut backend = Backend::new_simple_hardware_demo();
    backend.reset_run();

    assert_eq!(backend.frontend_state().calls.len(), 2);
}

#[test]
fn debug_time_expires_all_waiting_calls() {
    let mut backend = Backend::new_exchange();
    backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::AdvanceTime { seconds: 61 },
    });

    let response = backend.apply_input_message(first_input(&backend));

    assert!(response.accepted);
    assert_eq!(backend.money(), -6);
    assert_eq!(response.output.calls.len(), 3);
}
