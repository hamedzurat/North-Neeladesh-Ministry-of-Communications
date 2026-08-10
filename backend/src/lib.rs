//! Deliberately small, offline authority for the Cabinet Frontend MVP.
//! The newline-delimited JSON protocol is disposable: it exists only to make
//! the Odin/Rust boundary inspectable during the MVP demonstration.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    IncomingCaller,
    ConnectedToOperator,
    AwaitingRouting,
    CalleeRinging,
    CircuitConnected { tapped: bool },
    DemonstrationComplete,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::IncomingCaller => "INCOMING CALLER",
            Self::ConnectedToOperator => "CONNECTED TO OPERATOR",
            Self::AwaitingRouting => "AWAITING ROUTING",
            Self::CalleeRinging => "CALLEE RINGING",
            Self::CircuitConnected { tapped: false } => "DIRECT CIRCUIT CONNECTED",
            Self::CircuitConnected { tapped: true } => "TAP BRIDGE CIRCUIT CONNECTED",
            Self::DemonstrationComplete => "DEMONSTRATION COMPLETE",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cord(pub usize, pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CabinetSnapshot {
    pub sequence: u64,
    pub cords: Vec<Cord>,
    pub active_action: i32,
    pub crank_complete: bool,
    pub directory_id: u16,
    pub speaker_enabled: bool,
    pub reset: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CabinetOutput {
    pub sequence: u64,
    pub phase: Phase,
    pub health: &'static str,
    pub reset_status: &'static str,
    pub routing_status: &'static str,
    pub line_lamps: [bool; 16],
    pub directory: Vec<String>,
    pub printer: Vec<String>,
    pub monitor_active: bool,
    pub speaker_active: bool,
}

/// Typed consequential changes available to the MVP's hardcoded Subscribers.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum SubscriberAction {
    RequestKharadClinicRouting,
    AssessIncomingHouseholdCall,
    RequestSteelWorksRouting,
    ConfirmSteelWorksFreightStatus,
}

/// Authored MVP data owned by the Rust core. The Directory Terminal presents
/// the concise listing fields while dialogue and speech stages consume the
/// remaining profile fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubscriberProfile {
    id: u16,
    line: usize,
    pub identity: &'static str,
    pub line_listing: &'static str,
    pub occupation_or_role: &'static str,
    pub identifying_note: &'static str,
    pub personality: &'static str,
    pub speaking_style: &'static str,
    pub immediate_goal: &'static str,
    pub initial_perspective: &'static str,
    pub paired_relationship: &'static str,
    pub permitted_actions: &'static [SubscriberAction],
    pub local_voice_configuration: &'static str,
}

