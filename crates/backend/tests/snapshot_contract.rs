use std::collections::BTreeMap;

use exchange_backend::Backend;
use exchange_protocol::{
    CordConnection, HeldControls, InputDebug, InputMessage, InputState, PROTOCOL_VERSION, PortId,
    ProtocolError, RtpL16Packet, TuningState, VOICE_PROTOCOL_VERSION, VoiceControl,
    VoiceInputAudioMessage, VoiceStatus, VoiceStatusMessage, encode_voice_input_audio,
    encode_voice_status,
};

fn input(sequence: u64, revision: u64, digits: [u8; 4]) -> InputMessage {
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        expected_state_revision: revision,
        input: InputState {
            cord_topology: Vec::new(),
            held_controls: HeldControls::default(),
            directory_digits: digits,
            crank_rotation_timestamps: [0; 4],
            tuning: TuningState {
                coarse: 400,
                fine: 600,
            },
            debug: InputDebug {
                firmware_version: Some("test-firmware".to_string()),
                transport_connected: true,
                device_faults: vec!["printer_low_paper".to_string()],
            },
        },
    }
}

fn cord(first: PortId, second: PortId) -> CordConnection {
    CordConnection { first, second }
}

fn topology_input(sequence: u64, revision: u64, cords: Vec<CordConnection>) -> InputMessage {
    let mut message = input(sequence, revision, [0, 0, 0, 1]);
    message.input.cord_topology = cords;
    message
}

#[test]
fn response_contains_backend_output_only() {
    let mut backend = Backend::new();
    let mut request = input(1, 0, [0, 0, 0, 1]);
    request.input.held_controls.ptt = true;
    request.input.cord_topology = vec![cord(PortId::Subscriber(0), PortId::Operator)];
    let response = backend.apply_input_message(request);
    assert!(response.accepted);
    assert!(response.output.speaker_active);
    assert_eq!(response.output.tuning.coarse, 400);
    assert_eq!(response.output.tuning.fine, 600);

    let value: BTreeMap<String, serde_cbor::Value> =
        serde_cbor::from_slice(&serde_cbor::to_vec(&response).unwrap()).unwrap();
    assert_eq!(
        value.keys().collect::<Vec<_>>(),
        vec![
            "accepted",
            "error",
            "input_sequence",
            "output",
            "protocol_version",
            "state_revision"
        ]
    );
    let output = match &value["output"] {
        serde_cbor::Value::Map(entries) => entries,
        value => panic!("expected output map, got {value:?}"),
    };
    let output_keys = output
        .keys()
        .map(|key| match key {
            serde_cbor::Value::Text(key) => key.as_str(),
            value => panic!("expected text output key, got {value:?}"),
        })
        .collect::<Vec<_>>();
    assert!(!output_keys.contains(&"cord_topology"));
    assert!(!output_keys.contains(&"held_controls"));
    assert!(!output_keys.contains(&"directory_digits"));
    assert!(!output_keys.contains(&"debug_frontend"));
}

#[test]
fn input_debug_is_not_copied_into_backend_output() {
    let mut backend = Backend::new();
    let response = backend.apply_input_message(input(1, 0, [0, 0, 0, 1]));

    assert!(response.accepted);
    assert_eq!(response.output.debug.messages[0].code, "backend_ready");
    assert!(
        !serde_cbor::to_vec(&response.output)
            .unwrap()
            .windows("test-firmware".len())
            .any(|window| window == b"test-firmware")
    );
    let frontend = backend.debug_snapshot().frontend;
    assert!(
        frontend
            .last_input_json
            .as_deref()
            .is_some_and(|json| json.contains("test-firmware"))
    );
    assert!(
        frontend
            .last_output_json
            .as_deref()
            .is_some_and(|json| json.contains("state_revision"))
    );
}

