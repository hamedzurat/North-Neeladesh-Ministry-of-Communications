use exchange_backend::Backend;
use exchange_protocol::{
    CallPhase, CordConnection, HeldControls, InputDebug, InputMessage, InputState,
    PROTOCOL_VERSION, PortId, TuningState,
};

fn message(sequence: u64, revision: u64, digits: u8, cords: Vec<CordConnection>) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        expected_state_revision: revision,
        input: InputState {
            cord_topology: cords,
            held_controls: HeldControls::default(),
            directory_digits: [0, 0, 0, digits],
            crank_rotation_timestamps: [0; 4],
            tuning: TuningState {
                coarse: 512,
                fine: 512,
            },
            debug: InputDebug::default(),
        },
    }
}

fn cord(first: PortId, second: PortId) -> CordConnection {
    CordConnection { first, second }
}

fn send(backend: &mut Backend, sequence: &mut u64, message: InputMessage) {
    let response = backend.apply_input_message(message);
    assert!(response.accepted, "input rejected: {:?}", response.error);
    *sequence += 1;
}

fn send_current(backend: &mut Backend, sequence: &mut u64, digits: u8, cords: Vec<CordConnection>) {
    let current = *sequence;
    send(
        backend,
        sequence,
        message(current, current - 1, digits, cords),
    );
}

fn route_call(backend: &mut Backend, sequence: &mut u64, caller: u8, callee: u8, digits: u8) {
    send(
        backend,
        sequence,
        message(*sequence, *sequence - 1, digits, vec![]),
    );
    send(
        backend,
        sequence,
        message(
            *sequence,
            *sequence - 1,
            digits,
            vec![cord(PortId::Subscriber(caller), PortId::Operator)],
        ),
    );
    send(
        backend,
        sequence,
        message(*sequence, *sequence - 1, digits, vec![]),
    );
    let mut ringing = message(
        *sequence,
        *sequence - 1,
        digits,
        vec![
            cord(PortId::Subscriber(caller), PortId::Operator),
            cord(PortId::Subscriber(callee), PortId::RingGenerator),
        ],
    );
    ringing.input.crank_rotation_timestamps = [0, 100, 200, 300];
    send(backend, sequence, ringing);
    let routed = backend.apply_input_message(message(
        *sequence,
        *sequence - 1,
        digits,
        vec![cord(PortId::Subscriber(caller), PortId::Subscriber(callee))],
    ));
    assert_eq!(
        routed.output.call.as_ref().unwrap().phase,
        CallPhase::Connected
    );
    *sequence += 1;
    send(
        backend,
        sequence,
        message(
            *sequence,
            *sequence - 1,
            digits,
            vec![cord(PortId::Subscriber(caller), PortId::Subscriber(callee))],
        ),
    );
    send(
        backend,
        sequence,
        message(*sequence, *sequence - 1, digits, vec![]),
    );
}

fn route_first_three_shifts(backend: &mut Backend, sequence: &mut u64) {
    route_call(backend, sequence, 0, 1, 2);

    let mut ems = message(
        *sequence,
        *sequence - 1,
        4,
        vec![cord(PortId::Subscriber(2), PortId::Operator)],
    );
    ems.input.held_controls.ems = true;
    send(backend, sequence, ems);
    send(
        backend,
        sequence,
        message(*sequence, *sequence - 1, 4, vec![]),
    );
    route_call(backend, sequence, 2, 3, 4);

    route_call(backend, sequence, 4, 1, 2);
}

#[test]
fn complete_four_shift_demo_reaches_trade_detente() {
    let mut backend = Backend::new_four_shift_demo();
    let mut sequence = 1;

    route_first_three_shifts(&mut backend, &mut sequence);
    assert_eq!(backend.story_node_id(), "final_choice");
    assert_eq!(backend.debug_snapshot().shift.number, 4);

    route_call(&mut backend, &mut sequence, 0, 1, 2);

    assert_eq!(backend.story_node_id(), "ending_trade_detente");
    assert_eq!(
        backend.debug_snapshot().run.game_phase,
        exchange_protocol::GamePhase::Ended
    );
    assert!(
        backend
            .debug_snapshot()
            .shift
            .service_error_counts
            .is_empty()
    );
    assert!(backend.debug_snapshot().shift.phase == exchange_protocol::ShiftPhase::Settled);

    backend.reset_run();
    let reset = backend.debug_snapshot();
    assert_eq!(reset.story.current_node_id, "run_start");
    assert_eq!(reset.shift.number, 1);
    assert_eq!(reset.calls.len(), 0);
    assert_eq!(reset.subscribers.len(), 5);
}

#[test]
fn final_oren_routing_reaches_managed_emergency_rule() {
    let mut backend = Backend::new_four_shift_demo();
    let mut sequence = 1;
    route_first_three_shifts(&mut backend, &mut sequence);

    route_call(&mut backend, &mut sequence, 3, 1, 4);

    assert_eq!(backend.story_node_id(), "ending_managed_emergency_rule");
}

#[test]
fn standoff_choice_reaches_renewed_civil_war_without_a_hidden_call() {
    let mut backend = Backend::new_four_shift_demo();
    let mut sequence = 1;
    route_first_three_shifts(&mut backend, &mut sequence);

    send_current(&mut backend, &mut sequence, 0, vec![]);
    assert_eq!(backend.story_node_id(), "ending_civil_war");
    assert_eq!(
        backend.debug_snapshot().run.game_phase,
        exchange_protocol::GamePhase::Ended
    );
}