const SUBSCRIBERS: [SubscriberProfile; 4] = [
    SubscriberProfile {
        id: 4101,
        line: 4,
        identity: "NILA DAS",
        line_listing: "FOUNDRY APARTMENTS",
        occupation_or_role: "Foundry Apartments resident",
        identifying_note: "Keeps a sick household together on a foundry wage.",
        personality: "Worried, informal, impatient under stress, and fiercely protective of her household.",
        speaking_style: "Plainspoken and quick; asks directly when frightened.",
        immediate_goal: "Reach Kharad Clinic to arrange urgent care for her parent.",
        initial_perspective: "The clinic may be the only safe place left for her household tonight.",
        paired_relationship: "Seeking practical help from Dr. Sorin Vale at Kharad Clinic.",
        permitted_actions: &[SubscriberAction::RequestKharadClinicRouting],
        local_voice_configuration: "pocket-tts:nila-low-warm",
    },
    SubscriberProfile {
        id: 4102,
        line: 1,
        identity: "DR. SORIN VALE",
        line_listing: "KHARAD CLINIC",
        occupation_or_role: "Clinic intake worker",
        identifying_note: "Intake desk worker trusted to make scarce clinic time count.",
        personality: "Calm, concise, and empathetic; focused on facts that let the clinic help.",
        speaking_style: "Even-paced, precise questions followed by a brief reassurance.",
        immediate_goal: "Assess Nila Das's household emergency and secure the next safe step.",
        initial_perspective: "Care is scarce, but a clear account can still secure the right response.",
        paired_relationship: "Clinic contact for Nila Das, whose household needs urgent help.",
        permitted_actions: &[SubscriberAction::AssessIncomingHouseholdCall],
        local_voice_configuration: "pocket-tts:sorin-clear-neutral",
    },
    SubscriberProfile {
        id: 4103,
        line: 0,
        identity: "ARUN MEREK",
        line_listing: "RAILWAY DISPATCH",
        occupation_or_role: "Railway Dispatch clerk",
        identifying_note: "Dispatch ledger clerk responsible for an urgent freight interruption.",
        personality: "Brisk, procedural, and time-conscious; hates leaving an operational risk unlogged.",
        speaking_style: "Uses dispatch terms, short clauses, and numbered facts.",
        immediate_goal: "Reach Steel Works to resolve an urgent freight movement problem.",
        initial_perspective: "A missed rail window becomes a citywide delay unless Steel Works decides now.",
        paired_relationship: "Needs a decision from Leela Voss at Steel Works before the rail window closes.",
        permitted_actions: &[SubscriberAction::RequestSteelWorksRouting],
        local_voice_configuration: "pocket-tts:arun-brisk-mid",
    },
    SubscriberProfile {
        id: 4104,
        line: 11,
        identity: "LEELA VOSS",
        line_listing: "STEEL WORKS",
        occupation_or_role: "Steel Works manager",
        identifying_note: "Manager whose plant schedule can disrupt the city's freight plans.",
        personality: "Measured, guarded, and status-conscious; reluctant to disclose more than necessary.",
        speaking_style: "Formal and deliberate, answering only the question she considers necessary.",
        immediate_goal: "Protect Steel Works' schedule while resolving Railway Dispatch's urgent problem.",
        initial_perspective: "The plant's commitments matter, but an unmanaged freight problem could expose her authority.",
        paired_relationship: "The Steel Works decision-maker sought by Arun Merek at Railway Dispatch.",
        permitted_actions: &[SubscriberAction::ConfirmSteelWorksFreightStatus],
        local_voice_configuration: "pocket-tts:leela-measured-low",
    },
];

/// Returns the core-owned profile selected by a four-digit Directory ID.
/// Unknown IDs intentionally resolve to no profile and therefore no record.
pub fn subscriber_profile(id: u16) -> Option<&'static SubscriberProfile> {
    SUBSCRIBERS.iter().find(|subscriber| subscriber.id == id)
}

const OPERATOR_JACK: usize = 16;
const RING_JACK: usize = 17;
const TAP_ONE: (usize, usize) = (18, 19);
const TAP_TWO: (usize, usize) = (20, 21);

#[derive(Debug)]
pub struct MvpCore {
    call_index: usize,
    phase: Phase,
    receipts: Vec<String>,
    routing_status: &'static str,
    saw_operator_ptt: bool,
    active_tap_action: i32,
}

impl Default for MvpCore {
    fn default() -> Self {
        Self::new()
    }
}

impl MvpCore {
    pub fn new() -> Self {
        Self {
            call_index: 0,
            phase: Phase::IncomingCaller,
            receipts: vec![
                "MINISTRY OF COMMUNICATIONS".into(),
                "KHARAD PROVINCIAL EXCHANGE".into(),
                "MVP CORE ONLINE — OFFLINE AUTHORITY READY".into(),
                "--------------------------------".into(),
            ],
            routing_status: "CALLER OFF-HOOK: CONNECT TO OPERATOR",
            saw_operator_ptt: false,
            active_tap_action: -1,
        }
    }