#[test]
fn rejected_input_is_retained_in_debug_evidence() {
    let mut backend = Backend::new();
    let mut request = input(1, 0, [0, 0, 0, 1]);
    request.protocol_version = PROTOCOL_VERSION + 1;

    let response = backend.apply_input_message(request);

    assert!(!response.accepted);
    assert_eq!(
        response.error.as_ref().unwrap().code,
        "unsupported_protocol_version"
    );
    let frontend = backend.debug_snapshot().frontend;
    assert!(
        frontend
            .last_input_json
            .as_deref()
            .is_some_and(|json| json.contains("test-firmware"))
    );
    assert!(
        frontend
            .last_output_json
            .as_deref()
            .is_some_and(|json| json.contains("unsupported_protocol_version"))
    );
    assert_eq!(
        backend.debug_snapshot().recent_errors.last().unwrap().code,
        "frontend_unsupported_protocol_version"
    );
}

#[test]
fn printer_output_is_backend_owned_across_inputs() {
    let mut backend = Backend::new_with_printer_stress(true);
    let first = backend.apply_input_message(input(1, 0, [0, 0, 0, 1]));
    assert!(first.accepted);
    assert!(
        first
            .output
            .printer_output
            .iter()
            .all(|entry| !entry.text.contains("PRINTER STRESS LINE"))
    );

    let second = backend.apply_input_message(input(2, first.state_revision, [0, 0, 0, 1]));
    assert_eq!(second.output.printer_output, first.output.printer_output);
}

#[test]
fn eight_cords_are_allowed_but_ninth_and_duplicate_endpoints_are_rejected() {
    let mut backend = Backend::new();
    let cords: Vec<_> = (0..8)
        .map(|index| cord(PortId::Subscriber(index), PortId::Subscriber(index + 8)))
        .collect();
    let accepted = backend.apply_input_message(topology_input(1, 0, cords.clone()));
    assert!(accepted.accepted);

    let too_many = backend.apply_input_message(topology_input(
        2,
        accepted.state_revision,
        [
            cord(PortId::Subscriber(0), PortId::Subscriber(1)),
            cord(PortId::Subscriber(2), PortId::Subscriber(3)),
            cord(PortId::Subscriber(4), PortId::Subscriber(5)),
            cord(PortId::Subscriber(6), PortId::Subscriber(7)),
            cord(PortId::Subscriber(8), PortId::Subscriber(9)),
            cord(PortId::Subscriber(10), PortId::Subscriber(11)),
            cord(PortId::Subscriber(12), PortId::Subscriber(13)),
            cord(PortId::Subscriber(14), PortId::Operator),
            cord(PortId::Subscriber(15), PortId::RingGenerator),
        ]
        .to_vec(),
    ));
    assert!(!too_many.accepted);
    assert_eq!(too_many.error.unwrap().code, "too_many_cords");

    let duplicate = backend.apply_input_message(topology_input(
        3,
        accepted.state_revision,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(0), PortId::RingGenerator),
        ],
    ));
    assert!(!duplicate.accepted);
    assert_eq!(duplicate.error.unwrap().code, "duplicate_port");
}

#[test]
fn arbitrary_physical_topology_is_accepted_without_advancing_routing() {
    let mut backend = Backend::new();
    let response = backend.apply_input_message(topology_input(
        1,
        0,
        vec![cord(PortId::Subscriber(6), PortId::Subscriber(14))],
    ));

    assert!(response.accepted);
    assert!(response.output.call.is_none());
    assert_eq!(
        response.output.game_phase,
        exchange_protocol::GamePhase::Ready
    );
}

