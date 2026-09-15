use std::io::{self, BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

use exchange_backend::Backend;
use exchange_backend::story::{OperatorServiceReport, OperatorTurn, operator_action_from_name};
use exchange_protocol::{
    CordConnection, HeldControls, InputDebug, InputMessage, InputState, PROTOCOL_VERSION, PortId,
    TuningState,
};
use serde_json::json;

fn frontend_view(backend: &Backend) -> serde_json::Value {
    json!(backend.frontend_state())
}

fn emit(
    backend: &Backend,
    ok: bool,
    error: Option<String>,
    action: Option<String>,
    controls: Option<&str>,
) {
    let mut output = json!({ "ok": ok, "view": frontend_view(backend) });
    output["allowed_intents"] = json!(backend.allowed_operator_intents());
    output["caller"] =
        backend
            .frontend_state()
            .call
            .as_ref()
            .map_or(serde_json::Value::Null, |_| {
                backend
                    .story_call_id()
                    .and_then(|call_id| backend.story_graph().caller_prompt(call_id))
                    .map_or(serde_json::Value::Null, |prompt| {
                        json!({
                            "name": prompt.name,
                            "opening": prompt.opening,
                            "reveals": prompt.reveals,
                        })
                    })
            });
    if let Some(action) = action {
        output["action"] = json!(action);
    }
    if let Some(controls) = controls {
        output["controls"] = json!(controls);
    }
    if let Some(error) = error {
        output["error"] = json!(error);
    }
    println!("{}", output);
    io::stdout().flush().expect("flush response");
}

fn send(
    backend: &mut Backend,
    sequence: &mut u64,
    directory: u16,
    cords: Vec<CordConnection>,
    crank: [u64; 4],
) {
    send_with_controls(
        backend,
        sequence,
        directory,
        cords,
        crank,
        HeldControls::default(),
    );
}

fn send_with_controls(
    backend: &mut Backend,
    sequence: &mut u64,
    directory: u16,
    cords: Vec<CordConnection>,
    crank: [u64; 4],
    held_controls: HeldControls,
) {
    let revision = backend.debug_snapshot().run.state_revision;
    let response = backend.apply_input_message(InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: *sequence,
        expected_state_revision: revision,
        input: InputState {
            cord_topology: cords,
            held_controls,
            directory_digits: [0, 0, 0, directory as u8],
            crank_rotation_timestamps: crank,
            tuning: TuningState { coarse: 0, fine: 0 },
            debug: InputDebug::default(),
        },
    });
    assert!(
        response.accepted,
        "frontend tick rejected: {:?}",
        response.error
    );
    *sequence += 1;
}

fn advance_automatic_story(backend: &mut Backend) {
    loop {
        let Some(node) = backend.story_graph().node(backend.story_node_id()) else {
            return;
        };
        let kind = node.kind.clone();
        match kind {
            exchange_backend::story::StoryNodeKind::ShiftCall { .. }
            | exchange_backend::story::StoryNodeKind::Ending { .. } => return,
            exchange_backend::story::StoryNodeKind::RunStart { .. }
            | exchange_backend::story::StoryNodeKind::StoryEvent { .. }
            | exchange_backend::story::StoryNodeKind::Conditional { .. } => {
                backend.select_story_path(None);
            }
        }
    }
}