    pub fn apply(&mut self, input: CabinetSnapshot) -> CabinetOutput {
        if input.reset {
            *self = Self::new();
            return self.output(
                input.sequence,
                "RESET COMPLETE",
                input.directory_id,
                false,
                input.speaker_enabled,
            );
        }

        let (caller, callee) = self.current_pair();
        let connected_to_operator = has_pair(&input.cords, caller.line, OPERATOR_JACK);
        let ringing_attempt =
            has_pair(&input.cords, callee.line, RING_JACK) || input.crank_complete;
        let holding_ring_connection =
            input.cords.len() == 1 && has_pair(&input.cords, callee.line, RING_JACK);
        let valid_ringing_callee = holding_ring_connection && input.crank_complete;
        let direct = has_pair(&input.cords, caller.line, callee.line);
        let valid_direct = input.cords.len() == 1 && direct;
        let tap_action = tapped_bridge(&input.cords, caller.line, callee.line);
        let tapped = tap_action.is_some();

        if matches!(self.phase, Phase::AwaitingRouting) && direct {
            self.reject_routing("DIRECT CIRCUIT REJECTED: RING CALLEE FIRST");
            return self.output(
                input.sequence,
                "READY",
                input.directory_id,
                false,
                input.speaker_enabled,
            );
        }

        if input.active_action == 0 {
            self.saw_operator_ptt = true;
        }
        match self.phase {
            Phase::IncomingCaller if connected_to_operator => {
                self.phase = Phase::ConnectedToOperator;
                self.routing_status = "OPERATOR CONNECTED: HOLD PTT";
            }
            Phase::ConnectedToOperator if self.saw_operator_ptt && input.active_action != 0 => {
                self.phase = Phase::AwaitingRouting;
                self.routing_status = "AWAITING ROUTING: RING CALLEE";
            }
            Phase::AwaitingRouting if valid_ringing_callee => {
                self.phase = Phase::CalleeRinging;
                self.routing_status = "CALLEE RINGING: MAKE DIRECT CIRCUIT";
            }
            Phase::AwaitingRouting if ringing_attempt => {
                self.reject_routing("RINGING REJECTED: INVALID CORD TOPOLOGY");
            }
            Phase::CalleeRinging
                if (direct && !valid_direct)
                    || (!input.cords.is_empty()
                        && !holding_ring_connection
                        && !valid_direct
                        && !tapped) =>
            {
                self.reject_routing("DIRECT CIRCUIT REJECTED: INVALID CORD TOPOLOGY");
            }
            Phase::CalleeRinging if valid_direct || tapped => {
                self.complete_call(caller, callee, tap_action)
            }
            Phase::CircuitConnected { .. } if !direct && !tapped => {
                self.phase = Phase::IncomingCaller;
                self.routing_status = "NEXT CALLER OFF-HOOK: CONNECT TO OPERATOR";
            }
            _ => {}
        }

        let monitor_active = matches!(self.phase, Phase::CircuitConnected { tapped: true })
            && input.active_action == self.active_tap_action;
        self.output(
            input.sequence,
            "READY",
            input.directory_id,
            monitor_active,
            input.speaker_enabled,
        )
    }

    fn complete_call(
        &mut self,
        caller: SubscriberProfile,
        callee: SubscriberProfile,
        tap_action: Option<i32>,
    ) {
        let tapped = tap_action.is_some();
        let route = if tapped { "TAP BRIDGE" } else { "DIRECT" };
        self.receipts.push(format!(
            "ROUTING RECEIPT: {} → {}",
            caller.line_listing, callee.line_listing
        ));
        self.receipts.push(format!("CIRCUIT: {route} — SUCCESS"));
        self.receipts
            .push("--------------------------------".into());
        self.saw_operator_ptt = false;
        self.active_tap_action = tap_action.unwrap_or(-1);
        self.call_index += 1;
        self.routing_status = "ROUTING COMPLETE: CLEAR CIRCUIT";
        self.phase = if self.call_index == 2 {
            self.receipts
                .push("MVP SUMMARY: BOTH CALLS COMPLETE".into());
            self.receipts.push("ALL FOUR SUBSCRIBERS EXERCISED".into());
            self.routing_status = "DEMONSTRATION COMPLETE";
            Phase::DemonstrationComplete
        } else {
            Phase::CircuitConnected { tapped }
        };
    }

