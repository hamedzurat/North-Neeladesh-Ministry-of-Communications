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
            topology_revision: 0,
            held_controls: HeldControls::default(),
            directory_digits: [0, 0, 0, 1],
            ring_line: -1,
            crank_active: false,
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
fn production_exchange_starts_a_selected_set_of_story_callers() {
    let mut backend = Backend::new_exchange();
    let response = backend.apply_input_message(first_input(&backend));

    assert!(response.accepted);
    assert!((2..=3).contains(&response.output.calls.len()));
    assert!(
        backend
            .debug_snapshot()
            .events
            .iter()
            .any(|event| event.category == "input" && event.action == "accepted")
    );

    let response = backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::AdvanceTime { seconds: 8 },
    });
    assert!((2..=3).contains(&response.snapshot.active_calls.len()));
}

#[test]
fn reset_preserves_demo_call_capacity() {
    let mut backend = Backend::new_simple_hardware_demo();
    backend.reset_run();

    assert_eq!(backend.frontend_state().calls.len(), 3);
}

#[test]
fn selected_story_call_retries_after_patience_expiry() {
    let mut backend = Backend::new_exchange();
    let first = backend.apply_input_message(first_input(&backend));
    assert!((2..=3).contains(&first.output.calls.len()));

    backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::AdvanceTime { seconds: 44 },
    });
    let response = backend.apply_input_message(next_input(&backend));

    assert!(response.accepted);
    assert_eq!(backend.money(), -4);
    assert!((2..=3).contains(&response.output.calls.len()));
    assert_eq!(response.output.neel_story_beat, "ProfessorRouting");
}
