use exchange_backend::Backend;
use exchange_protocol::{
    CallPhase, CordConnection, HeldControls, InputDebug, InputMessage, InputState,
    PROTOCOL_VERSION, PortId, ServiceCallPhase, ServiceKind, TuningState,
};

fn input(
    sequence: u64,
    revision: u64,
    cords: Vec<CordConnection>,
    held_controls: HeldControls,
) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        expected_state_revision: revision,
        input: InputState {
            cord_topology: cords,
            held_controls,
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
    held_controls: HeldControls,
) -> exchange_protocol::StateMessage {
    let response =
        backend.apply_input_message(input(*sequence, *sequence - 1, cords, held_controls));
    *sequence += 1;
    response
}

#[test]
fn a_held_caller_survives_competing_call_handling_and_can_be_recalled() {
    let mut backend = Backend::new();
    let mut sequence = 1;

    let waiting = apply(&mut backend, &mut sequence, vec![], HeldControls::default());
    assert_eq!(waiting.output.call.unwrap().phase, CallPhase::Waiting);

    let operator = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
    );
    assert_eq!(operator.output.call.as_ref().unwrap().caller_line, 0);
    assert_eq!(
        operator.output.call.as_ref().unwrap().phase,
        CallPhase::OperatorSession
    );

    let competing = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(2), PortId::Operator)],
        HeldControls::default(),
    );
    assert_eq!(competing.output.calls.len(), 2);
    assert_eq!(competing.output.call.unwrap().caller_line, 2);
    assert!(
        competing
            .output
            .calls
            .iter()
            .any(|call| call.caller_line == 0 && call.phase == CallPhase::Held)
    );

    let recalled = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
    );
    assert_eq!(recalled.output.call.as_ref().unwrap().caller_line, 0);
    assert_eq!(
        recalled.output.call.as_ref().unwrap().phase,
        CallPhase::OperatorSession
    );
    assert!(
        recalled
            .output
            .calls
            .iter()
            .any(|call| call.caller_line == 2 && call.phase == CallPhase::Held)
    );
}

#[test]
fn tap_bridge_routes_a_circuit_and_only_monitors_while_held() {
    let mut backend = Backend::new();
    let mut sequence = 1;
    apply(&mut backend, &mut sequence, vec![], HeldControls::default());
    apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
    );

    let routed = apply(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Tap(1)),
            cord(PortId::Subscriber(1), PortId::Tap(2)),
        ],
        HeldControls::default(),
    );
    assert_eq!(routed.output.call.unwrap().phase, CallPhase::Connected);

    let mut listening = HeldControls::default();
    listening.tap_1 = true;
    let listening = apply(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Tap(1)),
            cord(PortId::Subscriber(1), PortId::Tap(2)),
        ],
        listening,
    );
    assert_eq!(listening.output.tap_bridge_monitoring, Some(1));
    assert!(listening.output.speaker_active);

    let released = apply(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Tap(1)),
            cord(PortId::Subscriber(1), PortId::Tap(2)),
        ],
        HeldControls::default(),
    );
    assert_eq!(released.output.tap_bridge_monitoring, None);
    assert!(!released.output.speaker_active);
}

#[test]
fn ems_service_calls_are_recorded_and_missing_ems_is_a_typed_error() {
    let mut backend = Backend::new();
    let mut sequence = 1;
    apply(&mut backend, &mut sequence, vec![], HeldControls::default());
    apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
    );
    let mut ems = HeldControls::default();
    ems.ems = true;
    let placed = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        ems,
    );
    assert_eq!(
        placed.output.service_call.as_ref().unwrap().service,
        ServiceKind::Ems
    );
    assert_eq!(
        placed.output.service_call.as_ref().unwrap().phase,
        ServiceCallPhase::Active
    );

    let recorded = apply(&mut backend, &mut sequence, vec![], HeldControls::default());
    assert_eq!(
        recorded.output.service_call.unwrap().phase,
        ServiceCallPhase::Completed
    );
    assert_eq!(recorded.output.shift.completed_service_calls, 1);
    assert_eq!(recorded.output.shift.service_errors, 0);

    let mut missing = Backend::new();
    let mut missing_sequence = 1;
    apply(
        &mut missing,
        &mut missing_sequence,
        vec![],
        HeldControls::default(),
    );
    apply(
        &mut missing,
        &mut missing_sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
    );
    apply(
        &mut missing,
        &mut missing_sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
        HeldControls::default(),
    );
    apply(
        &mut missing,
        &mut missing_sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
        HeldControls::default(),
    );
    let settled = apply(
        &mut missing,
        &mut missing_sequence,
        vec![],
        HeldControls::default(),
    );
    assert_eq!(settled.output.shift.service_errors, 1);
    assert_eq!(missing.story_node_id(), "ending_service_error");
    assert!(
        settled
            .output
            .printer_output
            .iter()
            .any(|entry| entry.text.contains("SERVICE ERROR"))
    );
}
