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
    let response = backend.apply_input_message(input(
        *sequence,
        backend.debug_snapshot().run.state_revision,
        cords,
        held_controls,
    ));
    *sequence += 1;
    response
}

fn apply_with_crank(
    backend: &mut Backend,
    sequence: &mut u64,
    cords: Vec<CordConnection>,
) -> exchange_protocol::StateMessage {
    let mut message = input(
        *sequence,
        backend.debug_snapshot().run.state_revision,
        cords,
        HeldControls::default(),
    );
    message.input.crank_rotation_timestamps = [0, 100, 200, 300];
    let response = backend.apply_input_message(message);
    *sequence += 1;
    response
}

fn apply_with_directory(
    backend: &mut Backend,
    sequence: &mut u64,
    cords: Vec<CordConnection>,
    held_controls: HeldControls,
    directory_id: u8,
) -> exchange_protocol::StateMessage {
    let mut message = input(
        *sequence,
        backend.debug_snapshot().run.state_revision,
        cords,
        held_controls,
    );
    message.input.directory_digits = [0, 0, 0, directory_id];
    let response = backend.apply_input_message(message);
    *sequence += 1;
    response
}

fn apply_with_directory_and_crank(
    backend: &mut Backend,
    sequence: &mut u64,
    cords: Vec<CordConnection>,
    directory_id: u8,
) -> exchange_protocol::StateMessage {
    let mut message = input(
        *sequence,
        backend.debug_snapshot().run.state_revision,
        cords,
        HeldControls::default(),
    );
    message.input.directory_digits = [0, 0, 0, directory_id];
    message.input.crank_rotation_timestamps = [0, 100, 200, 300];
    let response = backend.apply_input_message(message);
    *sequence += 1;
    response
}

fn apply_with_directory_tuning(
    backend: &mut Backend,
    sequence: &mut u64,
    cords: Vec<CordConnection>,
    held_controls: HeldControls,
    tuning: &TuningState,
    directory_id: u8,
) -> exchange_protocol::StateMessage {
    let mut message = input(
        *sequence,
        backend.debug_snapshot().run.state_revision,
        cords,
        held_controls,
    );
    message.input.directory_digits = [0, 0, 0, directory_id];
    message.input.tuning = tuning.clone();
    let response = backend.apply_input_message(message);
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
    apply(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
        HeldControls::default(),
    );
    let ringing = apply_with_crank(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
    );
    assert_eq!(
        ringing.output.call.as_ref().unwrap().phase,
        CallPhase::Ringing
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

    let listening = HeldControls {
        tap_1: true,
        ..HeldControls::default()
    };
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
    assert!(listening.output.tap_bridge_audio_active);
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
    assert!(!released.output.tap_bridge_audio_active);
    assert!(!released.output.speaker_active);
}

#[test]
fn direct_connection_before_ringing_is_rejected_without_a_routing_receipt() {
    let mut backend = Backend::new();
    let mut sequence = 1;
    apply(&mut backend, &mut sequence, vec![], HeldControls::default());
    apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
    );

    let direct = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
        HeldControls::default(),
    );

    assert_eq!(
        direct.output.call.as_ref().unwrap().phase,
        CallPhase::OperatorSession
    );
    assert!(!direct.accepted);
    assert_eq!(
        direct.error.as_ref().map(|error| error.code.as_str()),
        Some("ring_generator_required")
    );
    assert_eq!(direct.state_revision, 2);
    assert_eq!(direct.output.shift.completed_routings, 0);
    assert!(
        !direct
            .output
            .printer_output
            .iter()
            .any(|entry| entry.text.contains("ROUTING"))
    );
    assert!(
        direct
            .output
            .debug
            .messages
            .iter()
            .any(|message| message.code == "ring_generator_required")
    );

    let awaiting = apply(&mut backend, &mut sequence, vec![], HeldControls::default());
    assert_eq!(
        awaiting.output.call.as_ref().unwrap().phase,
        CallPhase::AwaitingRouting
    );
    let bypass = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
        HeldControls::default(),
    );
    assert_eq!(
        bypass.output.call.as_ref().unwrap().phase,
        CallPhase::AwaitingRouting
    );
    assert_eq!(bypass.output.shift.completed_routings, 0);
    assert!(
        bypass
            .output
            .debug
            .messages
            .iter()
            .any(|message| message.code == "ring_generator_required")
    );
}

#[test]
fn tap_bridge_connection_before_ringing_is_rejected() {
    let mut backend = Backend::new();
    let mut sequence = 1;
    apply(&mut backend, &mut sequence, vec![], HeldControls::default());
    let operator = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
    );
    let tap = apply(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Tap(1)),
            cord(PortId::Subscriber(1), PortId::Tap(2)),
        ],
        HeldControls::default(),
    );

    assert!(!tap.accepted);
    assert_eq!(tap.error.as_ref().unwrap().code, "ring_generator_required");
    assert_eq!(tap.state_revision, operator.state_revision);
    assert_eq!(tap.output.shift.completed_routings, 0);
}

