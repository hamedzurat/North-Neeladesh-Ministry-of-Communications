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

fn apply_with_crank(
    backend: &mut Backend,
    sequence: &mut u64,
    cords: Vec<CordConnection>,
) -> exchange_protocol::StateMessage {
    let mut message = input(*sequence, *sequence - 1, cords, HeldControls::default());
    message.input.crank_rotation_timestamps = [0, 100, 200, 300];
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
}

#[test]
fn police_ems_and_fire_controls_share_press_release_service_behavior() {
    for (service, text) in [
        (ServiceKind::Police, "POLICE"),
        (ServiceKind::Ems, "EMS"),
        (ServiceKind::Fire, "FIRE"),
    ] {
        let mut backend = Backend::new();
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
        let active = apply(
            &mut backend,
            &mut sequence,
            vec![cord(PortId::Subscriber(0), PortId::Operator)],
            held,
        );
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
fn hardware_demo_exposes_directory_interference_police_and_tap_bridge_state() {
    let mut backend = Backend::new_hardware_demo();
    let mut sequence = 1;
    let send_demo = |backend: &mut Backend,
                     sequence: &mut u64,
                     cords: Vec<CordConnection>,
                     held_controls: HeldControls,
                     tuning: TuningState| {
        let mut message = input(*sequence, *sequence - 1, cords, held_controls);
        message.input.directory_digits = [0, 0, 0, 2];
        message.input.tuning = tuning;
        let response = backend.apply_input_message(message);
        *sequence += 1;
        response
    };

    let waiting = send_demo(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        TuningState::default(),
    );
    assert_eq!(
        waiting.output.call.as_ref().unwrap().phase,
        CallPhase::Waiting
    );
    assert_eq!(waiting.output.interference_level, 100);
    assert!(
        waiting.output.directory_pages[0]
            .lines
            .iter()
            .any(|line| line.contains("LINE LISTING"))
    );

    let ems_attempt = send_demo(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls {
            ems: true,
            ..HeldControls::default()
        },
        TuningState::default(),
    );
    assert_eq!(ems_attempt.output.service_call, None);
    assert!(
        ems_attempt
            .output
            .debug
            .messages
            .iter()
            .any(|message| message.code == "police_service_required")
    );
    send_demo(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
        TuningState::default(),
    );

    let police = HeldControls {
        police: true,
        ..HeldControls::default()
    };
    let police_active = send_demo(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        police,
        TuningState::default(),
    );
    assert_eq!(
        police_active.output.service_call.as_ref().unwrap().service,
        ServiceKind::Police
    );
    let police_completed = send_demo(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        TuningState::default(),
    );
    assert_eq!(police_completed.output.shift.completed_service_calls, 1);

    send_demo(
        &mut backend,
        &mut sequence,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
        HeldControls::default(),
        TuningState::default(),
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
    assert!(ringing.output.line_lamps[1]);

    let tap = send_demo(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Tap(3)),
            cord(PortId::Subscriber(1), PortId::Tap(4)),
        ],
        HeldControls::default(),
        TuningState {
            coarse: 512,
            fine: 512,
        },
    );
    assert_eq!(
        tap.output.call.as_ref().unwrap().phase,
        CallPhase::Connected
    );
    assert_eq!(tap.output.interference_level, 0);

    let wrong_bridge_control = send_demo(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Tap(3)),
            cord(PortId::Subscriber(1), PortId::Tap(4)),
        ],
        HeldControls {
            tap_1: true,
            ..HeldControls::default()
        },
        TuningState {
            coarse: 512,
            fine: 512,
        },
    );
    assert_eq!(wrong_bridge_control.output.tap_bridge_monitoring, None);
    assert!(!wrong_bridge_control.output.tap_bridge_audio_active);

    let listening_controls = HeldControls {
        tap_2: true,
        ..HeldControls::default()
    };
    let listening = send_demo(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Tap(3)),
            cord(PortId::Subscriber(1), PortId::Tap(4)),
        ],
        listening_controls.clone(),
        TuningState {
            coarse: 512,
            fine: 512,
        },
    );
    assert_eq!(listening.output.tap_bridge_monitoring, Some(2));
    assert!(listening.output.tap_bridge_audio_active);
    assert!(backend.debug_snapshot().story.operator_knowledge.is_empty());
    let listening_again = send_demo(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Tap(3)),
            cord(PortId::Subscriber(1), PortId::Tap(4)),
        ],
        listening_controls,
        TuningState {
            coarse: 512,
            fine: 512,
        },
    );
    assert_eq!(listening_again.output.tap_bridge_monitoring, Some(2));
    assert!(
        backend
            .debug_snapshot()
            .story
            .operator_knowledge
            .iter()
            .any(|fact| fact.contains("intercepted signal"))
    );

    let released = send_demo(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Tap(3)),
            cord(PortId::Subscriber(1), PortId::Tap(4)),
        ],
        HeldControls::default(),
        TuningState {
            coarse: 512,
            fine: 512,
        },
    );
    assert_eq!(released.output.tap_bridge_monitoring, None);
    assert!(!released.output.tap_bridge_audio_active);

    let completed = send_demo(
        &mut backend,
        &mut sequence,
        vec![
            cord(PortId::Subscriber(0), PortId::Tap(3)),
            cord(PortId::Subscriber(1), PortId::Tap(4)),
        ],
        HeldControls::default(),
        TuningState {
            coarse: 512,
            fine: 512,
        },
    );
    assert_eq!(
        completed.output.call.as_ref().unwrap().phase,
        CallPhase::Completed
    );
    let settled = send_demo(
        &mut backend,
        &mut sequence,
        vec![],
        HeldControls::default(),
        TuningState {
            coarse: 512,
            fine: 512,
        },
    );
    assert_eq!(
        settled.output.game_phase,
        exchange_protocol::GamePhase::Ended
    );
    assert_eq!(settled.output.shift.service_errors, 0);
    assert!(
        settled
            .output
            .printer_output
            .iter()
            .any(|entry| entry.text.contains("demonstration completed"))
    );

    backend.reset_run();
    assert_eq!(backend.debug_snapshot().run.elapsed_seconds, 8 * 60 * 60);
    assert_eq!(backend.debug_snapshot().story.current_node_id, "run_start");
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
