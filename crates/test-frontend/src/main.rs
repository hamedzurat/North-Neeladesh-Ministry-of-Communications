use std::env;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

use exchange_protocol::{
    CordConnection, DEBUG_PROTOCOL_VERSION, DebugCommand, DebugRequest, DebugResponse,
    HeldControls, InputDebug, InputMessage, InputState, PROTOCOL_VERSION, PortId, StateMessage,
    TEXT_PROTOCOL_VERSION, TextInputMessage, TextResponseMessage, TextStatus, TuningState,
    read_frame, write_frame,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct PlayerPrompt<'a> {
    caller_line: u8,
    destination_line: u8,
    state_revision: u64,
    conversation: &'a [Turn],
    task: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Turn {
    speaker: String,
    text: String,
}

struct GameLog {
    file: std::fs::File,
    entry_number: u64,
}

#[allow(dead_code)]
struct CsvRow<'a> {
    event: &'a str,
    sequence: u64,
    revision: u64,
    caller: u8,
    destination: u8,
    status: &'a str,
    text: &'a str,
}

impl GameLog {
    fn open(path: &str) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        Ok(Self {
            file,
            entry_number: 0,
        })
    }

    fn row(&mut self, row: CsvRow<'_>) -> io::Result<()> {
        let prefix = format!("[{}]", self.entry_number + 1);
        self.entry_number += 1;
        match row.event {
            "state" => writeln!(
                self.file,
                "{} Incoming call: LINE {} -> LINE {} ({}).",
                prefix,
                row.caller,
                row.destination,
                row.text.trim_start_matches("phase=")
            ),
            "opening" => writeln!(self.file, "{} Caller: \"{}\".", prefix, row.text),
            "text_turn" => {
                let parts: Vec<_> = row.text.split(" | ").collect();
                let player = parts.first().unwrap_or(&"").trim_start_matches("player=");
                let classification = parts
                    .iter()
                    .find_map(|part| part.strip_prefix("classification="))
                    .unwrap_or("failure");
                let service = parts
                    .iter()
                    .find_map(|part| part.strip_prefix("service="))
                    .unwrap_or("service");
                let subscriber = parts
                    .iter()
                    .find_map(|part| part.strip_prefix("subscriber="))
                    .unwrap_or("");
                let addressee = if service == "caller" {
                    "caller"
                } else {
                    service
                };
                let classification = (classification != "none")
                    .then_some(format!("; classified as {classification}"))
                    .unwrap_or_default();
                let response = (!subscriber.is_empty())
                    .then_some(format!("; response: \"{subscriber}\""))
                    .unwrap_or_default();
                writeln!(
                    self.file,
                    "{} You say to {}: \"{}\"{}{}.",
                    prefix, addressee, player, classification, response
                )
            }
            "followup" => writeln!(
                self.file,
                "{} Incoming follow-up: LINE {} calls again.",
                prefix, row.caller
            ),
            "npc_response" => writeln!(self.file, "{} Caller: \"{}\".", prefix, row.text),
            _ => writeln!(self.file, "{} {}", prefix, row.text),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend_address = argument("--connect").unwrap_or_else(|| "127.0.0.1:7878".into());
    let text_address = argument("--text-connect").unwrap_or_else(|| "127.0.0.1:7880".into());
    let debug_address = argument("--debug-connect").unwrap_or_else(|| "127.0.0.1:7881".into());
    let log_path = argument("--log").unwrap_or_else(|| "story-test.log".into());
    let path = argument("--path").unwrap_or_else(|| "ems_success".into());
    let player_command = argument("--player-command");

    let mut backend = TcpStream::connect(&backend_address)?;
    let mut text = TcpStream::connect(&text_address)?;
    let mut debug = TcpStream::connect(&debug_address)?;
    for stream in [&backend, &text, &debug] {
        stream.set_read_timeout(Some(Duration::from_secs(120)))?;
        stream.set_write_timeout(Some(Duration::from_secs(120)))?;
    }
    let initial_debug = debug_command(&mut debug, DebugCommand::ResetRun)?;
    if initial_debug.snapshot.shapla_story_beat != "EmergencyCall"
        || initial_debug.snapshot.neel_story_beat != "ProfessorRouting"
        || initial_debug.snapshot.dirty_work_story_beat != "Instruction"
        || initial_debug.snapshot.nahid_story_beat != "Scamming"
    {
        return Err("story reset did not initialize registered story threads".into());
    }
    let initial_money = initial_debug.snapshot.money;
    let mut log = GameLog::open(&log_path)?;
    writeln!(log.file, "=== TEST PATH: {path} ===")?;
    if path == "cross_thread_success" {
        return run_cross_thread_success(
            &mut backend,
            &mut text,
            &mut debug,
            &mut log,
            &player_command,
            initial_money,
        );
    }
    if path == "cross_thread_nahid" {
        return run_cross_thread_nahid(
            &mut backend,
            &mut text,
            &mut debug,
            &mut log,
            &player_command,
        );
    }
    if path.starts_with("neel_") {
        return run_neel_story(
            &mut backend,
            &mut text,
            &mut debug,
            &mut log,
            &player_command,
            path.as_str(),
            initial_money,
        );
    }
    if path.starts_with("dirty_") {
        return run_dirty_work(
            &mut backend,
            &mut text,
            &mut debug,
            &mut log,
            &player_command,
            path.as_str(),
            initial_money,
        );
    }
    if path.starts_with("nahid_") {
        return run_nahid(
            &mut backend,
            &mut text,
            &mut debug,
            &mut log,
            path.as_str(),
            initial_money,
        );
    }
    let mut sequence = 0;
    let mut revision = 0;
    let mut turns = Vec::new();
    let mut state = exchange(
        &mut backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    let call = state
        .output
        .calls
        .first()
        .filter(|call| call.caller_line == 1)
        .or_else(|| state.output.calls.iter().find(|call| call.caller_line == 1))
        .cloned()
        .ok_or("backend returned no call")?;
    log.row(CsvRow {
        event: "state",
        sequence,
        revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status: "accepted",
        text: &format!("phase={:?}", call.phase),
    })?;

    let operator = vec![cord(PortId::Subscriber(call.caller_line), PortId::Operator)];
    state = exchange(
        &mut backend,
        input(
            &mut sequence,
            revision,
            operator.clone(),
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "operator",
        sequence,
        revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status: "accepted",
        text: "caller connected to operator",
    })?;

    log.row(CsvRow {
        event: "opening",
        sequence,
        revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status: "accepted",
        text: "My mother fell down in the bathroom. I don't know what to do.",
    })?;
    log.row(CsvRow {
        event: "ptt_start",
        sequence,
        revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status: "accepted",
        text: "You press PTT.",
    })?;
    let location_question = player_utterance(
        &player_command,
        &call,
        revision,
        &turns,
        "ask the caller for the exact location",
    )?;
    turns.push(Turn {
        speaker: "player".into(),
        text: location_question.clone(),
    });
    let location_response = send_text(
        &mut text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 1,
            state_revision: revision,
            held_controls: HeldControls::default(),
            text: location_question.clone(),
        },
    )?;
    if location_response.status != TextStatus::Completed {
        return Err(format!("location turn failed: {:?}", location_response.error).into());
    }
    let location_answer = location_response.response_text.clone().unwrap_or_default();
    turns.push(Turn {
        speaker: "subscriber".into(),
        text: location_answer.clone(),
    });
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status: "accepted",
        text: &format!(
            "player={location_question} | service=caller | classification=none | subscriber={location_answer}"
        ),
    })?;
    let no_service = matches!(
        path.as_str(),
        "water_no_help" | "unrelated_questions" | "random_conversation"
    );
    if !no_service {
        log.row(CsvRow {
            event: "disconnect",
            sequence,
            revision,
            caller: call.caller_line,
            destination: call.requested_callee_line,
            status: "accepted",
            text: "first conversation complete",
        })?;
    }

    let (service_name, service_controls, service_task, expected_success) = match path.as_str() {
        "ems_success" => (
            "EMS",
            HeldControls {
                ems: true,
                ..Default::default()
            },
            "ask EMS to send medical help to Shapla Apartments; phrase it naturally",
            true,
        ),
        "ems_failure" => (
            "EMS",
            HeldControls {
                ems: true,
                ..Default::default()
            },
            "say something vague that does not clearly request EMS or identify Shapla Apartments",
            false,
        ),
        "police_success" => (
            "Police",
            HeldControls {
                police: true,
                ..Default::default()
            },
            "clearly ask Police to send officers to Shapla Apartments, using a direct request rather than a question; phrase it naturally",
            true,
        ),
        "water_no_help" | "unrelated_questions" | "random_conversation" => {
            ("caller", HeldControls::default(), "", false)
        }
        other => return Err(format!("unknown test path: {other}").into()),
    };
    let utterance = if no_service {
        String::new()
    } else {
        player_utterance(&player_command, &call, revision, &turns, service_task)?
    };
    let text_response = if no_service {
        let turn_tasks = if path == "water_no_help" {
            vec![
                "tell the caller you will get her some water, without contacting EMS or Police"
                    .to_string(),
            ]
        } else if path == "unrelated_questions" {
            vec![
                "ask plainly what the caller's name is, even though this is an emergency".to_string(),
                "ask the caller what she does for work, even though this is an emergency"
                    .to_string(),
                "ask specifically whether the caller has a pet and what it is called, even though this is an emergency".to_string(),
            ]
        } else {
            let mut tasks = vec![
                "ask whether anyone else is with the caller".to_string(),
                "ask whether the caller's mother is conscious".to_string(),
                "tell the caller to stay calm and ask what happened".to_string(),
                "ask whether there is a safe way to reach the bathroom".to_string(),
                "ask whether the caller can hear her mother responding".to_string(),
            ];
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos() as usize);
            let task_count = tasks.len();
            tasks.rotate_left(seed % task_count);
            tasks.truncate(3);
            tasks
        };
        for (offset, task) in turn_tasks.iter().enumerate() {
            log.row(CsvRow {
                event: "ptt_start",
                sequence,
                revision,
                caller: call.caller_line,
                destination: call.requested_callee_line,
                status: "accepted",
                text: "You press PTT.",
            })?;
            let turn_text = player_utterance(&player_command, &call, revision, &turns, task)?;
            let response = send_text(
                &mut text,
                TextInputMessage {
                    protocol_version: TEXT_PROTOCOL_VERSION,
                    session_id: 1,
                    turn_id: 2 + offset as u64,
                    state_revision: revision,
                    held_controls: HeldControls::default(),
                    text: turn_text.clone(),
                },
            )?;
            if response.status != TextStatus::Completed {
                return Err(format!("conversation turn failed: {:?}", response.error).into());
            }
            let answer = response.response_text.clone().unwrap_or_default();
            turns.push(Turn {
                speaker: "player".into(),
                text: turn_text.clone(),
            });
            turns.push(Turn {
                speaker: "subscriber".into(),
                text: answer.clone(),
            });
            log.row(CsvRow {
                event: "text_turn",
                sequence,
                revision,
                caller: call.caller_line,
                destination: call.requested_callee_line,
                status: "accepted",
                text: &format!(
                    "player={turn_text} | service=caller | classification=none | subscriber={answer}"
                ),
            })?;
        }
        TextResponseMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 2,
            state_revision: revision,
            status: TextStatus::Completed,
            classification: None,
            response_text: None,
            error: None,
        }
    } else {
        log.row(CsvRow {
            event: "service_start",
            sequence,
            revision,
            caller: call.caller_line,
            destination: call.requested_callee_line,
            status: "accepted",
            text: &format!("You press {service_name}."),
        })?;
        send_text(
            &mut text,
            TextInputMessage {
                protocol_version: TEXT_PROTOCOL_VERSION,
                session_id: 1,
                turn_id: 2,
                state_revision: revision,
                held_controls: service_controls,
                text: utterance.clone(),
            },
        )?
    };
    let response_text = text_response.response_text.clone().unwrap_or_default();
    if text_response.status == TextStatus::Completed {
        turns.push(Turn {
            speaker: "subscriber".into(),
            text: response_text.clone(),
        });
    }
    if !no_service {
        log.row(CsvRow {
            event: "text_turn",
            sequence,
            revision,
            caller: call.caller_line,
            destination: call.requested_callee_line,
            status: &format!("{:?}", text_response.status),
            text: &format!(
                "player={utterance} | service={service_name} | classification={} | subscriber={response_text}",
                text_response
                    .classification
                    .as_deref()
                    .unwrap_or("not returned")
            ),
        })?;
    }
    if text_response.status != TextStatus::Completed {
        return Err(format!("text turn failed: {:?}", text_response.error).into());
    }

    let classified_success = text_response.classification.as_deref() == Some("success");
    if !no_service && classified_success != expected_success {
        return Err(format!(
            "path {path} expected success={expected_success}, got {:?}",
            text_response.classification
        )
        .into());
    }
    if no_service {
        state = exchange(
            &mut backend,
            input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
        )?;
        revision = state.state_revision;
    }
    let transition_debug = debug_snapshot(&mut debug)?;
    let expected_beat = if no_service {
        "BadFollowup"
    } else if path == "police_success" {
        "NeutralFollowup"
    } else if classified_success {
        "HappyFollowup"
    } else {
        "BadFollowup"
    };
    if transition_debug.snapshot.shapla_story_beat != expected_beat {
        return Err(format!(
            "path {path} expected backend beat {expected_beat}, got {}",
            transition_debug.snapshot.shapla_story_beat
        )
        .into());
    }
    let branch = format!(
        "Backend transition: EmergencyCall -> {}.",
        transition_debug.snapshot.shapla_story_beat
    );
    log.row(CsvRow {
        event: "beat_result",
        sequence,
        revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status: "accepted",
        text: &branch,
    })?;

    if no_service {
        let abandonment_debug = debug_snapshot(&mut debug)?;
        if abandonment_debug.snapshot.shapla_story_beat != "BadFollowup" {
            return Err(format!(
                "path {path} expected abandonment to select BadFollowup, got {}",
                abandonment_debug.snapshot.shapla_story_beat
            )
            .into());
        }
        log.row(CsvRow {
            event: "beat_result",
            sequence,
            revision,
            caller: call.caller_line,
            destination: call.requested_callee_line,
            status: "accepted",
            text: "Backend transition: EmergencyCall -> BadFollowup after abandonment.",
        })?;
    }
    log.row(CsvRow {
        event: "disconnect",
        sequence,
        revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status: "accepted",
        text: "caller disconnected after service request",
    })?;

    if state
        .output
        .calls
        .iter()
        .all(|active| active.caller_line != call.caller_line)
        && debug_snapshot(&mut debug)?.snapshot.shapla_story_beat == "BadFollowup"
    {
        log.row(CsvRow {
            event: "beat_result",
            sequence,
            revision,
            caller: call.caller_line,
            destination: call.requested_callee_line,
            status: "accepted",
            text: "BadFollowup is terminal; no second Shapla call is created.",
        })?;
        return Ok(());
    }

    let followup = state
        .output
        .calls
        .iter()
        .find(|active| active.caller_line == call.caller_line)
        .cloned()
        .ok_or("backend did not create the second story call")?;
    log.row(CsvRow {
        event: "followup",
        sequence,
        revision,
        caller: followup.caller_line,
        destination: followup.requested_callee_line,
        status: "accepted",
        text: "second story call",
    })?;
    state = exchange(
        &mut backend,
        input(
            &mut sequence,
            revision,
            vec![cord(
                PortId::Subscriber(followup.caller_line),
                PortId::Operator,
            )],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "operator",
        sequence,
        revision,
        caller: followup.caller_line,
        destination: followup.requested_callee_line,
        status: "accepted",
        text: "caller connected to operator",
    })?;
    log.row(CsvRow {
        event: "ptt_start",
        sequence,
        revision,
        caller: followup.caller_line,
        destination: followup.requested_callee_line,
        status: "accepted",
        text: "You press PTT.",
    })?;
    let followup_text = player_utterance(
        &player_command,
        &followup,
        revision,
        &turns,
        "ask the caller how the situation turned out",
    )?;
    turns.push(Turn {
        speaker: "player".into(),
        text: followup_text.clone(),
    });
    let followup_response = send_text(
        &mut text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 2,
            state_revision: revision,
            held_controls: HeldControls::default(),
            text: followup_text.clone(),
        },
    )?;
    if followup_response.status != TextStatus::Completed {
        return Err(format!("follow-up turn failed: {:?}", followup_response.error).into());
    }
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: followup.caller_line,
        destination: followup.requested_callee_line,
        status: "accepted",
        text: &format!(
            "player={followup_text} | service=the caller | classification=none | subscriber={}",
            followup_response.response_text.clone().unwrap_or_default()
        ),
    })?;
    state = exchange(
        &mut backend,
        input(
            &mut sequence,
            revision,
            vec![cord(
                PortId::Subscriber(followup.caller_line),
                PortId::Operator,
            )],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    state = exchange(
        &mut backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    let final_debug = debug_snapshot(&mut debug)?;
    let outcome = format!(
        "Backend final money: ${} (started at ${}; delta ${}). Final beat: {}.",
        final_debug.snapshot.money,
        initial_money,
        final_debug.snapshot.money - initial_money,
        final_debug.snapshot.shapla_story_beat,
    );
    for entry in &state.output.printer_output {
        if entry.text.contains("MONEY //") {
            log.row(CsvRow {
                event: "money",
                sequence,
                revision,
                caller: followup.caller_line,
                destination: followup.requested_callee_line,
                status: "accepted",
                text: &entry.text,
            })?;
        }
    }
    log.row(CsvRow {
        event: "outcome",
        sequence,
        revision,
        caller: followup.caller_line,
        destination: followup.requested_callee_line,
        status: "accepted",
        text: &outcome,
    })?;
    let finished = state
        .output
        .calls
        .iter()
        .all(|active| active.caller_line != followup.caller_line);
    let status = if finished { "accepted" } else { "failed" };
    log.row(CsvRow {
        event: "finish",
        sequence,
        revision: state.state_revision,
        caller: followup.caller_line,
        destination: followup.requested_callee_line,
        status,
        text: "call completion checked",
    })?;
    if !finished {
        return Err(format!(
            "backend did not complete the follow-up call: shift_completed={}, active_calls={:?}",
            state.output.shift.completed_routings, state.output.calls
        )
        .into());
    }
    println!("test completed; log written to {log_path}");
    Ok(())
}

fn argument(name: &str) -> Option<String> {
    let mut args = env::args().skip(1);
    while let Some(value) = args.next() {
        if value == name {
            return args.next();
        }
    }
    None
}

fn run_cross_thread_success(
    backend: &mut TcpStream,
    text: &mut TcpStream,
    debug: &mut TcpStream,
    log: &mut GameLog,
    player_command: &Option<String>,
    initial_money: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut sequence = 0;
    let mut revision = 0;
    let mut state = exchange(
        backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    let shapla = state
        .output
        .calls
        .iter()
        .find(|call| call.caller_line == 1)
        .cloned()
        .ok_or("cross-thread mode did not create the Shapla call")?;
    let professor = state
        .output
        .calls
        .iter()
        .find(|call| call.caller_line == 2)
        .cloned()
        .ok_or("cross-thread mode did not create the Neel call")?;
    log.row(CsvRow {
        event: "detail",
        sequence,
        revision,
        caller: 1,
        destination: 0,
        status: "accepted",
        text: "Story state: Shapla EmergencyCall; Neel ProfessorRouting; LINE 1 and LINE 2 are waiting.",
    })?;

    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(1), PortId::Operator)],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "operator",
        sequence,
        revision,
        caller: 1,
        destination: 0,
        status: "accepted",
        text: "You connect Shapla Apartments LINE 1 to the Operator.",
    })?;
    let mut shapla_turns = Vec::new();
    let location = player_utterance(
        player_command,
        &shapla,
        revision,
        &shapla_turns,
        "ask the caller for the exact location",
    )?;
    let location_response = send_text(
        text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 1,
            state_revision: revision,
            held_controls: HeldControls::default(),
            text: location.clone(),
        },
    )?;
    shapla_turns.push(Turn {
        speaker: "player".into(),
        text: location.clone(),
    });
    shapla_turns.push(Turn {
        speaker: "subscriber".into(),
        text: location_response.response_text.clone().unwrap_or_default(),
    });
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: 1,
        destination: 0,
        status: "accepted",
        text: &format!(
            "player={location} | service=caller | classification=none | subscriber={}",
            location_response.response_text.clone().unwrap_or_default()
        ),
    })?;
    let service_text = player_utterance(
        player_command,
        &shapla,
        revision,
        &shapla_turns,
        "ask EMS to send medical help to Shapla Apartments; phrase it naturally",
    )?;
    let service_response = send_text(
        text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 2,
            state_revision: revision,
            held_controls: HeldControls {
                ems: true,
                ..HeldControls::default()
            },
            text: service_text.clone(),
        },
    )?;
    if service_response.classification.as_deref() != Some("success") {
        return Err(format!(
            "cross-thread Shapla service was not successful: {:?}",
            service_response.classification
        )
        .into());
    }
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: 1,
        destination: 0,
        status: "accepted",
        text: &format!(
            "player={service_text} | service=EMS | classification=success | subscriber="
        ),
    })?;
    state = exchange(
        backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "disconnect",
        sequence,
        revision,
        caller: 1,
        destination: 0,
        status: "accepted",
        text: "Shapla service request finished; LINE 1 disconnects.",
    })?;
    let shapla_followup = state
        .output
        .calls
        .iter()
        .find(|call| call.caller_line == 1)
        .cloned()
        .ok_or("Shapla follow-up did not arrive while Neel was active")?;
    log.row(CsvRow {
        event: "beat_result",
        sequence,
        revision,
        caller: 1,
        destination: 0,
        status: "accepted",
        text: "Shapla beat: EmergencyCall -> HappyFollowup.",
    })?;
    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(1), PortId::Operator)],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    let followup_text = player_utterance(
        player_command,
        &shapla_followup,
        revision,
        &shapla_turns,
        "ask the caller how the situation turned out",
    )?;
    let followup_response = send_text(
        text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 3,
            state_revision: revision,
            held_controls: HeldControls::default(),
            text: followup_text.clone(),
        },
    )?;
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: 1,
        destination: 0,
        status: "accepted",
        text: &format!(
            "player={followup_text} | service=caller | classification=none | subscriber={}",
            followup_response.response_text.clone().unwrap_or_default()
        ),
    })?;
    state = exchange(
        backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "disconnect",
        sequence,
        revision,
        caller: 1,
        destination: 0,
        status: "accepted",
        text: "Shapla follow-up disconnects after the thank-you conversation.",
    })?;
    log.row(CsvRow {
        event: "beat_result",
        sequence,
        revision,
        caller: 1,
        destination: 0,
        status: "accepted",
        text: "Shapla follow-up completed.",
    })?;

    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(2), PortId::Operator)],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "operator",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "You connect LINE 2 to the Operator.",
    })?;
    let professor_text = player_utterance(
        player_command,
        &professor,
        revision,
        &[],
        "ask Prof. Kashem where he wants to be connected",
    )?;
    let professor_response = send_text(
        text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 4,
            state_revision: revision,
            held_controls: HeldControls::default(),
            text: professor_text.clone(),
        },
    )?;
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: &format!(
            "player={professor_text} | service=caller | classification=none | subscriber={}",
            professor_response.response_text.clone().unwrap_or_default()
        ),
    })?;
    let ringing = vec![
        cord(PortId::Subscriber(2), PortId::Operator),
        cord(PortId::Subscriber(3), PortId::RingGenerator),
    ];
    state = exchange(
        backend,
        input_with_ring(
            &mut sequence,
            revision,
            ringing.clone(),
            false,
            directory_for_id(1024),
            3,
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "ring_start",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "You connect the Ring Generator to Shadhin Housing; ringing starts.",
    })?;
    let _ = debug_command(debug, DebugCommand::AdvanceTime { seconds: 3 })?;
    state = exchange(
        backend,
        input_with_ring(
            &mut sequence,
            revision,
            ringing,
            false,
            directory_for_id(1024),
            3,
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "ring_ready",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "Shadhin Housing LED is active after the delayed crank.",
    })?;
    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(2), PortId::Subscriber(3))],
            false,
            directory_for_id(1024),
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "route",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "You disconnect LINE 2 from the Operator and connect it directly to LINE 3.",
    })?;
    let professor_audio = debug_snapshot(debug)?
        .snapshot
        .active_calls
        .iter()
        .find(|call| call.caller_line == 2)
        .map(|call| call.audio_duration_seconds)
        .unwrap_or(1);
    let _ = debug_command(
        debug,
        DebugCommand::AdvanceTime {
            seconds: professor_audio.saturating_add(1) as u32,
        },
    )?;
    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(2), PortId::Subscriber(3))],
            false,
            directory_for_id(1024),
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "audio",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: &format!("Professor Kashem's prerecorded audio lasts {professor_audio} seconds."),
    })?;
    let arnab = state
        .output
        .calls
        .iter()
        .find(|call| call.caller_line == 3)
        .cloned()
        .ok_or_else(|| {
            format!(
                "Neel Arnab follow-up did not arrive; backend calls={:?}, snapshot={:?}",
                state.output.calls,
                debug_snapshot(debug).ok()
            )
        })?;
    log.row(CsvRow {
        event: "beat_result",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "Bela Bose beat: ProfessorRouting -> ArnabDirectory.",
    })?;
    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(3), PortId::Operator)],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "operator",
        sequence,
        revision,
        caller: 3,
        destination: 5,
        status: "accepted",
        text: "You connect LINE 3 to the Operator.",
    })?;
    let arnab_text = player_utterance(
        player_command,
        &arnab,
        revision,
        &[],
        "ask Arnab who he wants to be connected to",
    )?;
    let arnab_response = send_text(
        text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 5,
            state_revision: revision,
            held_controls: HeldControls::default(),
            text: arnab_text.clone(),
        },
    )?;
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: 3,
        destination: 5,
        status: "accepted",
        text: &format!(
            "player={arnab_text} | service=caller | classification=none | subscriber={}",
            arnab_response.response_text.clone().unwrap_or_default()
        ),
    })?;
    let directory_text = player_utterance(
        player_command,
        &arnab,
        revision,
        &[],
        "ask Arnab whether he knows Bela Bose's directory number",
    )?;
    let directory_response = send_text(
        text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 6,
            state_revision: revision,
            held_controls: HeldControls::default(),
            text: directory_text.clone(),
        },
    )?;
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: 3,
        destination: 5,
        status: "accepted",
        text: &format!(
            "player={directory_text} | service=caller | classification=none | subscriber={}",
            directory_response.response_text.clone().unwrap_or_default()
        ),
    })?;
    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(3), PortId::Subscriber(5))],
            false,
            [1, 0, 3, 2],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "route",
        sequence,
        revision,
        caller: 3,
        destination: 5,
        status: "accepted",
        text: "You use directory 1032 to connect LINE 3 directly to Bela Bose on LINE 5.",
    })?;
    let bela_audio = debug_snapshot(debug)?
        .snapshot
        .active_calls
        .iter()
        .find(|call| call.caller_line == 3)
        .map(|call| call.audio_duration_seconds)
        .unwrap_or(1);
    let _ = debug_command(
        debug,
        DebugCommand::AdvanceTime {
            seconds: bela_audio.saturating_add(1) as u32,
        },
    )?;
    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(3), PortId::Subscriber(5))],
            false,
            [1, 0, 3, 2],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "audio",
        sequence,
        revision,
        caller: 3,
        destination: 5,
        status: "accepted",
        text: &format!(
            "Bela Bose's prerecorded audio lasted {bela_audio} seconds; LINE 3 and LINE 5 remained active until it ended."
        ),
    })?;
    let snapshot = debug_snapshot(debug)?;
    if snapshot.snapshot.shapla_story_beat != "HappyFollowup"
        || snapshot.snapshot.neel_story_beat != "Completed"
        || snapshot.snapshot.money <= initial_money
        || !state.output.printer_output.iter().any(|entry| {
            entry
                .text
                .contains("+$100 Arnab connected to Bela Bose 1032")
        })
    {
        return Err(format!(
            "cross-thread stories did not complete successfully: snapshot={snapshot:?}"
        )
        .into());
    }
    for entry in &state.output.printer_output {
        if entry.text.contains("MONEY //") {
            log.row(CsvRow {
                event: "money",
                sequence,
                revision,
                caller: 3,
                destination: 5,
                status: "accepted",
                text: &entry.text,
            })?;
        }
    }
    log.row(CsvRow {
        event: "outcome",
        sequence,
        revision,
        caller: 3,
        destination: 5,
        status: "accepted",
        text: &format!(
            "Final money ${} (started at ${}).",
            snapshot.snapshot.money, initial_money
        ),
    })?;
    log.row(CsvRow {
        event: "finish",
        sequence,
        revision,
        caller: 3,
        destination: 5,
        status: "accepted",
        text: "Story paths complete.",
    })?;
    Ok(())
}