#[test]
fn directory_selection_is_required_before_routing() {
    let mut backend = Backend::new_hardware_demo();
    let mut sequence = 1;
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        2,
    );
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
        2,
    );
    let ringing = apply_with_directory_and_crank(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
        2,
    );
    assert_eq!(
        ringing.output.call.as_ref().unwrap().phase,
        CallPhase::Ringing
    );

    let wrong_directory = apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
        HeldControls::default(),
        1,
    );
    assert!(!wrong_directory.accepted);
    assert_eq!(
        wrong_directory
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("directory_selection_required")
    );
    assert_eq!(
        wrong_directory.output.call.as_ref().unwrap().phase,
        CallPhase::Ringing
    );
    assert_eq!(wrong_directory.output.shift.completed_routings, 0);
}

#[test]
fn police_ems_and_fire_controls_share_press_release_service_behavior() {
    for (service, text) in [
        (ServiceKind::Police, "POLICE"),
        (ServiceKind::Ems, "EMS"),
        (ServiceKind::Fire, "FIRE"),
    ] {
        let mut backend = Backend::new_with_required_service(service);
        let mut sequence = 1;
        apply(&mut backend, &mut sequence, vec![], HeldControls::default());
        apply(
            &mut backend,
            &mut sequence,
            vec![cord(PortId::Subscriber(0), PortId::Operator)],
            HeldControls::default(),
        );

        let mut held = HeldControls::default();
        match service {
            ServiceKind::Police => held.police = true,
            ServiceKind::Ems => held.ems = true,
            ServiceKind::Fire => held.fire = true,
        }
        let active = if service == ServiceKind::Police {
            apply_with_directory(
                &mut backend,
                &mut sequence,
                vec![cord(PortId::Subscriber(0), PortId::Operator)],
                held,
                2,
            )
        } else {
            apply(
                &mut backend,
                &mut sequence,
                vec![cord(PortId::Subscriber(0), PortId::Operator)],
                held,
            )
        };
        assert_eq!(
            active.output.service_call.as_ref().unwrap().service,
            service
        );
        assert_eq!(
            active.output.service_call.as_ref().unwrap().phase,
            ServiceCallPhase::Active
        );

        let completed = apply(&mut backend, &mut sequence, vec![], HeldControls::default());
        assert_eq!(
            completed.output.service_call.as_ref().unwrap().service,
            service
        );
        assert_eq!(
            completed.output.service_call.as_ref().unwrap().phase,
            ServiceCallPhase::Completed
        );
        assert!(
            completed
                .output
                .printer_output
                .iter()
                .any(|entry| entry.text == format!("SERVICE {text} COMPLETED"))
        );
    }
}

#[test]
fn wrong_service_kind_does_not_satisfy_the_required_service() {
    let mut backend = Backend::new();
    let mut sequence = 1;
    apply(&mut backend, &mut sequence, vec![], HeldControls::default());
    apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
    );

    let police = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls {
            police: true,
            ..HeldControls::default()
        },
    );
    assert_eq!(police.output.service_call, None);
    assert_eq!(police.output.shift.completed_service_calls, 0);
    assert!(
        police
            .output
            .debug
            .messages
            .iter()
            .any(|message| message.code == "required_service_kind")
    );
}

#[test]
fn hardware_demo_authors_three_mechanical_shifts() {
    let backend = Backend::new_hardware_demo();
    let graph = backend.story_graph();

    assert!(graph.node("hardware_demo_call").is_some());
    assert!(graph.node("hardware_demo_interference_call").is_some());
    assert!(graph.node("hardware_demo_tap_call").is_some());
    assert_eq!(graph.outgoing("hardware_demo_call").len(), 3);
    assert_eq!(graph.outgoing("hardware_demo_interference_call").len(), 3);
    assert_eq!(graph.outgoing("hardware_demo_tap_call").len(), 3);
}

