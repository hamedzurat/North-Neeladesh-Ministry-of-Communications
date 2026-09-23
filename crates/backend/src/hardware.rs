//! Hardware topology and directory helpers live here.
//!
//! This module is intentionally kept separate from story orchestration: adding a
//! story should not require changing the physical cord, ringing, or directory
//! rules.

use exchange_protocol::{
    CallPhase, ClockState, CordConnection, InputState, OutputDebug, PROTOCOL_VERSION, PortId,
    PrinterEntry, ProtocolError, ShiftPhase, ShiftStatus, StateMessage, StateOutput,
    TapBridgeMonitoring, TuningState,
};

use crate::config::{GameConfig, LINES};

pub(crate) fn initial_state(config: &GameConfig) -> StateOutput {
    StateOutput {
        line_lamps: [false; 12],
        game_phase: exchange_protocol::GamePhase::Ready,
        run_generation: 0,
        clock: ClockState {
            shift: 1,
            elapsed_seconds: 0,
        },
        speaker_active: false,
        interference_level: 0,
        tap_bridge_audio_active: false,
        tuning: TuningState::default(),
        directory_pages: directory_pages(config, [0, 0, 0, 0]),
        printer_output: vec![PrinterEntry {
            entry_id: 1,
            text: "SHIFT 1 START // TELEPHONE EXCHANGE READY".into(),
        }],
        call: None,
        calls: vec![],
        service_call: None,
        tap_bridge_monitoring: None,
        shift: ShiftStatus {
            number: 1,
            phase: ShiftPhase::Ready,
            active_call_count: 0,
            completed_routings: 0,
            required_service_calls: 0,
            completed_service_calls: 0,
            service_errors: 0,
            service_error_counts: vec![],
        },
        debug: OutputDebug { messages: vec![] },
        shapla_story_beat: "EmergencyCall".into(),
        neel_story_beat: "ProfessorRouting".into(),
        dirty_work_story_beat: "Instruction".into(),
        dirty_work_completed_contacts: vec![],
        nahid_story_beat: "Scamming".into(),
        nahid_scam_count: 0,
    }
}

pub(crate) fn append_printer(state: &mut StateOutput, text: &str) {
    let id = state
        .printer_output
        .last()
        .map_or(1, |entry| entry.entry_id + 1);
    state.printer_output.push(PrinterEntry {
        entry_id: id,
        text: text.into(),
    });
}

pub(crate) fn rejected(
    sequence: u64,
    code: &str,
    message: &str,
    revision: u64,
    output: &StateOutput,
) -> StateMessage {
    StateMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: sequence,
        accepted: false,
        error: Some(ProtocolError {
            code: code.into(),
            message: message.into(),
        }),
        state_revision: revision,
        output: output.clone(),
    }
}

pub(crate) fn directory_id(digits: [u8; 4]) -> u16 {
    digits
        .into_iter()
        .fold(0, |number, digit| number * 10 + u16::from(digit))
}

pub(crate) fn directory_pages(
    config: &GameConfig,
    digits: [u8; 4],
) -> Vec<exchange_protocol::DirectoryPage> {
    let id = directory_id(digits);
    if let Some(line) = directory_line(config, id) {
        let (name, role, note) = directory_user(config, line);
        vec![exchange_protocol::DirectoryPage {
            page_number: 1,
            heading: simple_place(config, line),
            lines: vec![
                format!("SUBSCRIBER ID {id:04}"),
                format!("SUBSCRIBER // {name}"),
                format!("ROLE // {role}"),
                format!("NOTE // {note}"),
                format!("DESTINATION // {}", simple_place(config, line)),
            ],
        }]
    } else {
        vec![exchange_protocol::DirectoryPage {
            page_number: 1,
            heading: "NO RECORD".into(),
            lines: vec![
                format!("SUBSCRIBER ID {id:04}"),
                "SELECT A LINE FROM 0000 THROUGH 0011".into(),
            ],
        }]
    }
}

pub(crate) fn simple_place(config: &GameConfig, line: u8) -> String {
    config
        .subscribers
        .iter()
        .find(|subscriber| subscriber.line == line)
        .map(|subscriber| subscriber.place.clone())
        .unwrap_or_else(|| format!("LINE {line:02}"))
}

pub(crate) fn directory_user(config: &GameConfig, line: u8) -> (String, String, String) {
    config
        .subscribers
        .iter()
        .find(|subscriber| subscriber.line == line)
        .map(|subscriber| {
            (
                subscriber.name.clone(),
                subscriber.role.clone(),
                subscriber.private_info.clone(),
            )
        })
        .unwrap_or_else(|| {
            (
                format!("SUBSCRIBER {line:02}"),
                "unassigned".into(),
                "No subscriber profile assigned".into(),
            )
        })
}

pub(crate) fn directory_line(config: &GameConfig, id: u16) -> Option<u8> {
    config
        .subscribers
        .iter()
        .find(|subscriber| subscriber.id == id)
        .map(|subscriber| subscriber.line)
}

pub(crate) fn operator_line(cords: &[CordConnection]) -> Option<u8> {
    cords
        .iter()
        .find_map(|cord| match (&cord.first, &cord.second) {
            (PortId::Subscriber(line), PortId::Operator)
            | (PortId::Operator, PortId::Subscriber(line)) => Some(*line),
            _ => None,
        })
}

pub(crate) fn has_cord(input: &InputState, first: PortId, second: PortId) -> bool {
    input.cord_topology.iter().any(|cord| {
        (cord.first == first && cord.second == second)
            || (cord.first == second && cord.second == first)
    })
}

pub(crate) fn valid_ringing_circuit(input: &InputState, caller: u8, callee: u8) -> bool {
    has_cord(input, PortId::Subscriber(caller), PortId::Operator)
        && physical_ring_line(input) == i16::from(callee)
        && input.ring_line == i16::from(callee)
}