fn run_cross_thread_nahid(
    backend: &mut TcpStream,
    text: &mut TcpStream,
    debug: &mut TcpStream,
    log: &mut GameLog,
    player_command: &Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut sequence = 0;
    let mut revision = 0;
    let mut state = exchange(
        backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    for caller in [1, 2, 6, 11] {
        if !state
            .output
            .calls
            .iter()
            .any(|call| call.caller_line == caller)
        {
            return Err(format!("intertwined test did not start LINE {caller}").into());
        }
    }
    let nahid = state
        .output
        .calls
        .iter()
        .find(|call| call.caller_line == 11)
        .cloned()
        .ok_or("intertwined test did not create Nahid's call")?;
    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(11), PortId::Operator)],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "operator",
        sequence,
        revision,
        caller: 11,
        destination: nahid.requested_callee_line,
        status: "accepted",
        text: "You connect Nahid's line to the Operator and question him.",
    })?;
    let mut turns = Vec::new();
    for (turn_id, task) in [
        "ask Nahid to explain who he is and where he is calling from",
        "ask Nahid why a bKash account needs an urgent verification code",
    ]
    .into_iter()
    .enumerate()
    {
        let utterance = player_utterance(player_command, &nahid, revision, &turns, task)?;
        let response = send_text(
            text,
            TextInputMessage {
                protocol_version: TEXT_PROTOCOL_VERSION,
                session_id: 1,
                turn_id: turn_id as u64 + 1,
                state_revision: revision,
                held_controls: HeldControls::default(),
                text: utterance.clone(),
            },
        )?;
        let answer = response.response_text.clone().unwrap_or_default();
        turns.push(Turn {
            speaker: "player".into(),
            text: utterance.clone(),
        });
        turns.push(Turn {
            speaker: "subscriber".into(),
            text: answer.clone(),
        });
        log.row(CsvRow {
            event: "text_turn",
            sequence,
            revision,
            caller: 11,
            destination: nahid.requested_callee_line,
            status: "accepted",
            text: &format!(
                "player={utterance} | service=caller | classification=none | subscriber={answer}"
            ),
        })?;
    }
    let report = "Nahid is running a bKash scam from Shonarpara Tower. Send police.";
    let response = send_text(
        text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 3,
            state_revision: revision,
            held_controls: HeldControls {
                police: true,
                ..HeldControls::default()
            },
            text: report.into(),
        },
    )?;
    if response.classification.as_deref() != Some("success") {
        return Err(format!("intertwined Nahid report failed: {response:?}").into());
    }
    let snapshot = debug_snapshot(debug)?;
    if snapshot.snapshot.nahid_story_beat != "Stopped"
        || snapshot.snapshot.shapla_story_beat != "EmergencyCall"
        || snapshot.snapshot.neel_story_beat != "ProfessorRouting"
    {
        return Err(format!(
            "intertwined stories changed unexpectedly: {:?}",
            snapshot.snapshot
        )
        .into());
    }
    log.row(CsvRow {
        event: "police_report",
        sequence,
        revision,
        caller: 11,
        destination: 0,
        status: "success",
        text: report,
    })?;
    state = exchange(
        backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "disconnect",
        sequence,
        revision,
        caller: 11,
        destination: nahid.requested_callee_line,
        status: "accepted",
        text: "Nahid's line disconnects after the successful Police report.",
    })?;
    Ok(())
}