#[test]
fn hardware_demo_runtime_enforces_all_three_service_shifts() {
    let mut backend = Backend::new_hardware_demo();
    let mut sequence = 1;
    let clear = TuningState {
        coarse: 512,
        fine: 512,
    };

    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        2,
    );
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
        2,
    );
    assert_eq!(
        apply_with_directory(
            &mut backend,
            &mut sequence,
            vec![cord(PortId::Subscriber(0), PortId::Operator)],
            HeldControls {
                police: true,
                ..HeldControls::default()
            },
            2,
        )
        .output
        .service_call
        .unwrap()
        .service,
        ServiceKind::Police
    );
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        2,
    );
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
        2,
    );
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
        HeldControls::default(),
        2,
    );
    assert_eq!(
        apply_with_directory_and_crank(
            &mut backend,
            &mut sequence,
            vec![
                cord(PortId::Subscriber(0), PortId::Operator),
                cord(PortId::Subscriber(1), PortId::RingGenerator),
            ],
            2,
        )
        .output
        .call
        .unwrap()
        .phase,
        CallPhase::Ringing
    );
    apply_with_directory_tuning(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
        HeldControls::default(),
        &clear,
        2,
    );
    apply_with_directory_tuning(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
        HeldControls::default(),
        &clear,
        2,
    );
    apply_with_directory_tuning(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        &clear,
        2,
    );
    assert_eq!(backend.story_node_id(), "hardware_demo_interference_call");

    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        2,
    );
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(4), PortId::Operator)],
        HeldControls::default(),
        2,
    );
    assert_eq!(
        apply_with_directory(
            &mut backend,
            &mut sequence,
            vec![cord(PortId::Subscriber(4), PortId::Operator)],
            HeldControls {
                ems: true,
                ..HeldControls::default()
            },
            2,
        )
        .output
        .service_call
        .unwrap()
        .service,
        ServiceKind::Ems
    );
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        2,
    );
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(4), PortId::Operator)],
        HeldControls::default(),
        2,
    );
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(4), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
        HeldControls::default(),
        2,
    );
    apply_with_directory_and_crank(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(4), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
        2,
    );
    let blocked = apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(4), PortId::Subscriber(1))],
        HeldControls::default(),
        2,
    );
    assert_eq!(blocked.output.call.unwrap().phase, CallPhase::Ringing);
    apply_with_directory_tuning(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(4), PortId::Subscriber(1))],
        HeldControls::default(),
        &clear,
        2,
    );
    apply_with_directory_tuning(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(4), PortId::Subscriber(1))],
        HeldControls::default(),
        &clear,
        2,
    );
    apply_with_directory_tuning(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        &clear,
        2,
    );
    assert_eq!(backend.story_node_id(), "hardware_demo_tap_call");

    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        4,
    );
    apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(2), PortId::Operator)],
        HeldControls::default(),
        4,
    );
    let fire = apply_with_directory(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(2), PortId::Operator)],
        HeldControls {
            fire: true,
            ..HeldControls::default()
        },
        4,
    );
    assert_eq!(fire.output.service_call.unwrap().service, ServiceKind::Fire);
}

#[test]
fn hardware_demo_clock_maps_eight_real_minutes_to_the_shift_display() {
    let mut backend = Backend::new_hardware_demo();
    let advanced =
        backend.apply_debug_command(exchange_protocol::DebugCommand::AdvanceTime { seconds: 480 });

    assert!(advanced.accepted);
    assert_eq!(advanced.snapshot.run.elapsed_seconds, 16 * 60 * 60);
}

#[test]
fn hardware_demo_requires_directory_report_before_police_service() {
    let mut backend = Backend::new_hardware_demo();
    let mut sequence = 1;
    let mut directory = input(sequence, sequence - 1, vec![], HeldControls::default());
    directory.input.directory_digits = [0, 0, 0, 2];
    backend.apply_input_message(directory);
    sequence += 1;
    let directory_operator = input(
        sequence,
        sequence - 1,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
    );
    backend.apply_input_message(directory_operator);
    sequence += 1;
    let mut wrong_directory = input(
        sequence,
        sequence - 1,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
    );
    wrong_directory.input.directory_digits = [0, 0, 0, 1];
    backend.apply_input_message(wrong_directory);
    sequence += 1;
    let premature = apply(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls {
            police: true,
            ..HeldControls::default()
        },
    );
    assert_eq!(premature.output.service_call, None);
    assert!(
        premature
            .output
            .debug
            .messages
            .iter()
            .any(|message| message.code == "directory_report_required")
    );

    let operator_cord = vec![cord(PortId::Subscriber(0), PortId::Operator)];
    let mut directory = input(
        sequence,
        sequence - 1,
        operator_cord.clone(),
        HeldControls::default(),
    );
    directory.input.directory_digits = [0, 0, 0, 2];
    backend.apply_input_message(directory);
    sequence += 1;
    let mut police = input(
        sequence,
        sequence - 1,
        operator_cord,
        HeldControls {
            police: true,
            ..HeldControls::default()
        },
    );
    police.input.directory_digits = [0, 0, 0, 2];
    let police = backend.apply_input_message(police);
    assert_eq!(
        police.output.service_call.as_ref().unwrap().service,
        ServiceKind::Police
    );
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
    let ems = HeldControls {
        ems: true,
        ..HeldControls::default()
    };
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
    apply_with_crank(
        &mut missing,
        &mut missing_sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
    );
    apply(
        &mut missing,
        &mut missing_sequence,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
        HeldControls::default(),
    );
    assert_eq!(missing.story_node_id(), "event_success");
    assert_eq!(missing.debug_snapshot().shift.required_service_calls, 1);
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