#[test]
fn third_shift_supports_competing_calls_and_tap_bridge_monitoring() {
    let mut backend = Backend::new_four_shift_demo();
    let mut sequence = 1;
    route_call(&mut backend, &mut sequence, 0, 1, 2);

    let mut ems = message(
        sequence,
        sequence - 1,
        4,
        vec![cord(PortId::Subscriber(2), PortId::Operator)],
    );
    ems.input.held_controls.ems = true;
    send(&mut backend, &mut sequence, ems);
    send_current(&mut backend, &mut sequence, 4, vec![]);
    route_call(&mut backend, &mut sequence, 2, 3, 4);
    send_current(&mut backend, &mut sequence, 2, vec![]);
    send_current(
        &mut backend,
        &mut sequence,
        2,
        vec![cord(PortId::Subscriber(4), PortId::Operator)],
    );
    let competing = backend.apply_input_message(message(
        sequence,
        sequence - 1,
        2,
        vec![cord(PortId::Subscriber(1), PortId::Operator)],
    ));
    assert!(
        competing
            .output
            .calls
            .iter()
            .any(|call| call.caller_line == 4 && call.phase == CallPhase::Held)
    );
    assert!(
        competing
            .output
            .calls
            .iter()
            .any(|call| call.caller_line == 1 && call.phase == CallPhase::OperatorSession)
    );
    sequence += 1;

    send_current(
        &mut backend,
        &mut sequence,
        2,
        vec![cord(PortId::Subscriber(4), PortId::Operator)],
    );
    let tap = backend.apply_input_message(message(
        sequence,
        sequence - 1,
        2,
        vec![
            cord(PortId::Subscriber(4), PortId::Tap(1)),
            cord(PortId::Subscriber(1), PortId::Tap(2)),
        ],
    ));
    assert_eq!(
        tap.output.call.as_ref().unwrap().phase,
        CallPhase::Connected
    );
    sequence += 1;

    let mut listening = message(
        sequence,
        sequence - 1,
        2,
        vec![
            cord(PortId::Subscriber(4), PortId::Tap(1)),
            cord(PortId::Subscriber(1), PortId::Tap(2)),
        ],
    );
    listening.input.held_controls.tap_1 = true;
    let listening = backend.apply_input_message(listening);
    assert_eq!(listening.output.tap_bridge_monitoring, Some(1));
    assert!(listening.output.speaker_active);
    assert_eq!(
        backend.debug_snapshot().story.operator_knowledge,
        vec!["Neri Tal's intercepted signal mentions Vira Dhal"]
    );
}

#[test]
fn authored_caller_patience_expires_through_the_clock_boundary() {
    let mut backend = Backend::new_four_shift_demo();
    let waiting = backend.apply_input_message(message(1, 0, 2, vec![]));
    assert_eq!(
        waiting.output.call.as_ref().unwrap().phase,
        CallPhase::Waiting
    );

    let advanced =
        backend.apply_debug_command(exchange_protocol::DebugCommand::AdvanceTime { seconds: 90 });
    assert!(advanced.accepted);
    assert_eq!(advanced.snapshot.calls[0].phase, CallPhase::Missed);

    let cleared = backend.apply_input_message(message(2, 2, 2, vec![]));
    assert!(cleared.accepted);
    assert_eq!(backend.story_node_id(), "shift_2_call");
}

#[test]
fn shift_two_routing_waits_for_tuned_interference_controls() {
    let mut backend = Backend::new_four_shift_demo();
    let mut sequence = 1;
    route_call(&mut backend, &mut sequence, 0, 1, 2);
    send_current(&mut backend, &mut sequence, 4, vec![]);
    send_current(
        &mut backend,
        &mut sequence,
        4,
        vec![cord(PortId::Subscriber(2), PortId::Operator)],
    );

    let mut blocked = message(
        sequence,
        sequence - 1,
        4,
        vec![cord(PortId::Subscriber(2), PortId::Subscriber(3))],
    );
    blocked.input.tuning = TuningState::default();
    let blocked = backend.apply_input_message(blocked);
    assert_eq!(
        blocked.output.call.as_ref().unwrap().phase,
        CallPhase::OperatorSession
    );
    sequence += 1;

    let tuned = backend.apply_input_message(message(
        sequence,
        sequence - 1,
        4,
        vec![cord(PortId::Subscriber(2), PortId::Subscriber(3))],
    ));
    assert_eq!(
        tuned.output.call.as_ref().unwrap().phase,
        CallPhase::Connected
    );
    assert!(backend.debug_snapshot().story.interference_reduced);
}

#[test]
fn missed_shift_calls_and_omitted_ems_follow_authored_progression() {
    let mut backend = Backend::new_four_shift_demo();
    let mut sequence = 1;

    for (digits, caller, expected_next) in [
        (2, 0, "shift_2_call"),
        (4, 2, "shift_3_call"),
        (2, 4, "final_choice"),
    ] {
        send_current(&mut backend, &mut sequence, digits, vec![]);
        send_current(
            &mut backend,
            &mut sequence,
            digits,
            vec![cord(PortId::Subscriber(caller), PortId::Operator)],
        );
        send_current(&mut backend, &mut sequence, digits, vec![]);
        send_current(&mut backend, &mut sequence, digits, vec![]);
        send_current(&mut backend, &mut sequence, digits, vec![]);
        assert_eq!(backend.story_node_id(), expected_next);
    }

    assert_eq!(backend.debug_snapshot().shift.service_errors, 1);
    assert_eq!(backend.debug_snapshot().shift.number, 4);
    let choice = backend.select_story_path(Some("ending_civil_war"));
    assert!(!choice.rejected_proposal);
}