fn run_dirty_work(
    backend: &mut TcpStream,
    text: &mut TcpStream,
    debug: &mut TcpStream,
    log: &mut GameLog,
    player_command: &Option<String>,
    path: &str,
    initial_money: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut sequence = 0;
    let mut revision = 0;
    let mut turns = Vec::new();
    let mut state = exchange(
        backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    let rahman = state
        .output
        .calls
        .iter()
        .find(|call| call.caller_line == 6)
        .cloned()
        .ok_or("Dirty Work did not create the Rahman call")?;
    log.row(CsvRow {
        event: "state",
        sequence,
        revision,
        caller: 6,
        destination: 0,
        status: "accepted",
        text: "phase=Waiting",
    })?;

    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(6), PortId::Operator)],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "operator",
        sequence,
        revision,
        caller: 6,
        destination: 0,
        status: "accepted",
        text: "You connect Agent Rahman to the Operator.",
    })?;

    for (turn_id, task) in [
        "answer Agent Rahman with your name",
        "confirm Agent Rahman's identity and acknowledge the surveillance instruction without asking him to repeat his name",
    ]
    .into_iter()
    .enumerate()
    {
        let utterance = player_utterance(player_command, &rahman, revision, &turns, task)?;
        let response = send_text(
            text,
            TextInputMessage {
                protocol_version: TEXT_PROTOCOL_VERSION,
                session_id: 1,
                turn_id: turn_id as u64 + 1,
                state_revision: revision,
                held_controls: HeldControls::default(),
                text: utterance.clone(),
            },
        )?;
        let answer = response.response_text.clone().unwrap_or_default();
        turns.push(Turn {
            speaker: "player".into(),
            text: utterance.clone(),
        });
        turns.push(Turn {
            speaker: "subscriber".into(),
            text: answer.clone(),
        });
        log.row(CsvRow {
            event: "text_turn",
            sequence,
            revision,
            caller: 6,
            destination: 0,
            status: "accepted",
            text: &format!(
                "player={utterance} | service=caller | classification=none | subscriber={answer}"
            ),
        })?;
    }

    state = exchange(
        backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "disconnect",
        sequence,
        revision,
        caller: 6,
        destination: 0,
        status: "accepted",
        text: "You release Agent Rahman's line and begin the authorized monitoring assignment.",
    })?;

    for _ in 0..3 {
        let (caller, callee, label) = state
            .output
            .calls
            .iter()
            .find_map(|call| match call.caller_line {
                9 => Some((9, call.requested_callee_line, "Dr. Kamal calls Bagha News.")),
                8 => Some((8, call.requested_callee_line, "Tariq calls Bagha News.")),
                10 => Some((10, call.requested_callee_line, "Rehana calls Bagha News.")),
                _ => None,
            })
            .ok_or("Dirty Work did not expose its selected contact call")?;
        log.row(CsvRow {
            event: "tap_ready",
            sequence,
            revision,
            caller,
            destination: callee,
            status: "accepted",
            text: &format!("You prepare the monitor TAP for {label}"),
        })?;
        state = route_tap_call(backend, debug, &mut sequence, revision, caller, callee)?;
        revision = state.state_revision;
        log.row(CsvRow {
            event: "route",
            sequence,
            revision,
            caller,
            destination: callee,
            status: "accepted",
            text: &format!("TAP monitors the prerecorded conversation: {label}"),
        })?;
    }

    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(6), PortId::Operator)],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    let report = match path {
        "dirty_good" => "report only routine calls and protect the source",
        "dirty_neutral" => {
            "report the corruption and Stall 4 at Koyal Market without naming the whistleblower"
        }
        "dirty_bad" => "identify Tariq and Salim and report the rotten grain operation",
        other => return Err(format!("unknown Dirty Work path: {other}").into()),
    };
    let report_text = player_utterance(player_command, &rahman, revision, &turns, report)?;
    let response = send_text(
        text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 10,
            state_revision: revision,
            held_controls: HeldControls::default(),
            text: report_text.clone(),
        },
    )?;
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: 6,
        destination: 0,
        status: "accepted",
        text: &format!(
            "player={report_text} | service=caller | classification={} | subscriber={}",
            response.classification.as_deref().unwrap_or("not returned"),
            response.response_text.clone().unwrap_or_default()
        ),
    })?;
    let snapshot = debug_snapshot(debug)?;
    log.row(CsvRow {
        event: "outcome",
        sequence,
        revision,
        caller: 6,
        destination: 0,
        status: "accepted",
        text: &format!(
            "Dirty Work beat: {}; final money ${} (started at ${}).",
            snapshot.snapshot.dirty_work_story_beat, snapshot.snapshot.money, initial_money
        ),
    })?;
    Ok(())
}

