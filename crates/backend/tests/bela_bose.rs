use exchange_backend::Backend;
use exchange_protocol::{
    CordConnection, DebugCommand, DebugRequest, HeldControls, InputDebug, InputMessage, InputState,
    PROTOCOL_VERSION, PortId, TuningState,
};

fn input_with_ring(
    backend: &Backend,
    sequence: u64,
    cords: Vec<CordConnection>,
    digits: [u8; 4],
    ring_line: i16,
) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        expected_state_revision: backend.debug_snapshot().run.state_revision,
        input: InputState {
            cord_topology: cords,
            topology_revision: 0,
            held_controls: HeldControls::default(),
            directory_digits: digits,
            ring_line,
            crank_active: false,
            tuning: TuningState::default(),
            debug: InputDebug::default(),
        },
    }
}

fn input(
    backend: &Backend,
    sequence: u64,
    cords: Vec<CordConnection>,
    digits: [u8; 4],
) -> InputMessage {
    input_with_ring(backend, sequence, cords, digits, -1)
}

fn cord(first: PortId, second: PortId) -> CordConnection {
    CordConnection { first, second }
}

#[test]
fn registered_stories_start_with_professor_routing_and_authored_directory_records() {
    let mut backend = Backend::new_exchange();
    let reset = backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::ResetRun,
    });

    assert!(reset.accepted);
    assert_eq!(reset.snapshot.shapla_story_beat, "EmergencyCall");
    assert_eq!(reset.snapshot.neel_story_beat, "ProfessorRouting");

    let response = backend.apply_input_message(input(&backend, 1, vec![], [1, 0, 3, 1]));
    assert!(response.accepted);
    let dog = response
        .output
        .directory_pages
        .iter()
        .flat_map(|page| &page.lines)
        .collect::<Vec<_>>();
    assert_eq!(response.output.directory_pages[0].heading, "Meghna Abashon");
    assert_eq!(response.output.directory_pages[0].directory_id, Some(1031));
    assert_eq!(response.output.directory_pages[0].line, Some(4));
    assert!(dog.iter().any(|line| *line == "NAME // Bela Bose"));
    assert!(
        dog.iter()
            .any(|line| *line == "NOTE // has a dog named Momo")
    );

    let response = backend.apply_input_message(input(&backend, 2, vec![], [1, 0, 3, 2]));
    let cat = response
        .output
        .directory_pages
        .iter()
        .flat_map(|page| &page.lines)
        .collect::<Vec<_>>();
    assert_eq!(response.output.directory_pages[0].heading, "Padma Nibash");
    assert!(
        cat.iter()
            .any(|line| *line == "NOTE // has a cat named Tuli")
    );
    assert!(
        backend
            .debug_snapshot()
            .story_mechanics
            .iter()
            .any(|mechanic| mechanic == "bela_bose // directory_selection")
    );
}

#[test]
fn professor_must_wait_for_delayed_ring_activation_before_direct_connection() {
    let mut backend = Backend::new_exchange();
    backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::ResetRun,
    });
    let response = backend.apply_input_message(input(&backend, 1, vec![], [0, 0, 0, 1]));
    assert!(
        response
            .output
            .calls
            .iter()
            .any(|call| call.caller_line == 2)
    );
    let operator = vec![cord(PortId::Subscriber(2), PortId::Operator)];
    backend.apply_input_message(input(&backend, 2, operator.clone(), [0, 0, 0, 1]));
    let ringing = vec![
        cord(PortId::Subscriber(2), PortId::Operator),
        cord(PortId::Subscriber(3), PortId::RingGenerator),
    ];
    backend.apply_input_message(input_with_ring(
        &backend,
        3,
        ringing.clone(),
        [1, 0, 2, 4],
        3,
    ));
    assert!(!backend.frontend_state().line_lamps[3]);

    backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::AdvanceTime { seconds: 3 },
    });
    let ready = backend.apply_input_message(input_with_ring(&backend, 4, ringing, [1, 0, 2, 4], 3));
    assert!(ready.output.line_lamps[3]);

    let direct = vec![cord(PortId::Subscriber(2), PortId::Subscriber(3))];
    let response = backend.apply_input_message(input(&backend, 5, direct, [1, 0, 2, 4]));
    assert!(response.accepted);
    assert_eq!(
        response
            .output
            .calls
            .iter()
            .find(|call| call.caller_line == 2)
            .map(|call| call.phase.clone()),
        Some(exchange_protocol::CallPhase::Connected)
    );
}

