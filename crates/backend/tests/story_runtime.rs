use exchange_backend::Backend;
use exchange_protocol::{
    CallPhase, CordConnection, HeldControls, InputDebug, InputMessage, InputState,
    PROTOCOL_VERSION, PortId, TuningState,
};

fn input(sequence: u64, revision: u64, cords: Vec<CordConnection>) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        expected_state_revision: revision,
        input: InputState {
            cord_topology: cords,
            held_controls: HeldControls::default(),
            directory_digits: [0, 0, 0, 1],
            crank_rotation_timestamps: [0; 4],
            tuning: TuningState::default(),
            debug: InputDebug::default(),
        },
    }
}

fn cord(first: PortId, second: PortId) -> CordConnection {
    CordConnection { first, second }
}

fn apply(
    backend: &mut Backend,
    sequence: &mut u64,
    cords: Vec<CordConnection>,
) -> exchange_protocol::StateMessage {
    let response = backend.apply_input_message(input(*sequence, *sequence - 1, cords));
    *sequence += 1;
    response
}

#[test]
fn valid_routing_advances_the_authored_success_path_to_its_ending() {
    let mut backend = Backend::new();
    let mut sequence = 1;

    assert_eq!(backend.story_node_id(), "run_start");
    let waiting = apply(&mut backend, &mut sequence, vec![]);
    assert_eq!(backend.story_node_id(), "shift_call");
    assert_eq!(waiting.output.call.unwrap().phase, CallPhase::Waiting);

    let operator = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
    );
    assert_eq!(
        operator.output.call.unwrap().phase,
        CallPhase::OperatorSession
    );

    let ringing = {
        let mut response = input(
            sequence,
            sequence - 1,
            vec![
                cord(PortId::Subscriber(0), PortId::Operator),
                cord(PortId::Subscriber(1), PortId::RingGenerator),
            ],
        );
        response.input.crank_rotation_timestamps = [0, 100, 200, 300];
        let response = backend.apply_input_message(response);
        sequence += 1;
        response
    };
    assert_eq!(ringing.output.call.unwrap().phase, CallPhase::Ringing);

    let routed = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
    );
    assert_eq!(routed.output.call.unwrap().phase, CallPhase::Connected);
    assert_eq!(backend.story_node_id(), "event_success");
    let selection = backend.select_story_path(Some("ending_success"));
    assert!(selection.rejected_proposal);
    assert_eq!(backend.story_node_id(), "event_success");

    let completed = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
    );
    assert_eq!(completed.output.call.unwrap().phase, CallPhase::Completed);
    let cleared = apply(&mut backend, &mut sequence, vec![]);
    assert!(cleared.output.call.is_none());
    assert_eq!(backend.story_node_id(), "ending_success");
}

#[test]
fn missed_and_invalid_routing_follow_separate_authored_alternatives() {
    let mut backend = Backend::new();
    let mut sequence = 1;
    apply(&mut backend, &mut sequence, vec![]);
    apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
    );
    apply(&mut backend, &mut sequence, vec![]);
    let missed = apply(&mut backend, &mut sequence, vec![]);
    assert_eq!(missed.output.call.unwrap().phase, CallPhase::Missed);
    assert_eq!(backend.story_node_id(), "event_missed");
    apply(&mut backend, &mut sequence, vec![]);
    assert_eq!(backend.story_node_id(), "ending_missed");

    backend.reset_run();
    sequence = 1;
    apply(&mut backend, &mut sequence, vec![]);
    apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
    );
    let mut ringing = input(
        sequence,
        sequence - 1,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
    );
    ringing.input.crank_rotation_timestamps = [0, 100, 200, 300];
    backend.apply_input_message(ringing);
    sequence += 1;
    let invalid = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(2))],
    );
    assert_eq!(invalid.output.call.unwrap().phase, CallPhase::Misrouted);
    assert_eq!(backend.story_node_id(), "event_invalid");
    apply(&mut backend, &mut sequence, vec![]);
    assert_eq!(backend.story_node_id(), "ending_invalid");
}

#[test]
fn path_proposals_cannot_resolve_a_shift_call() {
    let mut backend = Backend::new();
    let selection = backend.select_story_path(Some("ending_invalid"));
    assert_eq!(selection.node_id, "shift_call");
    assert!(selection.used_default);
    assert_eq!(backend.story_node_id(), "shift_call");

    let waiting = backend.apply_input_message(input(1, 0, vec![]));
    assert!(waiting.accepted);
    let selection = backend.select_story_path(Some("event_invalid"));
    assert_eq!(selection.node_id, "shift_call");
    assert!(selection.rejected_proposal);
    assert_eq!(backend.story_node_id(), "shift_call");
}