fn run_nahid(
    backend: &mut TcpStream,
    text: &mut TcpStream,
    debug: &mut TcpStream,
    log: &mut GameLog,
    path: &str,
    initial_money: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut sequence = 0;
    let mut revision = 0;
    let mut state = exchange(
        backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    let mut victims = Vec::new();

    let scam_count = if path == "nahid_police_success" { 1 } else { 5 };
    for attempt in 0..scam_count {
        let call = state
            .output
            .calls
            .iter()
            .find(|call| call.caller_line == 11)
            .cloned()
            .ok_or_else(|| format!("Nahid call {} did not arrive", attempt + 1))?;
        if victims.contains(&call.requested_callee_line) {
            return Err(
                format!("Nahid selected victim {} twice", call.requested_callee_line).into(),
            );
        }
        victims.push(call.requested_callee_line);
        log.row(CsvRow {
            event: "scam_call",
            sequence,
            revision,
            caller: 11,
            destination: call.requested_callee_line,
            status: "accepted",
            text: &format!("Nahid calls victim {}", call.requested_callee_line),
        })?;
        state = route_tap_call(
            backend,
            debug,
            &mut sequence,
            revision,
            11,
            call.requested_callee_line,
        )?;
        revision = state.state_revision;
        log.row(CsvRow {
            event: "audio",
            sequence,
            revision,
            caller: 11,
            destination: call.requested_callee_line,
            status: "accepted",
            text: "Nahid's prerecorded scam conversation played over the monitor TAP.",
        })?;
    }

    if path == "nahid_police_success" {
        let call = state
            .output
            .calls
            .iter()
            .find(|call| call.caller_line == 11)
            .cloned()
            .ok_or("Nahid did not continue after the first scam")?;
        state = exchange(
            backend,
            input(
                &mut sequence,
                revision,
                vec![cord(PortId::Subscriber(call.caller_line), PortId::Operator)],
                false,
                [0, 0, 0, 1],
            ),
        )?;
        revision = state.state_revision;
        log.row(CsvRow {
            event: "operator",
            sequence,
            revision,
            caller: 11,
            destination: call.requested_callee_line,
            status: "accepted",
            text: "You connect Nahid's line to the Operator before reporting him.",
        })?;
        let report = "Nahid is running a bKash scam from Shonarpara Tower. Send police.";
        let response = send_text(
            text,
            TextInputMessage {
                protocol_version: TEXT_PROTOCOL_VERSION,
                session_id: 1,
                turn_id: 1,
                state_revision: revision,
                held_controls: HeldControls {
                    police: true,
                    ..HeldControls::default()
                },
                text: report.into(),
            },
        )?;
        if response.classification.as_deref() != Some("success") {
            return Err(format!("Nahid police report was not successful: {response:?}").into());
        }
        let snapshot = debug_snapshot(debug)?;
        if snapshot.snapshot.nahid_story_beat != "Stopped" {
            return Err("Nahid did not stop after the successful police report".into());
        }
        log.row(CsvRow {
            event: "police_report",
            sequence,
            revision,
            caller: 11,
            destination: 0,
            status: "success",
            text: report,
        })?;
        state = exchange(
            backend,
            input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
        )?;
        revision = state.state_revision;
        log.row(CsvRow {
            event: "disconnect",
            sequence,
            revision,
            caller: 11,
            destination: call.requested_callee_line,
            status: "accepted",
            text: "Nahid's line disconnects after the successful Police report.",
        })?;
    } else {
        let snapshot = debug_snapshot(debug)?;
        if snapshot.snapshot.nahid_story_beat != "Penalized"
            || snapshot.snapshot.nahid_scam_count != 5
            || !snapshot
                .snapshot
                .call_history
                .iter()
                .any(|call| call.reason.contains("direct circuit completed"))
        {
            return Err(format!(
                "Nahid five-scam path did not penalize correctly: {:?}",
                snapshot.snapshot
            )
            .into());
        }
        log.row(CsvRow {
            event: "penalty",
            sequence,
            revision,
            caller: 11,
            destination: 0,
            status: "accepted",
            text: &format!("five scams completed; balance started at ${initial_money}"),
        })?;
    }
    Ok(())
}

fn route_direct_call(
    backend: &mut TcpStream,
    debug: &mut TcpStream,
    sequence: &mut u64,
    revision: u64,
    caller: u8,
    callee: u8,
) -> Result<StateMessage, Box<dyn std::error::Error>> {
    let mut state = exchange(
        backend,
        input(
            sequence,
            revision,
            vec![cord(PortId::Subscriber(caller), PortId::Operator)],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    let mut revision = state.state_revision;
    let ringing = vec![
        cord(PortId::Subscriber(caller), PortId::Operator),
        cord(PortId::Subscriber(callee), PortId::RingGenerator),
    ];
    state = exchange(
        backend,
        input_with_ring(
            sequence,
            revision,
            ringing.clone(),
            false,
            directory_for_id(
                state
                    .output
                    .calls
                    .iter()
                    .find(|call| call.requested_callee_line == callee)
                    .and_then(|call| call.requested_callee_directory_id)
                    .ok_or("active call has no directory id")?,
            ),
            callee as i16,
        ),
    )?;
    revision = state.state_revision;
    let _ = debug_command(debug, DebugCommand::AdvanceTime { seconds: 3 })?;
    state = exchange(
        backend,
        input_with_ring(
            sequence,
            revision,
            ringing,
            false,
            directory_for_id(
                state
                    .output
                    .calls
                    .iter()
                    .find(|call| call.requested_callee_line == callee)
                    .and_then(|call| call.requested_callee_directory_id)
                    .ok_or("active call has no directory id")?,
            ),
            callee as i16,
        ),
    )?;
    revision = state.state_revision;
    let direct = vec![cord(PortId::Subscriber(caller), PortId::Subscriber(callee))];
    state = exchange(
        backend,
        input(
            sequence,
            revision,
            direct.clone(),
            false,
            directory_for_id(
                state
                    .output
                    .calls
                    .iter()
                    .find(|call| call.requested_callee_line == callee)
                    .and_then(|call| call.requested_callee_directory_id)
                    .ok_or("active call has no directory id")?,
            ),
        ),
    )?;
    revision = state.state_revision;
    let _ = debug_command(debug, DebugCommand::AdvanceTime { seconds: 3 })?;
    Ok(exchange(
        backend,
        input(
            sequence,
            revision,
            direct,
            false,
            directory_for_id(
                state
                    .output
                    .calls
                    .iter()
                    .find(|call| call.requested_callee_line == callee)
                    .and_then(|call| call.requested_callee_directory_id)
                    .ok_or("active call has no directory id")?,
            ),
        ),
    )?)
}

fn route_tap_call(
    backend: &mut TcpStream,
    debug: &mut TcpStream,
    sequence: &mut u64,
    revision: u64,
    caller: u8,
    callee: u8,
) -> Result<StateMessage, Box<dyn std::error::Error>> {
    let state = route_direct_call(backend, debug, sequence, revision, caller, callee)?;
    let revision = state.state_revision;
    let tap = vec![
        cord(PortId::Subscriber(caller), PortId::Tap(1)),
        cord(PortId::Subscriber(callee), PortId::Tap(2)),
    ];
    let mut state = exchange(
        backend,
        tap_input(
            sequence,
            revision,
            tap.clone(),
            true,
            directory_for_id(
                state
                    .output
                    .calls
                    .iter()
                    .find(|call| call.requested_callee_line == callee)
                    .and_then(|call| call.requested_callee_directory_id)
                    .ok_or("active call has no directory id")?,
            ),
        ),
    )?;
    for _ in 0..30 {
        if state.output.tap_bridge_audio_active {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
        state = exchange(
            backend,
            tap_input(
                sequence,
                state.state_revision,
                tap.clone(),
                true,
                directory_for_id(
                    state
                        .output
                        .calls
                        .iter()
                        .find(|call| call.requested_callee_line == callee)
                        .and_then(|call| call.requested_callee_directory_id)
                        .ok_or("active call has no directory id")?,
                ),
            ),
        )?;
    }
    if !state.output.tap_bridge_audio_active {
        return Err(format!(
            "Dirty Work TAP did not activate for LINE {caller}; calls={:?} monitoring={:?}",
            state.output.calls, state.output.tap_bridge_monitoring
        )
        .into());
    }
    let snapshot = debug_snapshot(debug)?;
    let duration = snapshot
        .snapshot
        .active_calls
        .iter()
        .find(|call| call.caller_line == caller)
        .map(|call| call.audio_duration_seconds)
        .unwrap_or(1);
    let _ = debug_command(
        debug,
        DebugCommand::AdvanceTime {
            seconds: duration.saturating_add(1) as u32,
        },
    )?;
    let state = exchange(
        backend,
        tap_input(
            sequence,
            state.state_revision,
            tap,
            false,
            directory_for_id(
                state
                    .output
                    .calls
                    .iter()
                    .find(|call| call.requested_callee_line == callee)
                    .and_then(|call| call.requested_callee_directory_id)
                    .ok_or("active call has no directory id")?,
            ),
        ),
    )?;
    // A completed monitored call keeps the story disconnect gate until its
    // TAP cords are released. Clear the cords before exposing the next state
    // to callers so the next story contact can be queued.
    Ok(exchange(
        backend,
        tap_input(sequence, state.state_revision, vec![], false, [0, 0, 0, 1]),
    )?)
}

fn run_neel_story(
    backend: &mut TcpStream,
    text: &mut TcpStream,
    debug: &mut TcpStream,
    log: &mut GameLog,
    player_command: &Option<String>,
    path: &str,
    initial_money: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut sequence = 0;
    let mut revision = 0;
    let mut state = exchange(
        backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    let professor = state
        .output
        .calls
        .iter()
        .find(|call| call.caller_line == 2)
        .cloned()
        .ok_or("Neel University did not call")?;
    log.row(CsvRow {
        event: "state",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "Neel University calls; Professor Routing is active.",
    })?;

    if path == "neel_patience" {
        let _ = debug_command(debug, DebugCommand::AdvanceTime { seconds: 33 })?;
        state = exchange(
            backend,
            input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
        )?;
        revision = state.state_revision;
        let snapshot = debug_snapshot(debug)?;
        if snapshot.snapshot.money != -4
            || snapshot.snapshot.neel_story_beat != "ProfessorRouting"
            || !state
                .output
                .printer_output
                .iter()
                .any(|entry| entry.text.contains("-$4 missed call line 2"))
        {
            return Err(format!(
                "Neel patience expiry was not recorded correctly: money={}, beat={}, printer={:?}",
                snapshot.snapshot.money,
                snapshot.snapshot.neel_story_beat,
                state.output.printer_output
            )
            .into());
        }
        log.row(CsvRow {
            event: "patience",
            sequence,
            revision,
            caller: 2,
            destination: 3,
            status: "accepted",
            text: "Neel call expired: Beat 1 repeated with a -$4 printer entry.",
        })?;
        log.row(CsvRow {
            event: "beat_result",
            sequence,
            revision,
            caller: 2,
            destination: 3,
            status: "accepted",
            text: "Backend transition: ProfessorRouting -> ProfessorRouting after patience expiry.",
        })?;
        return Ok(());
    }
    let operator = vec![cord(PortId::Subscriber(2), PortId::Operator)];
    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            operator.clone(),
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "operator",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "You connect Neel University to the Operator.",
    })?;
    let mut turns = Vec::new();
    let question = if path == "neel_professor_questions" {
        "What subject do you teach?".to_string()
    } else {
        player_utterance(
            player_command,
            &professor,
            revision,
            &turns,
            "ask Prof. Kashem where he wants to be connected",
        )?
    };
    let answer = send_text(
        text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 1,
            state_revision: revision,
            held_controls: HeldControls::default(),
            text: question.clone(),
        },
    )?;
    let answer_text = answer.response_text.clone().unwrap_or_default();
    turns.push(Turn {
        speaker: "player".into(),
        text: question.clone(),
    });
    turns.push(Turn {
        speaker: "subscriber".into(),
        text: answer_text.clone(),
    });
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: &format!(
            "player={question} | service=caller | classification=none | subscriber={answer_text}"
        ),
    })?;

    if path.contains("misdirection") {
        let wrong = vec![cord(PortId::Subscriber(2), PortId::Subscriber(4))];
        let wrong_response = exchange(
            backend,
            input(
                &mut sequence,
                revision,
                wrong,
                false,
                directory_for_id(1031),
            ),
        )?;
        if wrong_response.accepted {
            return Err("Neel misdirection unexpectedly succeeded".into());
        }
        revision = wrong_response.state_revision;
        log.row(CsvRow {
            event: "route",
            sequence,
            revision,
            caller: 2,
            destination: 4,
            status: "rejected",
            text: "You attempt to connect LINE 2 directly to LINE 4; the requested destination is LINE 3.",
        })?;
        log.row(CsvRow {
            event: "misdirection",
            sequence,
            revision,
            caller: 2,
            destination: 4,
            status: "rejected",
            text: "The wrong direct circuit fails and Neel University calls again.",
        })?;
        state = exchange(
            backend,
            input(
                &mut sequence,
                revision,
                vec![cord(PortId::Subscriber(2), PortId::Operator)],
                false,
                [0, 0, 0, 1],
            ),
        )?;
        revision = state.state_revision;
    }

    let ringing = vec![
        cord(PortId::Subscriber(2), PortId::Operator),
        cord(PortId::Subscriber(3), PortId::RingGenerator),
    ];
    state = exchange(
        backend,
        input_with_ring(
            &mut sequence,
            revision,
            ringing.clone(),
            false,
            directory_for_id(1024),
            3,
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "ring_start",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "You connect the Ring Generator to Shadhin Housing and keep ringing.",
    })?;
    let _ = debug_command(debug, DebugCommand::AdvanceTime { seconds: 3 })?;
    state = exchange(
        backend,
        input_with_ring(
            &mut sequence,
            revision,
            ringing,
            false,
            directory_for_id(1024),
            3,
        ),
    )?;
    revision = state.state_revision;
    if !state.output.line_lamps[3] {
        return Err("Shadhin Housing did not light after the crank delay".into());
    }
    log.row(CsvRow {
        event: "ring_ready",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "Shadhin Housing LED is active after the delayed crank.",
    })?;
    let direct = vec![cord(PortId::Subscriber(2), PortId::Subscriber(3))];
    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            direct.clone(),
            false,
            directory_for_id(1024),
        ),
    )?;
    revision = state.state_revision;
    if !state.accepted {
        return Err(format!("Neel University direct route rejected: {:?}", state.error).into());
    }
    let direct_snapshot = debug_snapshot(debug)?;
    let direct_call = direct_snapshot
        .snapshot
        .active_calls
        .iter()
        .find(|call| call.caller_line == 2)
        .ok_or("direct Neel call disappeared")?;
    if direct_call.phase != exchange_protocol::CallPhase::Connected {
        return Err(format!(
            "direct Neel call did not connect immediately: {:?}",
            direct_call.phase
        )
        .into());
    }
    log.row(CsvRow {
        event: "route",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "You disconnect LINE 2 from the Operator and connect it directly to LINE 3.",
    })?;
    let first_audio_duration = debug_snapshot(debug)?
        .snapshot
        .active_calls
        .iter()
        .find(|call| call.caller_line == 2)
        .map(|call| call.audio_duration_seconds)
        .unwrap_or(2);
    if path.contains("tap") || path.contains("rewire") {
        if path.contains("late") {
            let _ = debug_command(
                debug,
                DebugCommand::AdvanceTime {
                    seconds: (first_audio_duration / 2) as u32,
                },
            )?;
        }
        let tap = if path.contains("reverse") {
            vec![
                cord(PortId::Subscriber(2), PortId::Tap(2)),
                cord(PortId::Subscriber(3), PortId::Tap(1)),
            ]
        } else {
            vec![
                cord(PortId::Subscriber(2), PortId::Tap(1)),
                cord(PortId::Subscriber(3), PortId::Tap(2)),
            ]
        };
        if path.contains("rewire") {
            state = exchange(
                backend,
                input(
                    &mut sequence,
                    revision,
                    vec![],
                    false,
                    directory_for_id(1024),
                ),
            )?;
            revision = state.state_revision;
            if path.contains("late") {
                // The first advance moves halfway through the recording. Move past
                // the remaining audio plus the five-second TAP rewire window so
                // this path is deterministic for recordings of any authored length.
                let remaining = first_audio_duration
                    .saturating_sub(first_audio_duration / 2)
                    .saturating_add(6) as u32;
                let _ = debug_command(debug, DebugCommand::AdvanceTime { seconds: remaining })?;
            }
        }
        state = exchange(
            backend,
            tap_input(
                &mut sequence,
                revision,
                tap.clone(),
                true,
                directory_for_id(1024),
            ),
        )?;
        revision = state.state_revision;
        if path.contains("rewire_late") {
            if state.output.tap_bridge_audio_active {
                return Err("late Neel rewire unexpectedly kept TAP audio active".into());
            }
        } else if !state.output.tap_bridge_audio_active {
            return Err("TAP did not become active for the Neel/Shadhin call".into());
        }
        if !path.contains("rewire_late") {
            let monitoring = state
                .output
                .tap_bridge_monitoring
                .as_ref()
                .ok_or("TAP state did not identify the monitored connection")?;
            let expected_caller_port = if path.contains("reverse") { 2 } else { 1 };
            if monitoring.caller_line != 2
                || monitoring.callee_line != 3
                || monitoring.caller_tap_port != expected_caller_port
            {
                return Err(format!("unexpected TAP monitoring state: {monitoring:?}").into());
            }
        }
        if !path.contains("rewire_late")
            && (!state.output.line_lamps[2] || !state.output.line_lamps[3])
        {
            return Err("both Neel/Shadhin LEDs were not active during monitored audio".into());
        }
        log.row(CsvRow {
            event: "tap",
            sequence,
            revision,
            caller: 2,
            destination: 3,
            status: "accepted",
            text: if path.contains("rewire_late") {
                "TAP rewire was attempted after the buffer expired."
            } else {
                "TAP is held and monitors the active call timeline."
            },
        })?;
        if path.contains("rewire_late") {
            log.row(CsvRow {
                event: "rewire_expired",
                sequence,
                revision,
                caller: 2,
                destination: 3,
                status: "accepted",
                text: "The 5-second rewiring buffer expired; the old call was no longer monitorable.",
            })?;
            return Ok(());
        }
        let _ = debug_command(
            debug,
            DebugCommand::AdvanceTime {
                seconds: first_audio_duration.saturating_add(1) as u32,
            },
        )?;
        state = exchange(
            backend,
            tap_input(&mut sequence, revision, tap, false, directory_for_id(1024)),
        )?;
        // Release the TAP cords after the recording ends. Keeping either TAP
        // cord connected intentionally holds the completed story circuit's
        // disconnect gate, which prevents Arnab's follow-up from being queued.
        state = exchange(
            backend,
            tap_input(
                &mut sequence,
                state.state_revision,
                vec![],
                false,
                directory_for_id(1024),
            ),
        )?;
        if state.output.line_lamps[2] {
            return Err("Neel LED remained active after the loaded audio duration".into());
        }
    } else {
        let _ = debug_command(
            debug,
            DebugCommand::AdvanceTime {
                seconds: first_audio_duration.saturating_add(1) as u32,
            },
        )?;
        state = exchange(
            backend,
            input(
                &mut sequence,
                revision,
                direct.clone(),
                false,
                directory_for_id(1024),
            ),
        )?;
        // The completed direct circuit is removed during the input above. The
        // next story caller is queued only after that removal, so release the
        // old circuit before looking for Arnab's follow-up call.
        state = exchange(
            backend,
            input(
                &mut sequence,
                state.state_revision,
                vec![],
                false,
                directory_for_id(1024),
            ),
        )?;
    }
    revision = state.state_revision;
    let first_transition = debug_snapshot(debug)?;
    if first_transition.snapshot.neel_story_beat != "ArnabDirectory" {
        return Err(format!(
            "Neel story did not enter ArnabDirectory: {}",
            first_transition.snapshot.neel_story_beat
        )
        .into());
    }
    log.row(CsvRow {
        event: "audio",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: &format!(
            "The operator hears no direct-call mix; the authored Professor recording runs for {} seconds on the connected lines.",
            first_audio_duration
        ),
    })?;
    log.row(CsvRow {
        event: "beat_result",
        sequence,
        revision,
        caller: 2,
        destination: 3,
        status: "accepted",
        text: "Backend transition: ProfessorRouting -> ArnabDirectory.",
    })?;
    let arnab = state
        .output
        .calls
        .iter()
        .find(|call| call.caller_line == 3)
        .cloned()
        .ok_or_else(|| {
            format!(
                "Arnab did not call after the Professor connection; active calls: {:?}",
                state.output.calls
            )
        })?;
    log.row(CsvRow {
        event: "followup",
        sequence,
        revision,
        caller: 3,
        destination: arnab.requested_callee_line,
        status: "accepted",
        text: "Arnab Bhattacharjee calls from Shadhin Housing.",
    })?;
    if path.contains("arnab_patience") {
        let _ = debug_command(debug, DebugCommand::AdvanceTime { seconds: 33 })?;
        state = exchange(
            backend,
            input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
        )?;
        revision = state.state_revision;
        let snapshot = debug_snapshot(debug)?;
        if snapshot.snapshot.neel_story_beat != "ArnabDirectory"
            || !state
                .output
                .printer_output
                .iter()
                .any(|entry| entry.text.contains("-$4 missed call line 3"))
        {
            return Err(format!(
                "Arnab patience retry was not recorded correctly: beat={}, printer={:?}",
                snapshot.snapshot.neel_story_beat, state.output.printer_output
            )
            .into());
        }
        log.row(CsvRow {
            event: "patience",
            sequence,
            revision,
            caller: 3,
            destination: arnab.requested_callee_line,
            status: "accepted",
            text: "Arnab call expired and ArnabDirectory was retried with a -$4 printer entry.",
        })?;
        return Ok(());
    }
    state = exchange(
        backend,
        input(
            &mut sequence,
            revision,
            vec![cord(PortId::Subscriber(3), PortId::Operator)],
            false,
            [0, 0, 0, 1],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "operator",
        sequence,
        revision,
        caller: 3,
        destination: arnab.requested_callee_line,
        status: "accepted",
        text: "You connect LINE 3 to the Operator.",
    })?;
    let mut arnab_turns = turns;
    for turn in 0..2 {
        let task = if turn == 0 {
            "ask Arnab who he wants to be connected to"
        } else if path == "neel_arnab_unrelated_questions" {
            "ask Arnab an unrelated personal question about his favorite food"
        } else if path.contains("questions") {
            "ask Arnab whether Bela Bose has a cat"
        } else {
            "ask Arnab whether he knows Bela Bose's directory ID"
        };
        let utterance = if path == "neel_arnab_unrelated_questions" && turn == 1 {
            "What is your favorite food?".to_string()
        } else {
            player_utterance(player_command, &arnab, revision, &arnab_turns, task)?
        };
        let response = send_text(
            text,
            TextInputMessage {
                protocol_version: TEXT_PROTOCOL_VERSION,
                session_id: 1,
                turn_id: 10 + turn,
                state_revision: revision,
                held_controls: HeldControls::default(),
                text: utterance.clone(),
            },
        )?;
        let response_text = response.response_text.clone().unwrap_or_default();
        arnab_turns.push(Turn {
            speaker: "player".into(),
            text: utterance.clone(),
        });
        arnab_turns.push(Turn {
            speaker: "subscriber".into(),
            text: response_text.clone(),
        });
        log.row(CsvRow {
            event: "text_turn",
            sequence,
            revision,
            caller: 3,
            destination: 4,
            status: "accepted",
            text: &format!("player={utterance} | service=caller | classification=none | subscriber={response_text}"),
        })?;
    }
    let destination = if path.contains("1031") { 4 } else { 5 };
    let digits = if destination == 4 {
        [1, 0, 3, 1]
    } else {
        [1, 0, 3, 2]
    };
    let (_, next_revision) = ring_destination(backend, debug, &mut sequence, revision, 3, digits)?;
    revision = next_revision;
    let direct = vec![cord(PortId::Subscriber(3), PortId::Subscriber(destination))];
    let mut state = exchange(
        backend,
        input(&mut sequence, revision, direct.clone(), false, digits),
    )?;
    revision = state.state_revision;
    if !state.accepted {
        return Err(format!("Bela route rejected: {:?}", state.error).into());
    }
    log.row(CsvRow {
        event: "directory",
        sequence,
        revision,
        caller: 3,
        destination,
        status: "accepted",
        text: &format!(
            "You use directory {} to connect LINE 3 directly to Bela Bose on LINE {}.",
            digits.iter().map(u8::to_string).collect::<String>(),
            destination
        ),
    })?;
    let connected_snapshot = debug_snapshot(debug)?;
    let audio_duration = connected_snapshot
        .snapshot
        .active_calls
        .iter()
        .find(|call| call.caller_line == 3)
        .map(|call| call.audio_duration_seconds)
        .ok_or("Bela connection did not become active")?;
    log.row(CsvRow {
        event: "audio",
        sequence,
        revision,
        caller: 3,
        destination,
        status: "accepted",
        text: &format!(
            "The operator hears no direct-call mix; the authored Bela recording runs for {audio_duration} seconds on LINE 3 and LINE {destination}."
        ),
    })?;
    let _ = debug_command(
        debug,
        DebugCommand::AdvanceTime {
            seconds: audio_duration.saturating_add(1) as u32,
        },
    )?;
    state = exchange(
        backend,
        input(&mut sequence, revision, direct, false, digits),
    )?;
    revision = state.state_revision;
    if !state.accepted {
        return Err(format!("Bela completion input rejected: {:?}", state.error).into());
    }
    let snapshot = debug_snapshot(debug)?;
    if destination == 4 {
        if snapshot.snapshot.neel_story_beat != "BadEnding" {
            return Err(format!(
                "wrong Bela route changed Neel beat unexpectedly: {}",
                snapshot.snapshot.neel_story_beat
            )
            .into());
        }
        log.row(CsvRow {
            event: "beat_result",
            sequence,
            revision,
            caller: 3,
            destination,
            status: "accepted",
            text: "Wrong Bela route reached terminal BadEnding; no further Neel call is created.",
        })?;
        return Ok(());
    }
    if snapshot.snapshot.neel_story_beat != "Completed" {
        return Err(format!(
            "Neel story did not complete: beat={}, calls={:?}, state_calls={:?}, history={:?}",
            snapshot.snapshot.neel_story_beat,
            snapshot.snapshot.active_calls,
            state.output.calls,
            snapshot.snapshot.call_history
        )
        .into());
    }
    log.row(CsvRow {
        event: "disconnect",
        sequence,
        revision,
        caller: 3,
        destination,
        status: "accepted",
        text: "The Bela Bose recording ended and the completed connection cleared.",
    })?;
    log.row(CsvRow {
        event: "beat_result",
        sequence,
        revision,
        caller: 3,
        destination,
        status: "accepted",
        text: "Backend transition: ArnabDirectory -> Completed.",
    })?;
    assert_printer_balance(&state, snapshot.snapshot.money)?;
    let outcome = format!(
        "Backend final money: ${} (started at ${}; delta ${}). Final beat: {}.",
        snapshot.snapshot.money,
        initial_money,
        snapshot.snapshot.money - initial_money,
        snapshot.snapshot.neel_story_beat,
    );
    for entry in &state.output.printer_output {
        if entry.text.contains("MONEY //") {
            log.row(CsvRow {
                event: "money",
                sequence,
                revision,
                caller: 3,
                destination,
                status: "accepted",
                text: &entry.text,
            })?;
        }
    }
    log.row(CsvRow {
        event: "outcome",
        sequence,
        revision,
        caller: 3,
        destination,
        status: "accepted",
        text: &outcome,
    })?;
    log.row(CsvRow {
        event: "finish",
        sequence,
        revision,
        caller: 3,
        destination,
        status: "accepted",
        text: "Neel University story complete.",
    })?;
    Ok(())
}