pub(crate) fn valid_direct_circuit(input: &InputState, caller: u8, callee: u8) -> bool {
    direct(&input.cord_topology, caller, callee)
        && input.cord_topology.iter().all(|cord| {
            let touches_endpoint = |port: &PortId| {
                matches!(port, PortId::Subscriber(line) if *line == caller || *line == callee)
            };
            if !touches_endpoint(&cord.first) && !touches_endpoint(&cord.second) {
                return true;
            }
            matches!(
                (&cord.first, &cord.second),
                (PortId::Subscriber(a), PortId::Subscriber(b)) if (*a == caller && *b == callee) || (*a == callee && *b == caller)
            ) || matches!((&cord.first, &cord.second),
                (PortId::Subscriber(a), PortId::Tap(_)) | (PortId::Tap(_), PortId::Subscriber(a)) if *a == caller || *a == callee)
        })
}

pub(crate) fn direct(cords: &[CordConnection], caller: u8, callee: u8) -> bool {
    cords.iter().any(|cord| {
        (cord.first == PortId::Subscriber(caller) && cord.second == PortId::Subscriber(callee))
            || (cord.first == PortId::Subscriber(callee)
                && cord.second == PortId::Subscriber(caller))
    })
}

pub(crate) fn has_direct_circuit_for_caller(input: &InputState, caller: u8) -> bool {
    input
        .cord_topology
        .iter()
        .any(|cord| match (&cord.first, &cord.second) {
            (PortId::Subscriber(first), PortId::Subscriber(_)) if *first == caller => true,
            (PortId::Subscriber(_), PortId::Subscriber(second)) if *second == caller => true,
            _ => false,
        })
}

pub(crate) fn exact_cords(input: &[CordConnection], expected: &[(PortId, PortId)]) -> bool {
    input.len() == expected.len()
        && expected.iter().all(|(first, second)| {
            input.iter().any(|cord| {
                (&cord.first == first && &cord.second == second)
                    || (&cord.first == second && &cord.second == first)
            })
        })
}

pub(crate) fn has_wrong_direct_circuit(input: &InputState, caller: u8, callee: u8) -> bool {
    input.cord_topology.iter().any(|cord| {
        let (PortId::Subscriber(first), PortId::Subscriber(second)) = (&cord.first, &cord.second)
        else {
            return false;
        };
        (*first == caller && *second != callee) || (*second == caller && *first != callee)
    })
}

pub(crate) fn lamps(calls: &[crate::calls::ActiveCall], ring_line: i16) -> [bool; 12] {
    let mut result = [false; 12];
    if (0..i16::from(LINES)).contains(&ring_line) {
        result[ring_line as usize] = true;
    }
    for call in calls {
        if !matches!(
            call.phase,
            CallPhase::Missed | CallPhase::Failed | CallPhase::Completed
        ) {
            result[call.caller as usize] = true;
            if matches!(call.phase, CallPhase::Held | CallPhase::Connected)
                || (call.phase == CallPhase::Ringing && call.ring_activated)
            {
                result[call.callee as usize] = true;
            }
        }
    }
    result
}

pub(crate) fn effective_ring_line(input: &InputState) -> i16 {
    let physical_line = physical_ring_line(input);
    (input.ring_line == physical_line)
        .then_some(physical_line)
        .unwrap_or(-1)
}

pub(crate) fn physical_ring_line(input: &InputState) -> i16 {
    input
        .cord_topology
        .iter()
        .find_map(|cord| match (&cord.first, &cord.second) {
            (PortId::RingGenerator, PortId::Subscriber(line))
            | (PortId::Subscriber(line), PortId::RingGenerator) => Some(i16::from(*line)),
            _ => None,
        })
        .unwrap_or(-1)
}

pub(crate) fn tap_monitor(input: &InputState, state: &StateOutput) -> Option<TapBridgeMonitoring> {
    if !input.held_controls.tap {
        return None;
    }
    state.calls.iter().find_map(|call| {
        if call.phase != CallPhase::Connected {
            return None;
        }
        let caller_tap_port =
            if has_cord(input, PortId::Subscriber(call.caller_line), PortId::Tap(1)) {
                1
            } else {
                2
            };
        valid_tap_circuit(input, call.caller_line, call.requested_callee_line).then_some(
            TapBridgeMonitoring {
                caller_line: call.caller_line,
                callee_line: call.requested_callee_line,
                caller_tap_port,
                callee_tap_port: if caller_tap_port == 1 { 2 } else { 1 },
            },
        )
    })
}

pub(crate) fn valid_connected_circuit(input: &InputState, caller: u8, callee: u8) -> bool {
    valid_direct_circuit(input, caller, callee) || valid_tap_circuit(input, caller, callee)
}

pub(crate) fn valid_tap_circuit(input: &InputState, caller: u8, callee: u8) -> bool {
    exact_cords(
        &input.cord_topology,
        &[
            (PortId::Subscriber(caller), PortId::Tap(1)),
            (PortId::Subscriber(callee), PortId::Tap(2)),
        ],
    ) || exact_cords(
        &input.cord_topology,
        &[
            (PortId::Subscriber(caller), PortId::Tap(2)),
            (PortId::Subscriber(callee), PortId::Tap(1)),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::directory_pages;
    use crate::config::GameConfig;

    #[test]
    fn shapla_directory_uses_its_story_place() {
        let config = GameConfig::load();
        let page = &directory_pages(&config, [1, 0, 2, 2])[0];
        assert_eq!(page.heading, "Shapla Apartments");
        assert!(
            page.lines
                .iter()
                .any(|line| line == "DESTINATION // Shapla Apartments")
        );
    }
}