    fn reject_routing(&mut self, reason: &'static str) {
        if self.routing_status != reason {
            self.receipts.push(reason.into());
        }
        self.routing_status = reason;
    }

    fn current_pair(&self) -> (SubscriberProfile, SubscriberProfile) {
        if self.call_index == 0 {
            (SUBSCRIBERS[0], SUBSCRIBERS[1])
        } else {
            (SUBSCRIBERS[2], SUBSCRIBERS[3])
        }
    }

    fn output(
        &self,
        sequence: u64,
        reset_status: &'static str,
        directory_id: u16,
        monitor_active: bool,
        speaker_active: bool,
    ) -> CabinetOutput {
        let mut lamps = [false; 16];
        if !matches!(self.phase, Phase::DemonstrationComplete) {
            lamps[self.current_pair().0.line] = true;
        }
        CabinetOutput {
            sequence,
            phase: self.phase,
            health: "RUST CORE READY",
            reset_status,
            routing_status: self.routing_status,
            line_lamps: lamps,
            directory: directory_page(directory_id),
            printer: self.receipts.clone(),
            monitor_active,
            speaker_active,
        }
    }
}

fn has_pair(cords: &[Cord], left: usize, right: usize) -> bool {
    cords
        .iter()
        .any(|Cord(a, b)| (*a == left && *b == right) || (*a == right && *b == left))
}

fn tapped_bridge(cords: &[Cord], caller: usize, callee: usize) -> Option<i32> {
    [(TAP_ONE.0, TAP_ONE.1), (TAP_TWO.0, TAP_TWO.1)]
        .into_iter()
        .enumerate()
        .find_map(|(index, (left, right))| {
            ((has_pair(cords, caller, left) && has_pair(cords, callee, right))
                || (has_pair(cords, caller, right) && has_pair(cords, callee, left)))
            .then_some(4 + index as i32)
        })
}