fn cord(first: PortId, second: PortId) -> CordConnection {
    CordConnection { first, second }
}

fn directory_for_id(directory_id: u16) -> [u8; 4] {
    [
        (directory_id / 1000) as u8,
        ((directory_id / 100) % 10) as u8,
        ((directory_id / 10) % 10) as u8,
        (directory_id % 10) as u8,
    ]
}

fn input(
    sequence: &mut u64,
    revision: u64,
    cords: Vec<CordConnection>,
    ptt: bool,
    digits: [u8; 4],
) -> InputMessage {
    *sequence += 1;
    let ring_line = cords
        .iter()
        .find_map(|cord| match (&cord.first, &cord.second) {
            (PortId::RingGenerator, PortId::Subscriber(line))
            | (PortId::Subscriber(line), PortId::RingGenerator) => Some(i16::from(*line)),
            _ => None,
        })
        .unwrap_or(-1);
    InputMessage {
        protocol_version: PROTOCOL_VERSION,
        input_sequence: *sequence,
        expected_state_revision: revision,
        input: InputState {
            cord_topology: cords,
            topology_revision: 0,
            held_controls: HeldControls {
                ptt,
                ..HeldControls::default()
            },
            directory_digits: digits,
            ring_line,
            crank_active: false,
            tuning: TuningState::default(),
            debug: InputDebug {
                firmware_version: Some("text-test-frontend".into()),
                transport_connected: true,
                device_faults: vec![],
                topology_status: "empty".to_string(),
                topology_age_ms: 0,
            },
        },
    }
}