#[test]
fn next_bela_call_waits_for_the_previous_direct_circuit_to_be_removed() {
    let mut backend = Backend::new_exchange();
    backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::ResetRun,
    });

    let operator = vec![cord(PortId::Subscriber(2), PortId::Operator)];
    backend.apply_input_message(input(&backend, 1, vec![], [0, 0, 0, 1]));
    backend.apply_input_message(input(&backend, 2, operator, [0, 0, 0, 1]));

    let ringing = vec![
        cord(PortId::Subscriber(2), PortId::Operator),
        cord(PortId::Subscriber(3), PortId::RingGenerator),
    ];
    backend.apply_input_message(input_with_ring(
        &backend,
        3,
        ringing.clone(),
        [1, 0, 2, 4],
        3,
    ));
    backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::AdvanceTime { seconds: 3 },
    });
    backend.apply_input_message(input_with_ring(&backend, 4, ringing, [1, 0, 2, 4], 3));

    let direct = vec![cord(PortId::Subscriber(2), PortId::Subscriber(3))];
    backend.apply_input_message(input(&backend, 5, direct.clone(), [1, 0, 2, 4]));
    let audio_duration = backend
        .debug_snapshot()
        .active_calls
        .iter()
        .find(|call| call.caller_line == 2)
        .expect("direct call should be active")
        .audio_duration_seconds;
    assert!(audio_duration > 0 && audio_duration < u64::MAX);
    backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::AdvanceTime {
            seconds: audio_duration as u32,
        },
    });
    let finished = backend.apply_input_message(input(&backend, 6, direct, [1, 0, 2, 4]));

    assert_eq!(finished.output.neel_story_beat, "ArnabDirectory");
    assert!(!finished.output.line_lamps[3]);
    assert!(
        !finished
            .output
            .calls
            .iter()
            .any(|call| call.caller_line == 3)
    );

    let next_call = backend.apply_input_message(input(&backend, 7, vec![], [1, 0, 2, 4]));
    assert!(!next_call.output.line_lamps[3]);
    assert!(
        !next_call
            .output
            .calls
            .iter()
            .any(|call| call.caller_line == 3),
        "the beat stays dormant until the selector has an available slot"
    );
}

#[test]
fn reset_selects_two_or_three_conflict_free_story_calls() {
    let mut backend = Backend::new_exchange();
    let reset = backend.apply_debug_command(DebugRequest {
        protocol_version: exchange_protocol::DEBUG_PROTOCOL_VERSION,
        command: DebugCommand::ResetRun,
    });

    assert!(reset.accepted);
    assert_eq!(reset.snapshot.shapla_story_beat, "EmergencyCall");
    assert_eq!(reset.snapshot.neel_story_beat, "ProfessorRouting");
    assert_eq!(reset.snapshot.dirty_work_story_beat, "Instruction");
    assert_eq!(reset.snapshot.nahid_story_beat, "Scamming");

    let response = backend.apply_input_message(input(&backend, 1, vec![], [0, 0, 0, 1]));
    let callers = response
        .output
        .calls
        .iter()
        .map(|call| call.caller_line)
        .collect::<Vec<_>>();
    let selected_story_calls = callers
        .iter()
        .filter(|caller| [1, 2, 3, 6, 8, 9, 10, 11].contains(caller))
        .count();
    assert!(
        (2..=3).contains(&selected_story_calls),
        "expected 2-3 selected story calls: {callers:?}"
    );
    let calls = &response.output.calls;
    for (index, call) in calls.iter().enumerate() {
        for other in &calls[index + 1..] {
            assert_ne!(call.caller_line, other.caller_line);
            assert_ne!(call.caller_line, other.requested_callee_line);
            assert_ne!(call.requested_callee_line, other.caller_line);
            if call.requested_callee_line != 0 && other.requested_callee_line != 0 {
                assert_ne!(call.requested_callee_line, other.requested_callee_line);
            }
        }
    }
}