#[test]
fn routing_requires_timestamped_crank_rotations_and_accepts_valid_topology() {
    let mut backend = Backend::new();

    let waiting = backend.apply_input_message(input(1, 0, [0, 0, 0, 1]));
    assert_eq!(waiting.output.call.as_ref().unwrap().caller_line, 0);

    let operator = backend.apply_input_message(topology_input(
        2,
        waiting.state_revision,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
    ));
    assert_eq!(
        operator.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::OperatorSession
    );

    let awaiting = backend.apply_input_message(topology_input(3, operator.state_revision, vec![]));
    assert_eq!(
        awaiting.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::AwaitingRouting
    );

    let mut no_rotation = topology_input(
        4,
        awaiting.state_revision,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
    );
    no_rotation.input.held_controls.tap_1 = true;
    let no_rotation = backend.apply_input_message(no_rotation);
    assert_eq!(
        no_rotation.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::AwaitingRouting
    );

    let mut ringing = topology_input(
        5,
        no_rotation.state_revision,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
    );
    ringing.input.crank_rotation_timestamps = [0, 1000, 1100, 1200];
    let ringing = backend.apply_input_message(ringing);
    assert_eq!(
        ringing.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Ringing
    );

    let operator_again = backend.apply_input_message(topology_input(
        6,
        ringing.state_revision,
        vec![cord(PortId::Subscriber(0), PortId::Operator)],
    ));
    assert_eq!(
        operator_again.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::OperatorSession
    );

    let stale_ring = backend.apply_input_message(topology_input(
        7,
        operator_again.state_revision,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
    ));
    assert_eq!(
        stale_ring.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::OperatorSession
    );

    let mut ringing_again = topology_input(
        8,
        stale_ring.state_revision,
        vec![
            cord(PortId::Subscriber(0), PortId::Operator),
            cord(PortId::Subscriber(1), PortId::RingGenerator),
        ],
    );
    ringing_again.input.crank_rotation_timestamps = [1000, 1100, 1200, 1300];
    let ringing_again = backend.apply_input_message(ringing_again);
    assert_eq!(
        ringing_again.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Ringing
    );

    let connected = backend.apply_input_message(topology_input(
        9,
        ringing_again.state_revision,
        vec![cord(PortId::Subscriber(0), PortId::Subscriber(1))],
    ));
    assert_eq!(
        connected.output.call.as_ref().unwrap().phase,
        exchange_protocol::CallPhase::Connected
    );
    assert_eq!(connected.output.shift.completed_routings, 1);
    assert!(
        connected
            .output
            .printer_output
            .last()
            .unwrap()
            .text
            .contains("ROUTING")
    );
}

#[test]
fn exact_retries_are_idempotent_and_sequence_reuse_is_rejected() {
    let mut backend = Backend::new();
    let request = input(1, 0, [0, 0, 0, 1]);
    let first = backend.apply_input_message(request.clone());
    let retry = backend.apply_input_message(request);
    assert_eq!(retry, first);

    let changed = backend.apply_input_message(input(1, 0, [0, 0, 0, 2]));
    assert!(!changed.accepted);
    assert_eq!(changed.error.unwrap().code, "duplicate_input_sequence");
    assert_eq!(changed.state_revision, first.state_revision);
}

#[test]
fn invalid_crank_history_is_rejected() {
    let mut backend = Backend::new();
    let mut request = input(1, 0, [0, 0, 0, 1]);
    request.input.crank_rotation_timestamps = [0, 1000, 900, 1100];

    let response = backend.apply_input_message(request);

    assert!(!response.accepted);
    assert_eq!(response.error.unwrap().code, "invalid_crank_timestamps");
    assert_eq!(response.state_revision, 0);
}