fn input_with_ring(
    sequence: &mut u64,
    revision: u64,
    cords: Vec<CordConnection>,
    ptt: bool,
    digits: [u8; 4],
    ring_line: i16,
) -> InputMessage {
    let mut message = input(sequence, revision, cords, ptt, digits);
    message.input.ring_line = ring_line;
    message
}

fn ring_destination(
    backend: &mut TcpStream,
    debug: &mut TcpStream,
    sequence: &mut u64,
    revision: u64,
    caller: u8,
    digits: [u8; 4],
) -> io::Result<(StateMessage, u64)> {
    let mut state = exchange(
        backend,
        input(
            sequence,
            revision,
            vec![cord(PortId::Subscriber(caller), PortId::Operator)],
            false,
            digits,
        ),
    )?;
    let next_revision = state.state_revision;
    let ring_line = state
        .output
        .calls
        .iter()
        .find(|call| call.caller_line == caller)
        .map(|call| call.requested_callee_line)
        .ok_or_else(|| io::Error::other("ring request has no active caller"))?;
    let ringing = vec![
        cord(PortId::Subscriber(caller), PortId::Operator),
        cord(PortId::Subscriber(ring_line), PortId::RingGenerator),
    ];
    state = exchange(
        backend,
        input_with_ring(
            sequence,
            next_revision,
            ringing.clone(),
            false,
            digits,
            ring_line as i16,
        ),
    )?;
    if !state.accepted {
        return Err(io::Error::other(format!(
            "ring request rejected: {:?}",
            state.error
        )));
    }
    // The backend deliberately randomizes the crank delay between one and
    // three seconds. Advance past the maximum rather than relying on a draw.
    let _ = debug_command(debug, DebugCommand::AdvanceTime { seconds: 5 })?;
    let next_revision = state.state_revision;
    state = exchange(
        backend,
        input_with_ring(
            sequence,
            next_revision,
            ringing,
            false,
            digits,
            ring_line as i16,
        ),
    )?;
    if !state.accepted {
        return Err(io::Error::other(format!(
            "ring activation rejected: {:?}",
            state.error
        )));
    }
    if !state.output.line_lamps[ring_line as usize] {
        return Err(io::Error::other(format!(
            "ringed destination LINE {ring_line} LED did not activate: calls={:?} lamps={:?}",
            state.output.calls, state.output.line_lamps
        )));
    }
    Ok((state.clone(), state.state_revision))
}

