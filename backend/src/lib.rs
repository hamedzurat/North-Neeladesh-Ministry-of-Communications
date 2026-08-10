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
    pub line_lamps: [bool; 16],
    pub directory: Vec<String>,
    pub printer: Vec<String>,
    pub monitor_active: bool,
    pub speaker_active: bool,
}

#[derive(Clone, Copy)]
struct Subscriber {
    id: u16,
    line: usize,
    name: &'static str,
    listing: &'static str,
    role: &'static str,
    note: &'static str,
}

const SUBSCRIBERS: [Subscriber; 4] = [
    Subscriber {
        id: 4101,
        line: 4,
        name: "NILA DAS",
        listing: "FOUNDRY APARTMENTS",
        role: "Foundry Apartments resident",
        note: "Worried, informal, and protective of her household.",
    },
    Subscriber {
        id: 4102,
        line: 1,
        name: "DR. SORIN VALE",
        listing: "KHARAD CLINIC",
        role: "Clinic intake worker",
        note: "Calm and concise; needs the facts to help.",
    },
    Subscriber {
        id: 4103,
        line: 0,
        name: "ARUN MEREK",
        listing: "RAILWAY DISPATCH",
        role: "Railway Dispatch clerk",
        note: "Brisk, procedural, and pressed for time.",
    },
    Subscriber {
        id: 4104,
        line: 11,
        name: "LEELA VOSS",
        listing: "STEEL WORKS",
        role: "Steel Works manager",
        note: "Measured, guarded, and status-conscious.",
    },
];

const OPERATOR_JACK: usize = 16;
const RING_JACK: usize = 17;
const TAP_ONE: (usize, usize) = (18, 19);
const TAP_TWO: (usize, usize) = (20, 21);

#[derive(Debug)]
pub struct MvpCore {
    call_index: usize,
    phase: Phase,
    receipts: Vec<String>,
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
        let ringing_callee = has_pair(&input.cords, callee.line, RING_JACK) && input.crank_complete;
        let direct = has_pair(&input.cords, caller.line, callee.line);
        let tap_action = tapped_bridge(&input.cords, caller.line, callee.line);
        let tapped = tap_action.is_some();

        if input.active_action == 0 {
            self.saw_operator_ptt = true;
        }
        match self.phase {
            Phase::IncomingCaller if connected_to_operator => {
                self.phase = Phase::ConnectedToOperator
            }
            Phase::ConnectedToOperator if self.saw_operator_ptt && input.active_action != 0 => {
                self.phase = Phase::AwaitingRouting
            }
            Phase::AwaitingRouting if ringing_callee => self.phase = Phase::CalleeRinging,
            Phase::CalleeRinging if direct || tapped => {
                self.complete_call(caller, callee, tap_action)
            }
            Phase::CircuitConnected { .. } if !direct && !tapped => {
                self.phase = Phase::IncomingCaller
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

    fn complete_call(&mut self, caller: Subscriber, callee: Subscriber, tap_action: Option<i32>) {
        let tapped = tap_action.is_some();
        let route = if tapped { "TAP BRIDGE" } else { "DIRECT" };
        self.receipts.push(format!(
            "ROUTING RECEIPT: {} → {}",
            caller.listing, callee.listing
        ));
        self.receipts.push(format!("CIRCUIT: {route} — SUCCESS"));
        self.receipts
            .push("--------------------------------".into());
        self.saw_operator_ptt = false;
        self.active_tap_action = tap_action.unwrap_or(-1);
        self.call_index += 1;
        self.phase = if self.call_index == 2 {
            self.receipts
                .push("MVP SUMMARY: BOTH CALLS COMPLETE".into());
            self.receipts.push("ALL FOUR SUBSCRIBERS EXERCISED".into());
            Phase::DemonstrationComplete
        } else {
            Phase::CircuitConnected { tapped }
        };
    }

    fn current_pair(&self) -> (Subscriber, Subscriber) {
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
    match SUBSCRIBERS.iter().find(|subscriber| subscriber.id == id) {
        Some(subscriber) => vec![
            subscriber.name.into(),
            format!("{} · {}", subscriber.id, subscriber.listing),
            subscriber.role.into(),
            subscriber.note.into(),
        ],
        None => vec!["NO RECORD".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            core.apply(snapshot(&[], -1, false)).phase,
            Phase::IncomingCaller
        );
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