fn frontend_tick(
    backend: &mut Backend,
    sequence: &mut u64,
    complete: bool,
    action: Option<exchange_backend::story::OperatorTextAction>,
    caller_override: Option<u8>,
    directory: u16,
    callee: Option<u8>,
) {
    advance_automatic_story(backend);
    let node = backend.story_graph().node(backend.story_node_id()).unwrap();
    let exchange_backend::story::StoryNodeKind::ShiftCall { beat_id, .. } = &node.kind else {
        return;
    };
    let beat = backend.story_graph().story_beat(beat_id).unwrap();
    let premise = backend
        .story_graph()
        .call_premise(&beat.call_premise_id)
        .unwrap();
    let authored_caller = backend
        .story_graph()
        .line_listing(&premise.caller_line_id)
        .unwrap()
        .line;
    let caller = caller_override.unwrap_or(authored_caller);
    let terminal_call = backend.frontend_state().call.as_ref().is_some_and(|call| {
        matches!(
            call.phase,
            exchange_protocol::CallPhase::Completed
                | exchange_protocol::CallPhase::Missed
                | exchange_protocol::CallPhase::Misrouted
                | exchange_protocol::CallPhase::Failed
        )
    });
    let had_call = backend.frontend_state().call.is_some();
    // A new authored call needs its own directory entry to appear. Once the
    // call exists, routing must use the operator's explicit lookup instead.
    let physical_directory = if had_call {
        directory
    } else {
        premise.directory_ids[0]
    };
    if had_call
        && matches!(
            action,
            Some(
                exchange_backend::story::OperatorTextAction::Ask
                    | exchange_backend::story::OperatorTextAction::DirectoryCheck
            )
        )
    {
        send(
            backend,
            sequence,
            physical_directory,
            vec![CordConnection {
                first: PortId::Subscriber(caller),
                second: PortId::Operator,
            }],
            [0; 4],
        );
        return;
    }
    send(backend, sequence, physical_directory, vec![], [0; 4]);
    if terminal_call {
        advance_automatic_story(backend);
        if backend.frontend_state().call.is_none() {
            frontend_tick(backend, sequence, false, None, None, 0, None);
        }
        return;
    }
    if !complete && !had_call {
        return;
    }

    let operator_cord = vec![CordConnection {
        first: PortId::Subscriber(caller),
        second: PortId::Operator,
    }];
    match action {
        Some(
            exchange_backend::story::OperatorTextAction::Ask
            | exchange_backend::story::OperatorTextAction::DirectoryCheck,
        ) => {
            // Asking and checking the Directory are operator interactions, not routes.
            send(backend, sequence, directory, operator_cord, [0; 4]);
            return;
        }
        Some(exchange_backend::story::OperatorTextAction::Tap) => {}
        _ => {}
    }

    if matches!(
        action,
        Some(
            exchange_backend::story::OperatorTextAction::CallEms
                | exchange_backend::story::OperatorTextAction::ReportPolice
        )
    ) {
        send(backend, sequence, directory, operator_cord, [0; 4]);
        if complete {
            let held = HeldControls {
                ems: action == Some(exchange_backend::story::OperatorTextAction::CallEms),
                police: action == Some(exchange_backend::story::OperatorTextAction::ReportPolice),
                ..HeldControls::default()
            };
            send_with_controls(
                backend,
                sequence,
                directory,
                vec![CordConnection {
                    first: PortId::Subscriber(caller),
                    second: PortId::Operator,
                }],
                [0; 4],
                held,
            );
            send(backend, sequence, directory, vec![], [0; 4]);
        }
        advance_automatic_story(backend);
        if complete && backend.frontend_state().call.is_none() {
            frontend_tick(backend, sequence, false, None, None, 0, None);
        }
        return;
    }

    let Some(callee) = callee.filter(|line| *line < 12 && *line != caller) else {
        return;
    };
    send(backend, sequence, directory, operator_cord.clone(), [0; 4]);
    send(backend, sequence, directory, vec![], [0; 4]);
    send(
        backend,
        sequence,
        directory,
        vec![
            CordConnection {
                first: PortId::Subscriber(caller),
                second: PortId::Operator,
            },
            CordConnection {
                first: PortId::Subscriber(callee),
                second: PortId::RingGenerator,
            },
        ],
        [1_000, 1_100, 1_200, 1_300],
    );
    if complete && action != Some(exchange_backend::story::OperatorTextAction::Tap) {
        {
            send(
                backend,
                sequence,
                directory,
                vec![CordConnection {
                    first: PortId::Subscriber(caller),
                    second: PortId::Subscriber(callee),
                }],
                [0; 4],
            );
            send(
                backend,
                sequence,
                directory,
                vec![CordConnection {
                    first: PortId::Subscriber(caller),
                    second: PortId::Subscriber(callee),
                }],
                [0; 4],
            );
            send(backend, sequence, directory, vec![], [0; 4]);
        }
    } else if complete {
        let held = HeldControls {
            tap: true,
            ..HeldControls::default()
        };
        let tap_circuit = vec![
            CordConnection {
                first: PortId::Subscriber(caller),
                second: PortId::Tap(1),
            },
            CordConnection {
                first: PortId::Subscriber(callee),
                second: PortId::Tap(2),
            },
        ];
        send_with_controls(backend, sequence, directory, tap_circuit, [0; 4], held);
    }
    if complete && action != Some(exchange_backend::story::OperatorTextAction::Tap) {
        if backend.frontend_state().call.is_some() {
            send(backend, sequence, directory, vec![], [0; 4]);
        }
    }
    advance_automatic_story(backend);
    if complete
        && backend.frontend_state().call.is_none()
        && matches!(
            backend
                .story_graph()
                .node(backend.story_node_id())
                .map(|node| &node.kind),
            Some(exchange_backend::story::StoryNodeKind::ShiftCall { .. })
        )
    {
        frontend_tick(backend, sequence, false, None, None, 0, None);
    }
}

fn frontend_tick_safely(
    backend: &mut Backend,
    sequence: &mut u64,
    complete: bool,
    action: Option<exchange_backend::story::OperatorTextAction>,
    caller: Option<u8>,
    directory: u16,
    callee: Option<u8>,
) -> bool {
    catch_unwind(AssertUnwindSafe(|| {
        frontend_tick(
            backend, sequence, complete, action, caller, directory, callee,
        )
    }))
    .is_ok()
}

fn bool_field(value: &serde_json::Value, name: &str) -> Option<bool> {
    value.get(name).and_then(serde_json::Value::as_bool)
}