fn tap_input(
    sequence: &mut u64,
    revision: u64,
    cords: Vec<CordConnection>,
    tap: bool,
    digits: [u8; 4],
) -> InputMessage {
    let mut message = input(sequence, revision, cords, false, digits);
    message.input.held_controls.tap = tap;
    message
}

fn assert_printer_balance(
    state: &StateMessage,
    expected_balance: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let money_entries = state
        .output
        .printer_output
        .iter()
        .filter(|entry| entry.text.contains("MONEY //"))
        .collect::<Vec<_>>();
    if money_entries.is_empty() {
        return Err("backend produced no printer money entries".into());
    }
    let last_balance = money_entries
        .last()
        .and_then(|entry| entry.text.split("balance $").last())
        .and_then(|value| value.parse::<i32>().ok());
    if last_balance != Some(expected_balance) {
        return Err(format!(
            "printer balance {:?} disagrees with backend balance ${expected_balance}",
            last_balance
        )
        .into());
    }
    Ok(())
}

fn exchange(stream: &mut TcpStream, message: InputMessage) -> io::Result<StateMessage> {
    write_frame(stream, &message).map_err(frame_io)?;
    read_frame(stream).map_err(frame_io)
}

fn send_text(stream: &mut TcpStream, message: TextInputMessage) -> io::Result<TextResponseMessage> {
    write_frame(stream, &message).map_err(frame_io)?;
    read_frame(stream).map_err(frame_io)
}