#[test]
fn voice_udp_status_and_audio_do_not_create_a_story_transition() {
    let mut backend = Backend::new();
    let ready = VoiceStatusMessage {
        protocol_version: VOICE_PROTOCOL_VERSION,
        session_id: 4,
        turn_id: 1,
        state_revision: 0,
        status: VoiceStatus::Ready,
        transcript: None,
        response_text: None,
        error: None,
    };
    assert!(backend.apply_voice_datagram(&encode_voice_status(&ready).unwrap()));

    let status = VoiceStatusMessage {
        status: VoiceStatus::Playing,
        response_text: Some("A response".to_string()),
        ..ready
    };
    assert!(backend.apply_voice_datagram(&encode_voice_status(&status).unwrap()));
    assert!(
        backend.apply_voice_datagram(
            &RtpL16Packet {
                marker: true,
                sequence: 0,
                timestamp: 0,
                ssrc: 4,
                samples: vec![1, -1],
            }
            .encode()
        )
    );
    assert!(!backend.apply_voice_datagram(&[0xff, 0x00]));

    let playing = backend.apply_input_message(input(1, 0, [0, 0, 0, 1]));
    assert!(playing.accepted);
    assert!(playing.output.speaker_active);
    assert!(playing.output.call.is_some());

    let current = VoiceStatusMessage {
        state_revision: 1,
        ..status
    };
    assert!(backend.apply_voice_datagram(&encode_voice_status(&current).unwrap()));

    let stale = VoiceStatusMessage {
        status: VoiceStatus::Failed,
        state_revision: 0,
        error: Some(ProtocolError {
            code: "late_worker".to_string(),
            message: "late result".to_string(),
        }),
        ..current.clone()
    };
    assert!(!backend.apply_voice_datagram(&encode_voice_status(&stale).unwrap()));

    let completed = VoiceStatusMessage {
        status: VoiceStatus::Completed,
        ..current
    };
    assert!(backend.apply_voice_datagram(&encode_voice_status(&completed).unwrap()));
    let idle = backend.apply_input_message(input(2, playing.state_revision, [0, 0, 0, 1]));
    assert!(!idle.output.speaker_active);
    assert_eq!(idle.output.shift.completed_routings, 0);
}

#[test]
fn voice_capture_failure_is_retained_as_conversation_evidence() {
    let mut backend = Backend::new();
    let ready = VoiceStatusMessage {
        protocol_version: VOICE_PROTOCOL_VERSION,
        session_id: 8,
        turn_id: 4,
        state_revision: 0,
        status: VoiceStatus::Ready,
        transcript: None,
        response_text: None,
        error: None,
    };
    assert!(backend.apply_voice_datagram(&encode_voice_status(&ready).unwrap()));

    let failed = VoiceStatusMessage {
        status: VoiceStatus::Failed,
        error: Some(ProtocolError {
            code: "capture_empty".to_string(),
            message: "microphone returned no samples".to_string(),
        }),
        ..ready
    };
    assert!(backend.apply_voice_datagram(&encode_voice_status(&failed).unwrap()));

    let conversation = &backend.debug_snapshot().voice.conversations[0];
    assert_eq!(conversation.status, Some(VoiceStatus::Failed));
    assert_eq!(conversation.error.as_ref().unwrap().code, "capture_empty");
    assert_eq!(conversation.captured_samples, 0);
}

#[test]
fn accepted_ptt_edges_are_forwarded_to_the_registered_voice_daemon() {
    let mut backend = Backend::new();
    let ready = VoiceStatusMessage {
        protocol_version: VOICE_PROTOCOL_VERSION,
        session_id: 1,
        turn_id: 1,
        state_revision: 0,
        status: VoiceStatus::Ready,
        transcript: None,
        response_text: None,
        error: None,
    };
    let peer = "127.0.0.1:45678".parse().unwrap();
    assert!(backend.apply_voice_datagram_from(&encode_voice_status(&ready).unwrap(), Some(peer)));

    let mut start = input(1, 0, [0, 0, 0, 1]);
    start.input.held_controls.ptt = true;
    assert!(backend.apply_input_message(start).accepted);
    let (start_control, start_peer) = backend.take_voice_control().unwrap();
    assert_eq!(start_peer, peer);
    assert_eq!(start_control.control, VoiceControl::StartPtt);
    assert!(!start_control.voice_id.is_empty());

    let mut release = input(2, 1, [0, 0, 0, 1]);
    release.input.held_controls.ptt = false;
    assert!(backend.apply_input_message(release).accepted);
    let release_control = backend.take_voice_control().unwrap().0;
    assert_eq!(release_control.control, VoiceControl::ReleasePtt);
    assert_eq!(release_control.voice_id, start_control.voice_id);

    let mut next_start = input(3, 2, [0, 0, 0, 1]);
    next_start.input.held_controls.ptt = true;
    assert!(backend.apply_input_message(next_start).accepted);
    let next_start_control = backend.take_voice_control().unwrap().0;
    assert_eq!(next_start_control.turn_id, start_control.turn_id + 1);
}

