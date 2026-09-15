use exchange_backend::story::{
    AuthoredContent, Ending, GraphCompileError, LineListing, OperatorServiceReport,
    OperatorTextAction, OperatorTurn, StoryBeat, StoryCondition, StoryEligibilityState, StoryEvent,
    StoryEventOutcome, StoryNode, StoryNodeKind, Subscriber, operator_action_from_name,
};

fn demo() -> AuthoredContent {
    AuthoredContent::demo()
}

#[test]
fn four_shift_demo_compiles_with_the_authored_cast_and_terminal_choices() {
    let graph = AuthoredContent::four_shift_demo().compile().unwrap();

    assert_eq!(graph.subscribers().len(), 5);
    assert_eq!(
        graph
            .subscribers()
            .iter()
            .map(|subscriber| subscriber.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "Taren Kesh",
            "Vira Dhal",
            "Dr. Leya Varan",
            "Captain Oren Vey",
            "Neri Tal"
        ]
    );
    assert!(graph.node("shift_1_call").is_some());
    assert!(graph.node("shift_2_call").is_some());
    assert!(graph.node("shift_3_call").is_some());
    assert_eq!(
        graph.outgoing("final_choice"),
        ["final_taren_call", "final_oren_call", "ending_civil_war",]
    );
}

#[test]
fn north_neeladesh_catalog_contains_the_authoritative_cast_schedule_and_endings() {
    let content = AuthoredContent::north_neeladesh();
    assert_eq!(content.subscribers.len(), 17);
    assert_eq!(content.line_listings.len(), 12);
    assert_eq!(content.call_premises.len(), 23);
    assert_eq!(content.story_beats.len(), 23);

    let graph = content.compile().unwrap();
    assert_eq!(graph.start_node_id(), "run_start");
    assert!(graph.node("m11a").is_some());
    assert!(graph.node("m11b").is_some());
    assert!(graph.node("m14r").is_some());
    assert!(graph.node("m14a").is_some());
    assert!(graph.node("m14p").is_some());
    assert!(graph.node("m14c").is_some());
    assert!(graph.ending("ending_southbound").is_some());
    for call_id in [
        "s1_1_rafi",
        "m1_anika",
        "s1_2_asha",
        "m2_nayan",
        "m3_rakesh",
        "s2_asha",
        "m4_laleh",
        "s2_1_nahid",
        "m5_javed",
        "m6_varo",
        "m7_tomas",
        "s3_1_nahid",
        "m8_bikram",
        "s3_asha",
        "m9_paro",
        "s3_akash",
        "m10_audit",
        "s4_akash",
        "m11a_laleh",
        "m11b_dev",
        "m12_arman",
        "m13_meera",
        "m14_mira",
    ] {
        let prompt = graph.caller_prompt(call_id).unwrap();
        assert!(!prompt.name.is_empty());
        assert!(!prompt.opening.is_empty());
        assert!(!prompt.reveals.is_empty());
    }
}

#[test]
fn operator_intents_are_bounded_names_not_natural_language_classification() {
    assert_eq!(
        operator_action_from_name("directory_check").unwrap(),
        OperatorTextAction::DirectoryCheck
    );
    assert_eq!(
        operator_action_from_name("call_ems").unwrap(),
        OperatorTextAction::CallEms
    );
    assert!(operator_action_from_name("call_ambulance").is_none());
}

#[test]
fn rafi_ems_report_must_be_complete_and_authored() {
    let mut backend = exchange_backend::Backend::new_north_neeladesh();
    backend.select_story_path(None);
    let incomplete = backend.apply_operator_turn(OperatorTurn {
        speech: "I am sending medical help.".into(),
        action: OperatorTextAction::CallEms,
        service_report: Some(OperatorServiceReport {
            location: Some("shapla_apartments".into()),
            ..OperatorServiceReport::default()
        }),
    });
    assert!(matches!(
        incomplete,
        Err(exchange_backend::story::OperatorTextError::InvalidServiceReport)
    ));

    let complete = backend.apply_operator_turn(OperatorTurn {
        speech: "EMS is on the way.".into(),
        action: OperatorTextAction::CallEms,
        service_report: Some(OperatorServiceReport {
            location: Some("shapla_apartments".into()),
            medical_emergency: Some(true),
            ..OperatorServiceReport::default()
        }),
    });
    assert_eq!(complete.unwrap(), OperatorTextAction::CallEms);
}

#[test]
fn m11a_is_the_laleh_to_central_station_ems_call() {
    let graph = AuthoredContent::north_neeladesh().compile().unwrap();
    let beat = graph.story_beat("m11a_laleh").unwrap();
    let premise = graph.call_premise(&beat.call_premise_id).unwrap();

    assert_eq!(premise.caller_id, "laleh_mir");
    assert_eq!(premise.callee_id, "mira_halek");
    assert_eq!(premise.caller_line_id, "shapla");
    assert_eq!(premise.callee_line_id, "central_station");
}

#[test]
fn invalid_service_report_is_rejected_without_changing_story_state() {
    let mut backend = exchange_backend::Backend::new_north_neeladesh();
    backend.select_story_path(None);
    let before = backend.story_call_id().map(str::to_string);
    let result = backend.apply_operator_turn(OperatorTurn {
        speech: "EMS is on the way.".into(),
        action: OperatorTextAction::CallEms,
        service_report: Some(OperatorServiceReport {
            location: Some("the_market".into()),
            medical_emergency: Some(true),
            ..OperatorServiceReport::default()
        }),
    });
    assert!(result.is_err());
    assert_eq!(backend.story_call_id().map(str::to_string), before);
}