fn debug_snapshot(stream: &mut TcpStream) -> io::Result<DebugResponse> {
    write_frame(
        stream,
        &DebugRequest {
            protocol_version: DEBUG_PROTOCOL_VERSION,
            command: DebugCommand::Snapshot,
        },
    )
    .map_err(frame_io)?;
    read_frame(stream).map_err(frame_io)
}

fn debug_command(stream: &mut TcpStream, command: DebugCommand) -> io::Result<DebugResponse> {
    write_frame(
        stream,
        &DebugRequest {
            protocol_version: DEBUG_PROTOCOL_VERSION,
            command,
        },
    )
    .map_err(frame_io)?;
    read_frame(stream).map_err(frame_io)
}

fn frame_io(error: exchange_protocol::FrameError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn player_utterance(
    command: &Option<String>,
    call: &exchange_protocol::CallStatus,
    revision: u64,
    turns: &[Turn],
    task: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(command) = command {
        let prompt = serde_json::to_vec(&PlayerPrompt {
            caller_line: call.caller_line,
            destination_line: call.requested_callee_line,
            state_revision: revision,
            conversation: turns,
            task,
        })?;
        for _attempt in 0..3 {
            let mut child = Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?;
            child.stdin.take().unwrap().write_all(&prompt)?;
            let output = child.wait_with_output()?;
            if output.status.success() {
                #[derive(Deserialize)]
                struct ResultText {
                    text: String,
                }
                return Ok(serde_json::from_slice::<ResultText>(&output.stdout)?.text);
            }
        }
        return Err("player command failed after three attempts".into());
    }
    let stdin = io::stdin();
    eprint!("player> ");
    io::stderr().flush()?;
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}
