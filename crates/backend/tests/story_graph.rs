use exchange_backend::story::{
    AuthoredContent, Ending, GraphCompileError, LineListing, StoryBeat, StoryCondition,
    StoryEligibilityState, StoryEvent, StoryEventOutcome, StoryNode, StoryNodeKind, Subscriber,
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
    assert_eq!(graph.node("shift_1_call").is_some(), true);
    assert_eq!(graph.node("shift_2_call").is_some(), true);
    assert_eq!(graph.node("shift_3_call").is_some(), true);
    assert_eq!(
        graph.outgoing("final_choice"),
        ["final_taren_call", "final_oren_call", "ending_civil_war",]
    );
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