#[test]
fn authored_demo_compiles_into_an_inspectable_story_graph() {
    let graph = demo().compile().unwrap();

    assert_eq!(graph.start_node_id(), "run_start");
    assert!(graph.node("shift_call").is_some());
    assert_eq!(
        graph.outgoing("shift_call"),
        ["event_success", "event_missed", "event_invalid"]
    );
}

#[test]
fn compiler_rejects_cycles() {
    let mut content = demo();
    let node = content
        .nodes
        .iter_mut()
        .find(|node| node.id == "event_success")
        .unwrap();
    node.kind = StoryNodeKind::StoryEvent {
        event_id: "routing_success".to_string(),
        default_outcome_id: "success".to_string(),
    };
    content
        .story_events
        .iter_mut()
        .find(|event| event.id == "routing_success")
        .unwrap()
        .outcomes[0]
        .next_node_id = "shift_call".to_string();

    assert!(matches!(
        content.compile(),
        Err(GraphCompileError::Cycle { .. })
    ));
}

#[test]
fn compiler_rejects_missing_references_and_unreachable_nodes() {
    let mut missing = demo();
    missing.story_beats[0].call_premise_id = "missing_premise".to_string();
    assert!(matches!(
        missing.compile(),
        Err(GraphCompileError::MissingReference { .. })
    ));

    let mut unreachable = demo();
    unreachable.nodes.push(StoryNode {
        id: "unreachable".to_string(),
        kind: StoryNodeKind::Ending {
            ending_id: "unreachable_ending".to_string(),
        },
    });
    unreachable.endings.push(Ending {
        id: "unreachable_ending".to_string(),
        conclusion: "Never reached".to_string(),
    });
    assert!(matches!(
        unreachable.compile(),
        Err(GraphCompileError::UnreachableNode { node_id }) if node_id == "unreachable"
    ));
}

#[test]
fn compiler_rejects_invalid_endings() {
    let mut content = demo();
    content.nodes.retain(|node| node.id != "ending_success");

    assert!(matches!(
        content.compile(),
        Err(GraphCompileError::MissingReference { reference, .. }) if reference == "ending_success"
    ));
}

#[test]
fn compiler_rejects_physically_impossible_call_references() {
    let mut duplicate_lines = demo();
    duplicate_lines.line_listings[1].line = duplicate_lines.line_listings[0].line;
    assert!(matches!(
        duplicate_lines.compile(),
        Err(GraphCompileError::DuplicateLine { .. })
    ));

    let mut same_line = demo();
    same_line.call_premises[0].callee_line_id = "railway_dispatch_office".to_string();
    assert!(matches!(
        same_line.compile(),
        Err(GraphCompileError::SameCallLine { .. })
    ));
}

#[test]
fn invalid_proposals_use_the_authored_default_without_becoming_a_graph_edge() {
    let graph = demo().compile().unwrap();

    let selection = graph.select_next("run_start", Some("ending_success"));

    assert_eq!(selection.node_id, "shift_call");
    assert!(selection.used_default);
    assert!(selection.rejected_proposal);
    assert!(
        !graph
            .outgoing("run_start")
            .iter()
            .any(|node| node == "ending_success")
    );
}

#[test]
fn typed_service_error_counter_selects_the_authored_conditional_path() {
    let graph = demo().compile().unwrap();

    let clean = graph.select_next_with_state(
        "service_gate",
        None,
        &StoryEligibilityState { service_errors: 0 },
    );
    assert_eq!(clean.node_id, "ending_success");

    let failed = graph.select_next_with_state(
        "service_gate",
        None,
        &StoryEligibilityState { service_errors: 1 },
    );
    assert_eq!(failed.node_id, "ending_service_error");

    assert_eq!(
        graph.node("service_gate").unwrap().kind,
        StoryNodeKind::Conditional {
            condition: StoryCondition::MaxServiceErrors(0),
            on_met: "ending_success".to_string(),
            on_unmet: "ending_service_error".to_string(),
        }
    );
}

#[test]
fn demo_definitions_use_stable_typed_references() {
    let content = demo();

    assert_eq!(
        content.subscribers,
        vec![
            Subscriber {
                id: "taren_kesh".to_string(),
                name: "Taren Kesh".to_string()
            },
            Subscriber {
                id: "vira_dhal".to_string(),
                name: "Vira Dhal".to_string()
            },
            Subscriber {
                id: "leyla_varan".to_string(),
                name: "Leyla Varan".to_string()
            },
            Subscriber {
                id: "oren_vey".to_string(),
                name: "Oren Vey".to_string()
            },
        ]
    );
    assert_eq!(
        content.line_listings[0],
        LineListing {
            id: "railway_dispatch_office".to_string(),
            line: 0,
            subscriber_ids: vec!["taren_kesh".to_string()],
        }
    );
    assert_eq!(
        content.story_beats[0],
        StoryBeat {
            id: "railway_dispatch".to_string(),
            call_premise_id: "dispatch_request".to_string(),
        }
    );
    assert_eq!(
        content.story_events[0],
        StoryEvent {
            id: "routing_success".to_string(),
            outcomes: vec![StoryEventOutcome {
                id: "success".to_string(),
                next_node_id: "service_gate".to_string(),
            }],
        }
    );
}
