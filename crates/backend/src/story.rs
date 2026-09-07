use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscriber {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineListing {
    pub id: String,
    pub line: u8,
    pub subscriber_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallPremise {
    pub id: String,
    pub caller_id: String,
    pub callee_id: String,
    pub caller_line_id: String,
    pub callee_line_id: String,
    pub directory_ids: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryBeat {
    pub id: String,
    pub call_premise_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryEventOutcome {
    pub id: String,
    pub next_node_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryEvent {
    pub id: String,
    pub outcomes: Vec<StoryEventOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ending {
    pub id: String,
    pub conclusion: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoryNodeKind {
    RunStart {
        next_node_id: String,
    },
    ShiftCall {
        beat_id: String,
        on_success: String,
        on_missed: String,
        on_invalid: String,
    },
    StoryEvent {
        event_id: String,
        default_outcome_id: String,
    },
    Conditional {
        condition: StoryCondition,
        on_met: String,
        on_unmet: String,
    },
    Ending {
        ending_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryNode {
    pub id: String,
    pub kind: StoryNodeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryCondition {
    MaxServiceErrors(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StoryEligibilityState {
    pub service_errors: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredContent {
    pub subscribers: Vec<Subscriber>,
    pub line_listings: Vec<LineListing>,
    pub call_premises: Vec<CallPremise>,
    pub story_beats: Vec<StoryBeat>,
    pub story_events: Vec<StoryEvent>,
    pub endings: Vec<Ending>,
    pub nodes: Vec<StoryNode>,
    pub start_node_id: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GraphCompileError {
    #[error("{kind} has an empty stable ID")]
    EmptyId { kind: &'static str },
    #[error("duplicate stable ID {id} in {kind}")]
    DuplicateId { kind: &'static str, id: String },
    #[error("{kind} references missing {reference_kind} {reference}")]
    MissingReference {
        kind: &'static str,
        reference_kind: &'static str,
        reference: String,
    },
    #[error("line listing {listing_id} has invalid line {line}")]
    InvalidLine { listing_id: String, line: u8 },
    #[error("line listing {listing_id} has no subscriber")]
    EmptyLineListing { listing_id: String },
    #[error("line listings {first_listing_id} and {second_listing_id} share line {line}")]
    DuplicateLine {
        first_listing_id: String,
        second_listing_id: String,
        line: u8,
    },
    #[error("call premise {premise_id} does not assign caller {subscriber_id} to line {line_id}")]
    CallerLineMismatch {
        premise_id: String,
        subscriber_id: String,
        line_id: String,
    },
    #[error("call premise {premise_id} does not assign callee {subscriber_id} to line {line_id}")]
    CalleeLineMismatch {
        premise_id: String,
        subscriber_id: String,
        line_id: String,
    },
    #[error("call premise {premise_id} assigns caller and callee to the same line {line_id}")]
    SameCallLine { premise_id: String, line_id: String },
    #[error("call premise {premise_id} has no Directory selections")]
    EmptyDirectorySelections { premise_id: String },
    #[error("call premise {premise_id} has invalid Directory selection {directory_id}")]
    InvalidDirectorySelection {
        premise_id: String,
        directory_id: u16,
    },
    #[error("story event {event_id} has no outcomes")]
    EmptyEventOutcomes { event_id: String },
    #[error("ending {ending_id} has an empty conclusion")]
    EmptyEnding { ending_id: String },
    #[error("story graph does not contain start node {node_id}")]
    MissingStartNode { node_id: String },
    #[error("story graph contains a cycle at {node_id}")]
    Cycle { node_id: String },
    #[error("required story node {node_id} is unreachable")]
    UnreachableNode { node_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledStoryGraph {
    content: AuthoredContent,
    nodes: BTreeMap<String, StoryNode>,
    outgoing: BTreeMap<String, Vec<String>>,
    call_premises: BTreeMap<String, CallPremise>,
    story_beats: BTreeMap<String, StoryBeat>,
    story_events: BTreeMap<String, StoryEvent>,
    start_node_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryPathSelection {
    pub node_id: String,
    pub used_default: bool,
    pub rejected_proposal: bool,
}

impl AuthoredContent {
    pub fn compile(self) -> Result<CompiledStoryGraph, GraphCompileError> {
        validate_authored_content(&self)?;

        let nodes = self
            .nodes
            .iter()
            .cloned()
            .map(|node| (node.id.clone(), node))
            .collect::<BTreeMap<_, _>>();
        let outgoing = self
            .nodes
            .iter()
            .map(|node| (node.id.clone(), node_outgoing(node, &self.story_events)))
            .collect::<BTreeMap<_, _>>();
        validate_reachability_and_cycles(&nodes, &outgoing, &self.start_node_id)?;

        Ok(CompiledStoryGraph {
            call_premises: self
                .call_premises
                .iter()
                .cloned()
                .map(|premise| (premise.id.clone(), premise))
                .collect(),
            story_beats: self
                .story_beats
                .iter()
                .cloned()
                .map(|beat| (beat.id.clone(), beat))
                .collect(),
            story_events: self
                .story_events
                .iter()
                .cloned()
                .map(|event| (event.id.clone(), event))
                .collect(),
            start_node_id: self.start_node_id.clone(),
            content: self,
            nodes,
            outgoing,
        })
    }

    pub fn demo() -> Self {
        Self {
            subscribers: vec![
                Subscriber {
                    id: "taren_kesh".to_string(),
                    name: "Taren Kesh".to_string(),
                },
                Subscriber {
                    id: "vira_dhal".to_string(),
                    name: "Vira Dhal".to_string(),
                },
                Subscriber {
                    id: "leyla_varan".to_string(),
                    name: "Leyla Varan".to_string(),
                },
                Subscriber {
                    id: "oren_vey".to_string(),
                    name: "Oren Vey".to_string(),
                },
            ],
            line_listings: vec![
                LineListing {
                    id: "railway_dispatch_office".to_string(),
                    line: 0,
                    subscriber_ids: vec!["taren_kesh".to_string()],
                },
                LineListing {
                    id: "factory_records_office".to_string(),
                    line: 1,
                    subscriber_ids: vec!["vira_dhal".to_string()],
                },
                LineListing {
                    id: "clinic".to_string(),
                    line: 2,
                    subscriber_ids: vec!["leyla_varan".to_string()],
                },
                LineListing {
                    id: "border_post".to_string(),
                    line: 3,
                    subscriber_ids: vec!["oren_vey".to_string()],
                },
            ],
            call_premises: vec![CallPremise {
                id: "dispatch_request".to_string(),
                caller_id: "taren_kesh".to_string(),
                callee_id: "vira_dhal".to_string(),
                caller_line_id: "railway_dispatch_office".to_string(),
                callee_line_id: "factory_records_office".to_string(),
                directory_ids: vec![1, 2],
            },
            CallPremise {
                id: "competing_service_request".to_string(),
                caller_id: "leyla_varan".to_string(),
                callee_id: "oren_vey".to_string(),
                caller_line_id: "clinic".to_string(),
                callee_line_id: "border_post".to_string(),
                directory_ids: vec![2],
            }],
            story_beats: vec![StoryBeat {
                id: "railway_dispatch".to_string(),
                call_premise_id: "dispatch_request".to_string(),
            }],
            story_events: vec![
                StoryEvent {
                    id: "routing_success".to_string(),
                    outcomes: vec![StoryEventOutcome {
                        id: "success".to_string(),
                        next_node_id: "service_gate".to_string(),
                    }],
                },
                StoryEvent {
                    id: "routing_missed".to_string(),
                    outcomes: vec![StoryEventOutcome {
                        id: "missed".to_string(),
                        next_node_id: "ending_missed".to_string(),
                    }],
                },
                StoryEvent {
                    id: "routing_invalid".to_string(),
                    outcomes: vec![StoryEventOutcome {
                        id: "invalid".to_string(),
                        next_node_id: "ending_invalid".to_string(),
                    }],
                },
            ],
            endings: vec![
                Ending {
                    id: "successful_dispatch".to_string(),
                    conclusion: "The railway receives the dispatch in time.".to_string(),
                },
                Ending {
                    id: "missed_dispatch".to_string(),
                    conclusion: "The waiting dispatch expires unanswered.".to_string(),
                },
                Ending {
                    id: "invalid_dispatch".to_string(),
                    conclusion: "The exchange records an invalid routing.".to_string(),
                },
                Ending {
                    id: "service_error_dispatch".to_string(),
                    conclusion: "The railway receives the dispatch, but the exchange records a service error.".to_string(),
                },
            ],
            nodes: vec![
                StoryNode {
                    id: "run_start".to_string(),
                    kind: StoryNodeKind::RunStart {
                        next_node_id: "shift_call".to_string(),
                    },
                },
                StoryNode {
                    id: "shift_call".to_string(),
                    kind: StoryNodeKind::ShiftCall {
                        beat_id: "railway_dispatch".to_string(),
                        on_success: "event_success".to_string(),
                        on_missed: "event_missed".to_string(),
                        on_invalid: "event_invalid".to_string(),
                    },
                },
                StoryNode {
                    id: "event_success".to_string(),
                    kind: StoryNodeKind::StoryEvent {
                        event_id: "routing_success".to_string(),
                        default_outcome_id: "success".to_string(),
                    },
                },
                StoryNode {
                    id: "service_gate".to_string(),
                    kind: StoryNodeKind::Conditional {
                        condition: StoryCondition::MaxServiceErrors(0),
                        on_met: "ending_success".to_string(),
                        on_unmet: "ending_service_error".to_string(),
                    },
                },
                StoryNode {
                    id: "event_missed".to_string(),
                    kind: StoryNodeKind::StoryEvent {
                        event_id: "routing_missed".to_string(),
                        default_outcome_id: "missed".to_string(),
                    },
                },
                StoryNode {
                    id: "event_invalid".to_string(),
                    kind: StoryNodeKind::StoryEvent {
                        event_id: "routing_invalid".to_string(),
                        default_outcome_id: "invalid".to_string(),
                    },
                },
                StoryNode {
                    id: "ending_success".to_string(),
                    kind: StoryNodeKind::Ending {
                        ending_id: "successful_dispatch".to_string(),
                    },
                },
                StoryNode {
                    id: "ending_missed".to_string(),
                    kind: StoryNodeKind::Ending {
                        ending_id: "missed_dispatch".to_string(),
                    },
                },
                StoryNode {
                    id: "ending_invalid".to_string(),
                    kind: StoryNodeKind::Ending {
                        ending_id: "invalid_dispatch".to_string(),
                    },
                },
                StoryNode {
                    id: "ending_service_error".to_string(),
                    kind: StoryNodeKind::Ending {
                        ending_id: "service_error_dispatch".to_string(),
                    },
                },
            ],
            start_node_id: "run_start".to_string(),
        }
    }

    pub fn four_shift_demo() -> Self {
        let mut content = Self::demo();
        let leya = content
            .subscribers
            .iter_mut()
            .find(|subscriber| subscriber.id == "leyla_varan")
            .expect("demo fixture includes the clinic Subscriber");
        leya.id = "leya_varan".to_string();
        leya.name = "Dr. Leya Varan".to_string();
        content
            .subscribers
            .iter_mut()
            .find(|subscriber| subscriber.id == "oren_vey")
            .expect("demo fixture includes the border post Subscriber")
            .name = "Captain Oren Vey".to_string();
        for listing in &mut content.line_listings {
            for subscriber_id in &mut listing.subscriber_ids {
                if subscriber_id == "leyla_varan" {
                    *subscriber_id = "leya_varan".to_string();
                }
            }
        }
        content.subscribers.push(Subscriber {
            id: "neri_tal".to_string(),
            name: "Neri Tal".to_string(),
        });
        content.line_listings.push(LineListing {
            id: "signal_box".to_string(),
            line: 4,
            subscriber_ids: vec!["neri_tal".to_string()],
        });
        content.call_premises = vec![
            CallPremise {
                id: "relief_train_request".to_string(),
                caller_id: "taren_kesh".to_string(),
                callee_id: "vira_dhal".to_string(),
                caller_line_id: "railway_dispatch_office".to_string(),
                callee_line_id: "factory_records_office".to_string(),
                directory_ids: vec![1, 2],
            },
            CallPremise {
                id: "parent_collapse_request".to_string(),
                caller_id: "leya_varan".to_string(),
                callee_id: "oren_vey".to_string(),
                caller_line_id: "clinic".to_string(),
                callee_line_id: "border_post".to_string(),
                directory_ids: vec![4],
            },
            CallPremise {
                id: "signal_intercept_request".to_string(),
                caller_id: "neri_tal".to_string(),
                callee_id: "vira_dhal".to_string(),
                caller_line_id: "signal_box".to_string(),
                callee_line_id: "factory_records_office".to_string(),
                directory_ids: vec![2],
            },
            CallPremise {
                id: "factory_response_request".to_string(),
                caller_id: "vira_dhal".to_string(),
                callee_id: "taren_kesh".to_string(),
                caller_line_id: "factory_records_office".to_string(),
                callee_line_id: "railway_dispatch_office".to_string(),
                directory_ids: vec![1],
            },
            CallPremise {
                id: "final_taren_request".to_string(),
                caller_id: "taren_kesh".to_string(),
                callee_id: "vira_dhal".to_string(),
                caller_line_id: "railway_dispatch_office".to_string(),
                callee_line_id: "factory_records_office".to_string(),
                directory_ids: vec![2],
            },
            CallPremise {
                id: "final_oren_request".to_string(),
                caller_id: "oren_vey".to_string(),
                callee_id: "vira_dhal".to_string(),
                caller_line_id: "border_post".to_string(),
                callee_line_id: "factory_records_office".to_string(),
                directory_ids: vec![4],
            },
        ];
        content.story_beats = vec![
            StoryBeat {
                id: "relief_train_7".to_string(),
                call_premise_id: "relief_train_request".to_string(),
            },
            StoryBeat {
                id: "parent_collapse".to_string(),
                call_premise_id: "parent_collapse_request".to_string(),
            },
            StoryBeat {
                id: "intercepted_signal".to_string(),
                call_premise_id: "signal_intercept_request".to_string(),
            },
            StoryBeat {
                id: "final_taren_routing".to_string(),
                call_premise_id: "final_taren_request".to_string(),
            },
            StoryBeat {
                id: "final_oren_routing".to_string(),
                call_premise_id: "final_oren_request".to_string(),
            },
        ];
        content.story_events = vec![
            authored_event("relief_train_success", "shift_2_call"),
            authored_event("relief_train_missed", "shift_2_call"),
            authored_event("relief_train_invalid", "shift_2_call"),
            authored_event("parent_collapse_success", "shift_3_call"),
            authored_event("parent_collapse_missed", "shift_3_call"),
            authored_event("parent_collapse_invalid", "shift_3_call"),
            authored_event("intercepted_signal_success", "final_choice"),
            authored_event("intercepted_signal_missed", "final_choice"),
            authored_event("intercepted_signal_invalid", "final_choice"),
            StoryEvent {
                id: "final_choice_event".to_string(),
                outcomes: vec![
                    StoryEventOutcome {
                        id: "taren".to_string(),
                        next_node_id: "final_taren_call".to_string(),
                    },
                    StoryEventOutcome {
                        id: "oren".to_string(),
                        next_node_id: "final_oren_call".to_string(),
                    },
                    StoryEventOutcome {
                        id: "standoff".to_string(),
                        next_node_id: "ending_civil_war".to_string(),
                    },
                ],
            },
            authored_event("final_taren_success", "final_taren_gate"),
            authored_event("final_taren_missed", "ending_civil_war"),
            authored_event("final_taren_invalid", "ending_civil_war"),
            authored_event("final_oren_success", "final_oren_gate"),
            authored_event("final_oren_missed", "ending_civil_war"),
            authored_event("final_oren_invalid", "ending_civil_war"),
        ];
        content.endings = vec![
            Ending {
                id: "trade_detente".to_string(),
                conclusion: "Trade Détente: Relief Train 7 crosses the Partition and the exchange keeps the corridor open.".to_string(),
            },
            Ending {
                id: "managed_emergency_rule".to_string(),
                conclusion: "Managed Emergency Rule: the emergency holds, but the Ministry keeps the exchange under emergency control.".to_string(),
            },
            Ending {
                id: "renewed_civil_war".to_string(),
                conclusion: "Renewed Civil War: the final exchange fails and the border stations return to open conflict.".to_string(),
            },
        ];
        content.nodes = vec![
            StoryNode {
                id: "run_start".to_string(),
                kind: StoryNodeKind::RunStart {
                    next_node_id: "shift_1_call".to_string(),
                },
            },
            shift_call_node(
                "shift_1_call",
                "relief_train_7",
                "relief_train_success_event",
                "relief_train_missed_event",
                "relief_train_invalid_event",
            ),
            event_node("relief_train_success_event", "relief_train_success"),
            event_node("relief_train_missed_event", "relief_train_missed"),
            event_node("relief_train_invalid_event", "relief_train_invalid"),
            shift_call_node(
                "shift_2_call",
                "parent_collapse",
                "parent_collapse_success_event",
                "parent_collapse_missed_event",
                "parent_collapse_invalid_event",
            ),
            event_node("parent_collapse_success_event", "parent_collapse_success"),
            event_node("parent_collapse_missed_event", "parent_collapse_missed"),
            event_node("parent_collapse_invalid_event", "parent_collapse_invalid"),
            shift_call_node(
                "shift_3_call",
                "intercepted_signal",
                "intercepted_signal_success_event",
                "intercepted_signal_missed_event",
                "intercepted_signal_invalid_event",
            ),
            event_node(
                "intercepted_signal_success_event",
                "intercepted_signal_success",
            ),
            event_node(
                "intercepted_signal_missed_event",
                "intercepted_signal_missed",
            ),
            event_node(
                "intercepted_signal_invalid_event",
                "intercepted_signal_invalid",
            ),
            StoryNode {
                id: "final_choice".to_string(),
                kind: StoryNodeKind::StoryEvent {
                    event_id: "final_choice_event".to_string(),
                    default_outcome_id: "taren".to_string(),
                },
            },
            shift_call_node(
                "final_taren_call",
                "final_taren_routing",
                "final_taren_success_event",
                "final_taren_missed_event",
                "final_taren_invalid_event",
            ),
            event_node("final_taren_success_event", "final_taren_success"),
            event_node("final_taren_missed_event", "final_taren_missed"),
            event_node("final_taren_invalid_event", "final_taren_invalid"),
            StoryNode {
                id: "final_taren_gate".to_string(),
                kind: StoryNodeKind::Conditional {
                    condition: StoryCondition::MaxServiceErrors(0),
                    on_met: "ending_trade_detente".to_string(),
                    on_unmet: "ending_managed_emergency_rule".to_string(),
                },
            },
            shift_call_node(
                "final_oren_call",
                "final_oren_routing",
                "final_oren_success_event",
                "final_oren_missed_event",
                "final_oren_invalid_event",
            ),
            event_node("final_oren_success_event", "final_oren_success"),
            event_node("final_oren_missed_event", "final_oren_missed"),
            event_node("final_oren_invalid_event", "final_oren_invalid"),
            StoryNode {
                id: "final_oren_gate".to_string(),
                kind: StoryNodeKind::Conditional {
                    condition: StoryCondition::MaxServiceErrors(0),
                    on_met: "ending_managed_emergency_rule".to_string(),
                    on_unmet: "ending_civil_war".to_string(),
                },
            },
            StoryNode {
                id: "ending_trade_detente".to_string(),
                kind: StoryNodeKind::Ending {
                    ending_id: "trade_detente".to_string(),
                },
            },
            StoryNode {
                id: "ending_managed_emergency_rule".to_string(),
                kind: StoryNodeKind::Ending {
                    ending_id: "managed_emergency_rule".to_string(),
                },
            },
            StoryNode {
                id: "ending_civil_war".to_string(),
                kind: StoryNodeKind::Ending {
                    ending_id: "renewed_civil_war".to_string(),
                },
            },
        ];
        content.start_node_id = "run_start".to_string();
        content
    }
}

fn authored_event(id: &str, next_node_id: &str) -> StoryEvent {
    StoryEvent {
        id: id.to_string(),
        outcomes: vec![StoryEventOutcome {
            id: "default".to_string(),
            next_node_id: next_node_id.to_string(),
        }],
    }
}

fn event_node(id: &str, event_id: &str) -> StoryNode {
    StoryNode {
        id: id.to_string(),
        kind: StoryNodeKind::StoryEvent {
            event_id: event_id.to_string(),
            default_outcome_id: "default".to_string(),
        },
    }
}

fn shift_call_node(
    id: &str,
    beat_id: &str,
    on_success: &str,
    on_missed: &str,
    on_invalid: &str,
) -> StoryNode {
    StoryNode {
        id: id.to_string(),
        kind: StoryNodeKind::ShiftCall {
            beat_id: beat_id.to_string(),
            on_success: on_success.to_string(),
            on_missed: on_missed.to_string(),
            on_invalid: on_invalid.to_string(),
        },
    }
}

impl CompiledStoryGraph {
    pub fn start_node_id(&self) -> &str {
        &self.start_node_id
    }

    pub fn node(&self, node_id: &str) -> Option<&StoryNode> {
        self.nodes.get(node_id)
    }

    pub fn subscribers(&self) -> &[Subscriber] {
        &self.content.subscribers
    }

    pub fn story_event_node_id(&self, event_id: &str) -> Option<&str> {
        self.nodes.values().find_map(|node| {
            matches!(
                &node.kind,
                StoryNodeKind::StoryEvent { event_id: node_event_id, .. }
                    if node_event_id == event_id
            )
            .then_some(node.id.as_str())
        })
    }

    pub fn outgoing(&self, node_id: &str) -> &[String] {
        self.outgoing.get(node_id).map_or(&[], Vec::as_slice)
    }

    pub fn call_premise(&self, premise_id: &str) -> Option<&CallPremise> {
        self.call_premises.get(premise_id)
    }

    pub fn story_beat(&self, beat_id: &str) -> Option<&StoryBeat> {
        self.story_beats.get(beat_id)
    }

    pub fn story_event(&self, event_id: &str) -> Option<&StoryEvent> {
        self.story_events.get(event_id)
    }

    pub fn line_listing(&self, listing_id: &str) -> Option<&LineListing> {
        self.content
            .line_listings
            .iter()
            .find(|listing| listing.id == listing_id)
    }

    pub fn content_line_for_subscriber(&self, subscriber_id: &str) -> Option<&LineListing> {
        self.content
            .line_listings
            .iter()
            .find(|listing| listing.subscriber_ids.iter().any(|id| id == subscriber_id))
    }

    pub fn authored_call_for_caller_line(&self, caller_line: u8) -> Option<(u8, u8)> {
        self.call_premises.values().find_map(|premise| {
            let caller = self.line_listing(&premise.caller_line_id)?.line;
            let callee = self.line_listing(&premise.callee_line_id)?.line;
            (caller == caller_line).then_some((caller, callee))
        })
    }

    pub fn ending(&self, ending_id: &str) -> Option<&Ending> {
        self.content
            .endings
            .iter()
            .find(|ending| ending.id == ending_id)
    }

    pub fn select_next(&self, node_id: &str, proposal: Option<&str>) -> StoryPathSelection {
        self.select_next_with_state(node_id, proposal, &StoryEligibilityState::default())
    }

    pub fn select_next_with_state(
        &self,
        node_id: &str,
        proposal: Option<&str>,
        state: &StoryEligibilityState,
    ) -> StoryPathSelection {
        if let Some(StoryNodeKind::Conditional {
            condition,
            on_met,
            on_unmet,
        }) = self.node(node_id).map(|node| &node.kind)
        {
            let eligible = match condition {
                StoryCondition::MaxServiceErrors(maximum) if state.service_errors <= *maximum => {
                    on_met
                }
                StoryCondition::MaxServiceErrors(_) => on_unmet,
            };
            return StoryPathSelection {
                node_id: eligible.clone(),
                used_default: proposal != Some(eligible.as_str()),
                rejected_proposal: proposal.is_some_and(|proposal| proposal != eligible),
            };
        }
        let outgoing = self.outgoing(node_id);
        if matches!(
            self.node(node_id).map(|node| &node.kind),
            Some(StoryNodeKind::ShiftCall { .. })
        ) {
            return StoryPathSelection {
                node_id: node_id.to_string(),
                used_default: false,
                rejected_proposal: proposal.is_some(),
            };
        }
        if outgoing.is_empty() {
            return StoryPathSelection {
                node_id: node_id.to_string(),
                used_default: true,
                rejected_proposal: proposal.is_some(),
            };
        }
        let default = outgoing
            .first()
            .expect("compiled graph nodes always have an edge");
        match proposal.filter(|proposal| outgoing.iter().any(|node| node == *proposal)) {
            Some(node_id) => StoryPathSelection {
                node_id: node_id.to_string(),
                used_default: false,
                rejected_proposal: false,
            },
            None => StoryPathSelection {
                node_id: default.clone(),
                used_default: true,
                rejected_proposal: proposal.is_some(),
            },
        }
    }
}

fn validate_authored_content(content: &AuthoredContent) -> Result<(), GraphCompileError> {
    validate_ids(
        "Subscriber",
        content.subscribers.iter().map(|value| value.id.as_str()),
    )?;
    validate_ids(
        "LineListing",
        content.line_listings.iter().map(|value| value.id.as_str()),
    )?;
    validate_ids(
        "CallPremise",
        content.call_premises.iter().map(|value| value.id.as_str()),
    )?;
    validate_ids(
        "StoryBeat",
        content.story_beats.iter().map(|value| value.id.as_str()),
    )?;
    validate_ids(
        "StoryEvent",
        content.story_events.iter().map(|value| value.id.as_str()),
    )?;
    validate_ids(
        "Ending",
        content.endings.iter().map(|value| value.id.as_str()),
    )?;
    validate_ids(
        "StoryNode",
        content.nodes.iter().map(|value| value.id.as_str()),
    )?;

    let subscribers = content
        .subscribers
        .iter()
        .map(|value| value.id.as_str())
        .collect::<BTreeSet<_>>();
    let listings = content
        .line_listings
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let premises = content
        .call_premises
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let beats = content
        .story_beats
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let events = content
        .story_events
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect::<BTreeMap<_, _>>();
    let endings = content
        .endings
        .iter()
        .map(|value| value.id.as_str())
        .collect::<BTreeSet<_>>();
    let nodes = content
        .nodes
        .iter()
        .map(|value| value.id.as_str())
        .collect::<BTreeSet<_>>();

    for listing in content.line_listings.iter() {
        if listing.line >= 16 {
            return Err(GraphCompileError::InvalidLine {
                listing_id: listing.id.clone(),
                line: listing.line,
            });
        }
        if listing.subscriber_ids.is_empty() {
            return Err(GraphCompileError::EmptyLineListing {
                listing_id: listing.id.clone(),
            });
        }
        for subscriber_id in &listing.subscriber_ids {
            require_id("LineListing", "Subscriber", subscriber_id, &subscribers)?;
        }
    }

    for (index, first_listing) in content.line_listings.iter().enumerate() {
        if let Some(second_listing) = content.line_listings[index + 1..]
            .iter()
            .find(|listing| listing.line == first_listing.line)
        {
            return Err(GraphCompileError::DuplicateLine {
                first_listing_id: first_listing.id.clone(),
                second_listing_id: second_listing.id.clone(),
                line: first_listing.line,
            });
        }
    }

    for premise in content.call_premises.iter() {
        if premise.directory_ids.is_empty() {
            return Err(GraphCompileError::EmptyDirectorySelections {
                premise_id: premise.id.clone(),
            });
        }
        if let Some(directory_id) = premise.directory_ids.iter().find(|id| **id > 9_999) {
            return Err(GraphCompileError::InvalidDirectorySelection {
                premise_id: premise.id.clone(),
                directory_id: *directory_id,
            });
        }
        require_id(
            "CallPremise",
            "Subscriber",
            &premise.caller_id,
            &subscribers,
        )?;
        require_id(
            "CallPremise",
            "Subscriber",
            &premise.callee_id,
            &subscribers,
        )?;
        let caller_line = require_reference(
            "CallPremise",
            "LineListing",
            &premise.caller_line_id,
            &listings,
        )?;
        let callee_line = require_reference(
            "CallPremise",
            "LineListing",
            &premise.callee_line_id,
            &listings,
        )?;
        if premise.caller_line_id == premise.callee_line_id {
            return Err(GraphCompileError::SameCallLine {
                premise_id: premise.id.clone(),
                line_id: premise.caller_line_id.clone(),
            });
        }
        if !caller_line
            .subscriber_ids
            .iter()
            .any(|id| id == &premise.caller_id)
        {
            return Err(GraphCompileError::CallerLineMismatch {
                premise_id: premise.id.clone(),
                subscriber_id: premise.caller_id.clone(),
                line_id: premise.caller_line_id.clone(),
            });
        }
        if !callee_line
            .subscriber_ids
            .iter()
            .any(|id| id == &premise.callee_id)
        {
            return Err(GraphCompileError::CalleeLineMismatch {
                premise_id: premise.id.clone(),
                subscriber_id: premise.callee_id.clone(),
                line_id: premise.callee_line_id.clone(),
            });
        }
    }

    for beat in &content.story_beats {
        require_reference("StoryBeat", "CallPremise", &beat.call_premise_id, &premises)?;
    }
    for event in &content.story_events {
        if event.outcomes.is_empty() {
            return Err(GraphCompileError::EmptyEventOutcomes {
                event_id: event.id.clone(),
            });
        }
        validate_ids(
            "StoryEventOutcome",
            event.outcomes.iter().map(|value| value.id.as_str()),
        )?;
        for outcome in &event.outcomes {
            require_id(
                "StoryEventOutcome",
                "StoryNode",
                &outcome.next_node_id,
                &nodes,
            )?;
        }
    }
    for ending in &content.endings {
        if ending.conclusion.trim().is_empty() {
            return Err(GraphCompileError::EmptyEnding {
                ending_id: ending.id.clone(),
            });
        }
    }

    require_id(
        "AuthoredContent",
        "StoryNode",
        &content.start_node_id,
        &nodes,
    )?;
    for node in &content.nodes {
        match &node.kind {
            StoryNodeKind::RunStart { next_node_id } => {
                require_id("RunStart", "StoryNode", next_node_id, &nodes)?;
            }
            StoryNodeKind::ShiftCall {
                beat_id,
                on_success,
                on_missed,
                on_invalid,
            } => {
                require_reference("ShiftCall", "StoryBeat", beat_id, &beats)?;
                for next_node_id in [on_success, on_missed, on_invalid] {
                    require_id("ShiftCall", "StoryNode", next_node_id, &nodes)?;
                }
            }
            StoryNodeKind::StoryEvent {
                event_id,
                default_outcome_id,
            } => {
                let event = require_reference("StoryEventNode", "StoryEvent", event_id, &events)?;
                require_reference(
                    "StoryEventNode",
                    "StoryEventOutcome",
                    default_outcome_id,
                    &event
                        .outcomes
                        .iter()
                        .map(|outcome| (outcome.id.as_str(), outcome))
                        .collect::<BTreeMap<_, _>>(),
                )?;
            }
            StoryNodeKind::Conditional {
                on_met, on_unmet, ..
            } => {
                require_id("Conditional", "StoryNode", on_met, &nodes)?;
                require_id("Conditional", "StoryNode", on_unmet, &nodes)?;
            }
            StoryNodeKind::Ending { ending_id } => {
                require_id("EndingNode", "Ending", ending_id, &endings)?;
            }
        }
    }

    Ok(())
}

fn validate_ids<'a>(
    kind: &'static str,
    ids: impl Iterator<Item = &'a str>,
) -> Result<(), GraphCompileError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if id.trim().is_empty() {
            return Err(GraphCompileError::EmptyId { kind });
        }
        if !seen.insert(id) {
            return Err(GraphCompileError::DuplicateId {
                kind,
                id: id.to_string(),
            });
        }
    }
    Ok(())
}

fn require_reference<'a, T>(
    kind: &'static str,
    reference_kind: &'static str,
    reference: &str,
    values: &'a BTreeMap<&str, T>,
) -> Result<&'a T, GraphCompileError> {
    values
        .get(reference)
        .ok_or_else(|| GraphCompileError::MissingReference {
            kind,
            reference_kind,
            reference: reference.to_string(),
        })
}

fn require_id(
    kind: &'static str,
    reference_kind: &'static str,
    reference: &str,
    values: &BTreeSet<&str>,
) -> Result<(), GraphCompileError> {
    if values.contains(reference) {
        Ok(())
    } else {
        Err(GraphCompileError::MissingReference {
            kind,
            reference_kind,
            reference: reference.to_string(),
        })
    }
}

fn node_outgoing(node: &StoryNode, events: &[StoryEvent]) -> Vec<String> {
    match &node.kind {
        StoryNodeKind::RunStart { next_node_id } => vec![next_node_id.clone()],
        StoryNodeKind::ShiftCall {
            on_success,
            on_missed,
            on_invalid,
            ..
        } => vec![on_success.clone(), on_missed.clone(), on_invalid.clone()],
        StoryNodeKind::StoryEvent {
            event_id,
            default_outcome_id,
        } => events
            .iter()
            .find(|event| event.id == *event_id)
            .map_or_else(Vec::new, |event| {
                event
                    .outcomes
                    .iter()
                    .find(|outcome| outcome.id == *default_outcome_id)
                    .into_iter()
                    .chain(
                        event
                            .outcomes
                            .iter()
                            .filter(|outcome| outcome.id != *default_outcome_id),
                    )
                    .map(|outcome| outcome.next_node_id.clone())
                    .collect()
            }),
        StoryNodeKind::Conditional {
            on_met, on_unmet, ..
        } => {
            vec![on_met.clone(), on_unmet.clone()]
        }
        StoryNodeKind::Ending { .. } => Vec::new(),
    }
}

fn validate_reachability_and_cycles(
    nodes: &BTreeMap<String, StoryNode>,
    outgoing: &BTreeMap<String, Vec<String>>,
    start_node_id: &str,
) -> Result<(), GraphCompileError> {
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    visit_node(start_node_id, nodes, outgoing, &mut visiting, &mut visited)?;
    if let Some(node_id) = nodes.keys().find(|node_id| !visited.contains(*node_id)) {
        return Err(GraphCompileError::UnreachableNode {
            node_id: node_id.clone(),
        });
    }
    Ok(())
}

fn visit_node(
    node_id: &str,
    nodes: &BTreeMap<String, StoryNode>,
    outgoing: &BTreeMap<String, Vec<String>>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) -> Result<(), GraphCompileError> {
    if visiting.contains(node_id) {
        return Err(GraphCompileError::Cycle {
            node_id: node_id.to_string(),
        });
    }
    if !visited.insert(node_id.to_string()) {
        return Ok(());
    }
    visiting.insert(node_id.to_string());
    for next_node_id in outgoing.get(node_id).into_iter().flatten() {
        if !nodes.contains_key(next_node_id) {
            continue;
        }
        visit_node(next_node_id, nodes, outgoing, visiting, visited)?;
    }
    visiting.remove(node_id);
    Ok(())
}