#[test]
fn voice_selection_follows_subscriber_identity_not_the_line_number() {
    let ready = VoiceStatusMessage {
        protocol_version: VOICE_PROTOCOL_VERSION,
        session_id: 1,
        turn_id: 1,
        state_revision: 0,
        status: VoiceStatus::Ready,
        transcript: None,
        response_text: None,
        error: None,
    };
    let peer = "127.0.0.1:45680".parse().unwrap();

    let mut taren_backend = Backend::new();
    assert!(
        taren_backend.apply_voice_datagram_from(&encode_voice_status(&ready).unwrap(), Some(peer))
    );
    let mut taren = input(1, 0, [0, 0, 0, 1]);
    taren.input.held_controls.ptt = true;
    assert!(taren_backend.apply_input_message(taren).accepted);
    assert_eq!(
        taren_backend.take_voice_control().unwrap().0.voice_id,
        "Ryan"
    );

    let mut vira_backend = Backend::new();
    assert!(
        vira_backend.apply_voice_datagram_from(&encode_voice_status(&ready).unwrap(), Some(peer))
    );
    assert!(
        vira_backend
            .apply_debug_command(exchange_protocol::DebugCommand::SetBypassRestrictions {
                enabled: true
            })
            .accepted
    );
    assert!(
        vira_backend
            .apply_debug_command(exchange_protocol::DebugCommand::InjectCall {
                caller_line: 1,
                callee_line: 0,
            })
            .accepted
    );
    let revision = vira_backend.debug_snapshot().run.state_revision;
    let mut vira = input(1, revision, [0, 0, 0, 1]);
    vira.input.held_controls.ptt = true;
    assert!(vira_backend.apply_input_message(vira).accepted);
    assert_eq!(
        vira_backend.take_voice_control().unwrap().0.voice_id,
        "Vivian"
    );
}

#[test]
fn remote_voice_input_requires_ordered_bounded_chunks() {
    let mut backend = Backend::new();
    let ready = VoiceStatusMessage {
        protocol_version: VOICE_PROTOCOL_VERSION,
        session_id: 9,
        turn_id: 2,
        state_revision: 0,
        status: VoiceStatus::Ready,
        transcript: None,
        response_text: None,
        error: None,
    };
    let peer = "127.0.0.1:45679".parse().unwrap();
    assert!(backend.apply_voice_datagram_from(&encode_voice_status(&ready).unwrap(), Some(peer)));

    let chunk = |chunk_index, complete, samples| {
        encode_voice_input_audio(&VoiceInputAudioMessage {
            protocol_version: VOICE_PROTOCOL_VERSION,
            session_id: 9,
            turn_id: 2,
            state_revision: 0,
            chunk_index,
            complete,
            samples,
        })
        .unwrap()
    };
    assert!(backend.apply_voice_datagram_from(&chunk(0, false, vec![1, 2]), Some(peer)));
    assert!(!backend.apply_voice_datagram_from(&chunk(2, true, vec![3]), Some(peer)));
    assert!(backend.apply_voice_datagram_from(&chunk(1, true, vec![3]), Some(peer)));
}

#[test]
fn voice_relay_can_reannounce_after_reset_or_a_new_udp_socket() {
    let mut backend = Backend::new();
    let ready = VoiceStatusMessage {
        protocol_version: VOICE_PROTOCOL_VERSION,
        session_id: 11,
        turn_id: 1,
        state_revision: 0,
        status: VoiceStatus::Ready,
        transcript: None,
        response_text: None,
        error: None,
    };
    let first_peer = "127.0.0.1:45680".parse().unwrap();
    let second_peer = "127.0.0.1:45681".parse().unwrap();
    assert!(
        backend.apply_voice_datagram_from(&encode_voice_status(&ready).unwrap(), Some(first_peer))
    );
    backend.reset_run();
    assert!(
        backend.apply_voice_datagram_from(&encode_voice_status(&ready).unwrap(), Some(second_peer))
    );
}
