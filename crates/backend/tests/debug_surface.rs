use exchange_backend::Backend;
use exchange_protocol::DebugCommand;

#[test]
fn debug_snapshot_exposes_authoritative_run_story_and_diagnostics() {
    let backend = Backend::new();
    let snapshot = backend.debug_snapshot();

    assert_eq!(snapshot.run.number, 1);
    assert_eq!(snapshot.run.state_revision, 0);
    assert_eq!(snapshot.story.current_node_id, "run_start");
    assert_eq!(snapshot.story.frontier, vec!["shift_call"]);
    assert_eq!(snapshot.subscribers.len(), 4);
    assert_eq!(snapshot.subscribers[0].line, Some(0));
    assert_eq!(snapshot.frontend.transport_connected, false);
    assert_eq!(snapshot.recent_errors[0].code, "backend_ready");
}

#[test]
fn debug_controls_use_the_backend_command_boundary_and_reset_cleanly() {
    let mut backend = Backend::new();

    let advanced = backend.apply_debug_command(DebugCommand::AdvanceTime { seconds: 30 });
    assert!(advanced.accepted);
    assert!(advanced.snapshot.run.elapsed_seconds >= 30);

    let injected = backend.apply_debug_command(DebugCommand::InjectCall {
        caller_line: 4,
        callee_line: 5,
    });
    assert!(injected.accepted);
    assert_eq!(injected.snapshot.calls.len(), 1);
    assert_eq!(injected.snapshot.calls[0].caller_line, 4);

    let restricted = backend.apply_debug_command(DebugCommand::InjectCall {
        caller_line: 6,
        callee_line: 7,
    });
    assert!(!restricted.accepted);
    assert_eq!(restricted.error.unwrap().code, "debug_call_restricted");

    let bypass = backend.apply_debug_command(DebugCommand::SetBypassRestrictions { enabled: true });
    assert!(bypass.accepted);
    let second = backend.apply_debug_command(DebugCommand::InjectCall {
        caller_line: 6,
        callee_line: 7,
    });
    assert!(second.accepted);
    assert_eq!(second.snapshot.calls.len(), 2);

    let reset = backend.apply_debug_command(DebugCommand::ResetRun);
    assert!(reset.accepted);
    assert!(reset.snapshot.calls.is_empty());
    assert_eq!(reset.snapshot.run.elapsed_seconds, 0);
    assert!(!reset.snapshot.run.bypass_restrictions);
}

#[test]
fn debug_story_controls_only_use_authored_nodes_and_frontier_unless_bypassed() {
    let mut backend = Backend::new();

    let selected = backend.apply_debug_command(DebugCommand::SelectStoryPath {
        node_id: "shift_call".to_string(),
    });
    assert!(selected.accepted);
    assert_eq!(selected.snapshot.story.current_node_id, "shift_call");
    assert_eq!(
        selected.snapshot.story.current_story_beat.as_deref(),
        Some("railway_dispatch")
    );

    let forced = backend.apply_debug_command(DebugCommand::ForceStoryEvent {
        event_id: "routing_success".to_string(),
    });
    assert!(forced.accepted);
    assert_eq!(forced.snapshot.story.current_node_id, "event_success");

    let rejected = backend.apply_debug_command(DebugCommand::ForceStoryEvent {
        event_id: "not_authored".to_string(),
    });
    assert!(!rejected.accepted);
    assert_eq!(rejected.error.unwrap().code, "unknown_story_event");

    let godmode = backend.apply_debug_command(DebugCommand::SetGodmode { enabled: true });
    assert!(godmode.accepted);
    let forced = backend.apply_debug_command(DebugCommand::ForceStoryEvent {
        event_id: "routing_invalid".to_string(),
    });
    assert!(forced.accepted);
    assert_eq!(forced.snapshot.story.current_node_id, "event_invalid");
}