fn service_report(value: Option<&serde_json::Value>) -> Option<OperatorServiceReport> {
    let value = value?;
    Some(OperatorServiceReport {
        location: value
            .get("location")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        medical_emergency: bool_field(value, "medical_emergency"),
        identity: bool_field(value, "identity"),
        target_addresses: bool_field(value, "target_addresses"),
        report_phrase: bool_field(value, "report_phrase"),
        alias: bool_field(value, "alias"),
        source_line: bool_field(value, "source_line"),
        verification_code: bool_field(value, "verification_code"),
        employer: bool_field(value, "employer"),
        false_clinic: bool_field(value, "false_clinic"),
        product_claim: bool_field(value, "product_claim"),
        payment_request: bool_field(value, "payment_request"),
    })
}

fn main() {
    let stdin = io::stdin();
    let mut backend = Backend::new_north_neeladesh();
    let mut sequence = 1;
    frontend_tick(&mut backend, &mut sequence, false, None, None, 0, None);
    let mut selected_directory = None;
    emit(&backend, true, None, None, None);
    for line in stdin.lock().lines() {
        let Ok(text) = line else { break };
        let payload: serde_json::Value = match serde_json::from_str(&text) {
            Ok(payload) => payload,
            Err(error) => {
                emit(
                    &backend,
                    false,
                    Some(format!("invalid JSON: {error}")),
                    None,
                    None,
                );
                continue;
            }
        };
        let Some(speech) = payload.get("speech").and_then(serde_json::Value::as_str) else {
            emit(
                &backend,
                false,
                Some("speech is required".to_string()),
                None,
                None,
            );
            continue;
        };
        let Some(intent) = payload
            .get("intent")
            .and_then(serde_json::Value::as_str)
            .and_then(operator_action_from_name)
        else {
            emit(
                &backend,
                false,
                Some("intent is unsupported".to_string()),
                None,
                None,
            );
            continue;
        };
        let subscriber_id = payload
            .get("subscriber_id")
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as u16);
        if subscriber_id.is_some_and(|id| id > 9) {
            emit(
                &backend,
                false,
                Some("subscriber_id must be between 1 and 9".to_string()),
                None,
                None,
            );
            continue;
        }
        if subscriber_id.is_some() {
            selected_directory = subscriber_id;
        }
        let callee_line = payload
            .get("callee_line")
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as u8);
        let caller_line = payload
            .get("caller_line")
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as u8);
        if matches!(
            intent,
            exchange_backend::story::OperatorTextAction::DirectoryCheck
        ) && subscriber_id.is_none()
        {
            emit(
                &backend,
                false,
                Some("subscriber_id is required for directory_check".to_string()),
                None,
                None,
            );
            continue;
        }
        if matches!(
            intent,
            exchange_backend::story::OperatorTextAction::Connect
                | exchange_backend::story::OperatorTextAction::Tap
        ) && callee_line.is_none()
        {
            emit(
                &backend,
                false,
                Some("callee_line is required for connect or tap".to_string()),
                None,
                None,
            );
            continue;
        }
        let turn = OperatorTurn {
            speech: speech.to_string(),
            action: intent,
            service_report: service_report(payload.get("service_report")),
        };
        match backend.apply_operator_turn(turn) {
            Ok(action) => {
                if matches!(
                    action,
                    exchange_backend::story::OperatorTextAction::Refuse
                        | exchange_backend::story::OperatorTextAction::Disclose
                        | exchange_backend::story::OperatorTextAction::AcceptPayment
                ) {
                    if let Err(error) = backend.complete_operator_decision(action) {
                        emit(&backend, false, Some(error.to_string()), None, None);
                        continue;
                    }
                    advance_automatic_story(&mut backend);
                    if backend.frontend_state().call.is_none() {
                        frontend_tick(&mut backend, &mut sequence, false, None, None, 0, None);
                    }
                } else if !matches!(
                    action,
                    exchange_backend::story::OperatorTextAction::Ask
                        | exchange_backend::story::OperatorTextAction::DirectoryCheck
                        | exchange_backend::story::OperatorTextAction::Tap
                ) {
                    if intent == exchange_backend::story::OperatorTextAction::DirectoryCheck {
                        selected_directory = subscriber_id;
                    }
                    if !frontend_tick_safely(
                        &mut backend,
                        &mut sequence,
                        true,
                        Some(action),
                        caller_line,
                        selected_directory.unwrap_or(0),
                        callee_line,
                    ) {
                        emit(
                            &backend,
                            false,
                            Some("frontend rejected the physical action".into()),
                            None,
                            None,
                        );
                        continue;
                    }
                    selected_directory = None;
                } else {
                    if intent == exchange_backend::story::OperatorTextAction::DirectoryCheck {
                        selected_directory = subscriber_id;
                    }
                    if !frontend_tick_safely(
                        &mut backend,
                        &mut sequence,
                        false,
                        Some(action),
                        caller_line,
                        selected_directory.unwrap_or(0),
                        callee_line,
                    ) {
                        emit(
                            &backend,
                            false,
                            Some("frontend rejected the physical action".into()),
                            None,
                            None,
                        );
                        continue;
                    }
                }
                emit(
                    &backend,
                    true,
                    None,
                    Some(format!("{action:?}")),
                    Some("directory_select_ring_route"),
                )
            }
            Err(error) => emit(&backend, false, Some(error.to_string()), None, None),
        }
    }
}