fn directory_page(id: u16) -> Vec<String> {
    match subscriber_profile(id) {
        Some(subscriber) => vec![
            subscriber.identity.into(),
            format!("{} · {}", subscriber.id, subscriber.line_listing),
            subscriber.occupation_or_role.into(),
            subscriber.identifying_note.into(),
        ],
        None => vec!["NO RECORD".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn snapshot(
        cords: &[(usize, usize)],
        active_action: i32,
        crank_complete: bool,
    ) -> CabinetSnapshot {
        CabinetSnapshot {
            sequence: 1,
            cords: cords.iter().map(|&(a, b)| Cord(a, b)).collect(),
            active_action,
            crank_complete,
            directory_id: 4101,
            speaker_enabled: true,
            reset: false,
        }
    }

    #[test]
    fn fixed_call_transitions_from_incoming_to_direct_routing_receipt() {
        let mut core = MvpCore::new();
        let incoming = core.apply(snapshot(&[], -1, false));
        assert_eq!(incoming.phase, Phase::IncomingCaller);
        assert!(incoming.line_lamps[4]);
        assert!(!incoming.line_lamps[1]);
        assert_eq!(
            core.apply(snapshot(&[(4, 16)], -1, false)).phase,
            Phase::ConnectedToOperator
        );
        core.apply(snapshot(&[(4, 16)], 0, false));
        assert_eq!(
            core.apply(snapshot(&[(4, 16)], -1, false)).phase,
            Phase::AwaitingRouting
        );
        assert_eq!(
            core.apply(snapshot(&[(1, 17)], -1, true)).phase,
            Phase::CalleeRinging
        );
        let output = core.apply(snapshot(&[(4, 1)], -1, false));
        assert_eq!(output.phase, Phase::CircuitConnected { tapped: false });
        assert!(
            output
                .printer
                .iter()
                .any(|line| line.contains("FOUNDRY APARTMENTS"))
        );
        assert!(
            output
                .printer
                .iter()
                .any(|line| line == "CIRCUIT: DIRECT — SUCCESS")
        );
    }

    #[test]
    fn direct_circuit_requires_the_callee_to_be_rung_first() {
        let mut core = MvpCore::new();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));

        let output = core.apply(snapshot(&[(4, 1)], -1, false));

        assert_eq!(output.phase, Phase::AwaitingRouting);
        assert_eq!(
            output.routing_status,
            "DIRECT CIRCUIT REJECTED: RING CALLEE FIRST"
        );
        assert!(
            output
                .printer
                .iter()
                .any(|line| line == "DIRECT CIRCUIT REJECTED: RING CALLEE FIRST")
        );
    }

    #[test]
    fn ringing_rejects_extra_or_unrelated_cords() {
        let mut core = MvpCore::new();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));

        let output = core.apply(snapshot(&[(1, 17), (0, 2)], -1, true));

        assert_eq!(output.phase, Phase::AwaitingRouting);
        assert_eq!(
            output.routing_status,
            "RINGING REJECTED: INVALID CORD TOPOLOGY"
        );
        assert!(
            output
                .printer
                .iter()
                .any(|line| line == "RINGING REJECTED: INVALID CORD TOPOLOGY")
        );
    }

    #[test]
    fn direct_circuit_rejects_extra_or_unrelated_cords() {
        let mut core = MvpCore::new();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(1, 17)], -1, true));

        let output = core.apply(snapshot(&[(4, 1), (0, 2)], -1, false));

        assert_eq!(output.phase, Phase::CalleeRinging);
        assert_eq!(
            output.routing_status,
            "DIRECT CIRCUIT REJECTED: INVALID CORD TOPOLOGY"
        );
        assert!(
            output
                .printer
                .iter()
                .any(|line| line == "DIRECT CIRCUIT REJECTED: INVALID CORD TOPOLOGY")
        );
    }

    #[test]
    fn direct_circuit_rejects_the_caller_connected_to_the_wrong_line() {
        let mut core = MvpCore::new();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(1, 17)], -1, true));

        let output = core.apply(snapshot(&[(4, 2)], -1, false));

        assert_eq!(output.phase, Phase::CalleeRinging);
        assert_eq!(
            output.routing_status,
            "DIRECT CIRCUIT REJECTED: INVALID CORD TOPOLOGY"
        );
    }

    #[test]
    fn tap_bridge_requires_both_bridge_ports_and_only_monitors_when_held() {
        let mut core = MvpCore::new();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(1, 17)], -1, true));
        let output = core.apply(snapshot(&[(4, 18), (1, 19)], 4, false));
        assert_eq!(output.phase, Phase::CircuitConnected { tapped: true });
        assert!(output.monitor_active);
        assert!(
            !core
                .apply(snapshot(&[(4, 18), (1, 19)], -1, false))
                .monitor_active
        );
    }

    #[test]
    fn unknown_directory_id_and_reset_are_backend_owned() {
        let mut core = MvpCore::new();
        let mut unknown = snapshot(&[], -1, false);
        unknown.directory_id = 9999;
        assert_eq!(core.apply(unknown).directory, vec!["NO RECORD"]);
        let mut reset = snapshot(&[], -1, false);
        reset.reset = true;
        let output = core.apply(reset);
        assert_eq!(output.reset_status, "RESET COMPLETE");
        assert_eq!(output.phase, Phase::IncomingCaller);
    }

    #[test]
    fn reset_discards_completed_routing_and_is_repeatable() {
        let mut core = MvpCore::new();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(1, 17)], -1, true));
        let completed = core.apply(snapshot(&[(4, 1)], -1, false));
        assert!(
            completed
                .printer
                .iter()
                .any(|line| line.starts_with("ROUTING RECEIPT:"))
        );

        let mut reset = snapshot(&[], -1, false);
        reset.reset = true;
        let first_reset = core.apply(reset.clone());
        let second_reset = core.apply(reset);

        assert_eq!(first_reset.phase, Phase::IncomingCaller);
        assert_eq!(
            first_reset.routing_status,
            "CALLER OFF-HOOK: CONNECT TO OPERATOR"
        );
        assert!(first_reset.line_lamps[4]);
        assert_eq!(first_reset.printer, second_reset.printer);
        assert!(
            !first_reset
                .printer
                .iter()
                .any(|line| line.starts_with("ROUTING RECEIPT:"))
        );
    }

    #[test]
    fn directory_lookup_returns_each_complete_backend_owned_subscriber_profile() {
        let expected_records = [
            (
                4101,
                "NILA DAS",
                "FOUNDRY APARTMENTS",
                "Foundry Apartments resident",
                "pocket-tts:nila-low-warm",
            ),
            (
                4102,
                "DR. SORIN VALE",
                "KHARAD CLINIC",
                "Clinic intake worker",
                "pocket-tts:sorin-clear-neutral",
            ),
            (
                4103,
                "ARUN MEREK",
                "RAILWAY DISPATCH",
                "Railway Dispatch clerk",
                "pocket-tts:arun-brisk-mid",
            ),
            (
                4104,
                "LEELA VOSS",
                "STEEL WORKS",
                "Steel Works manager",
                "pocket-tts:leela-measured-low",
            ),
        ];

        let mut core = MvpCore::new();
        for (id, identity, listing, role, voice) in expected_records {
            let profile = subscriber_profile(id).expect("known directory ID has a profile");
            assert_eq!(profile.identity, identity);
            assert_eq!(profile.line_listing, listing);
            assert_eq!(profile.occupation_or_role, role);
            assert_eq!(profile.local_voice_configuration, voice);
            assert!(!profile.identifying_note.is_empty());
            assert!(!profile.personality.is_empty());
            assert!(!profile.speaking_style.is_empty());
            assert!(!profile.immediate_goal.is_empty());
            assert!(!profile.initial_perspective.is_empty());
            assert!(!profile.paired_relationship.is_empty());
            assert!(!profile.permitted_actions.is_empty());

            let mut input = snapshot(&[], -1, false);
            input.directory_id = id;
            assert_eq!(
                core.apply(input).directory,
                vec![
                    identity.into(),
                    format!("{id} · {listing}"),
                    role.into(),
                    profile.identifying_note.into(),
                ]
            );
        }

        assert!(subscriber_profile(9999).is_none());

        let profiles = [4101, 4102, 4103, 4104]
            .map(|id| subscriber_profile(id).expect("known directory ID has a profile"));
        for field_values in [
            profiles.map(|profile| profile.identity),
            profiles.map(|profile| profile.line_listing),
            profiles.map(|profile| profile.occupation_or_role),
            profiles.map(|profile| profile.identifying_note),
            profiles.map(|profile| profile.personality),
            profiles.map(|profile| profile.speaking_style),
            profiles.map(|profile| profile.immediate_goal),
            profiles.map(|profile| profile.initial_perspective),
            profiles.map(|profile| profile.paired_relationship),
            profiles.map(|profile| profile.local_voice_configuration),
        ] {
            assert_eq!(field_values.into_iter().collect::<HashSet<_>>().len(), 4);
        }
        assert_eq!(
            profiles
                .map(|profile| profile.permitted_actions[0])
                .into_iter()
                .collect::<HashSet<_>>()
                .len(),
            4
        );
    }

    #[test]
    fn clearing_a_completed_circuit_advances_to_the_second_fixed_call() {
        let mut core = MvpCore::new();
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(4, 16)], 0, false));
        core.apply(snapshot(&[(4, 16)], -1, false));
        core.apply(snapshot(&[(1, 17)], -1, true));
        core.apply(snapshot(&[(4, 1)], -1, false));

        let output = core.apply(snapshot(&[], -1, false));

        assert_eq!(output.phase, Phase::IncomingCaller);
        assert!(output.line_lamps[0]);
        assert!(!output.line_lamps[4]);
    }

    #[test]
    fn speaker_control_is_echoed_as_rust_owned_cabinet_output() {
        let mut core = MvpCore::new();
        let mut input = snapshot(&[], -1, false);
        input.speaker_enabled = false;

        assert!(!core.apply(input).speaker_active);
    }
}
