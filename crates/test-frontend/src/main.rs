use std::env;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

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
        let _ = (row.destination, row.status, row.sequence, row.revision);
        let prefix = format!("[{}]", self.entry_number + 1);
        self.entry_number += 1;
        match row.event {
            "state" => writeln!(
                self.file,
                "{} Story beat 1 begins: LINE {} calls.",
                prefix, row.caller
            ),
            "operator" => writeln!(
                self.file,
                "{} You connect LINE {} to the Operator.",
                prefix, row.caller
            ),
            "opening" => writeln!(
                self.file,
                "{} The caller says: \"My mother fell down in the bathroom. I don't know what to do.\".",
                prefix
            ),
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
                writeln!(
                    self.file,
                    "{} You say to {}: \"{}\".",
                    prefix,
                    if service == "caller" {
                        "the caller"
                    } else {
                        service
                    },
                    player
                )?;
                if classification != "none" {
                    writeln!(
                        self.file,
                        "{} The request is classified as {}.",
                        prefix, classification
                    )?;
                }
                if !subscriber.is_empty() {
                    writeln!(self.file, "{} The caller says: \"{}\".", prefix, subscriber)?;
                }
                Ok(())
            }
            "disconnect" => writeln!(self.file, "{} You disconnect LINE {}.", prefix, row.caller),
            "beat_result" => writeln!(self.file, "{} {}", prefix, row.text),
            "followup" => writeln!(
                self.file,
                "{} Story beat 2 begins: LINE {} calls again.",
                prefix, row.caller
            ),
            "npc_response" => writeln!(self.file, "{} The caller says: \"{}\".", prefix, row.text),
            "money" => writeln!(self.file, "{} {}", prefix, row.text),
            "outcome" => writeln!(self.file, "{} {}", prefix, row.text),
            "finish" => writeln!(self.file, "{} Story complete.", prefix),
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
    let story_thread = if path.starts_with("neel_") {
        "neel_university"
    } else {
        "shapla_apartments"
    };
    let initial_debug = select_story_thread(&mut debug, story_thread)?;
    if initial_debug.snapshot.story_thread != story_thread
        || (story_thread == "shapla_apartments"
            && initial_debug.snapshot.story_beat != "EmergencyCall")
        || (story_thread == "neel_university"
            && initial_debug.snapshot.story_beat != "ProfessorRouting")
    {
        return Err("thread selection did not reset to EmergencyCall".into());
    }
    let initial_money = initial_debug.snapshot.money;
    let mut log = GameLog::open(&log_path)?;
    writeln!(
        log.file,
        "=== STORY THREAD: {story_thread} / PATH: {path} ==="
    )?;
    if story_thread == "neel_university" {
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
        text: "opening caller dialogue",
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
    println!("PLAYER LLM    // {utterance}");
    println!(
        "CLASSIFIER    // {}",
        text_response
            .classification
            .as_deref()
            .unwrap_or("not returned")
    );
    println!("SUBSCRIBER    // {response_text}");
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
    let transition_debug = debug_snapshot(&mut debug)?;
    let expected_beat = if no_service {
        "EmergencyCall"
    } else if path == "police_success" {
        "NeutralFollowup"
    } else if classified_success {
        "HappyFollowup"
    } else {
        "BadFollowup"
    };
    if transition_debug.snapshot.story_beat != expected_beat {
        return Err(format!(
            "path {path} expected backend beat {expected_beat}, got {}",
            transition_debug.snapshot.story_beat
        )
        .into());
    }
    let branch = format!(
        "Backend transition: EmergencyCall -> {}.",
        transition_debug.snapshot.story_beat
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

    state = exchange(
        &mut backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    if no_service {
        let abandonment_debug = debug_snapshot(&mut debug)?;
        if abandonment_debug.snapshot.story_beat != "BadFollowup" {
            return Err(format!(
                "path {path} expected abandonment to select BadFollowup, got {}",
                abandonment_debug.snapshot.story_beat
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
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    revision = state.state_revision;
    let final_debug = debug_snapshot(&mut debug)?;
    let outcome = format!(
        "Backend final money: ${} (started at ${}; delta ${}). Final beat: {}.",
        final_debug.snapshot.money,
        initial_money,
        final_debug.snapshot.money - initial_money,
        final_debug.snapshot.story_beat,
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

    if path.contains("patience") {
        let _ = debug_command(debug, DebugCommand::AdvanceTime { seconds: 33 })?;
        state = exchange(
            backend,
            input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
        )?;
        revision = state.state_revision;
        let snapshot = debug_snapshot(debug)?;
        if snapshot.snapshot.money != -4
            || snapshot.snapshot.story_beat != "ProfessorRouting"
            || !state
                .output
                .printer_output
                .iter()
                .any(|entry| entry.text.contains("-$4 missed call line 2"))
        {
            return Err(format!(
                "Neel patience expiry was not recorded correctly: money={}, beat={}, printer={:?}",
                snapshot.snapshot.money, snapshot.snapshot.story_beat, state.output.printer_output
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
    let question = player_utterance(
        player_command,
        &professor,
        revision,
        &turns,
        "ask Prof. Kashem where he wants to be connected",
    )?;
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
            input(&mut sequence, revision, wrong, false, [0, 0, 0, 4]),
        )?;
        if wrong_response.accepted {
            return Err("Neel misdirection unexpectedly succeeded".into());
        }
        revision = wrong_response.state_revision;
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
            [0, 0, 0, 3],
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
        input_with_ring(&mut sequence, revision, ringing, false, [0, 0, 0, 3], 3),
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
        input(&mut sequence, revision, direct.clone(), false, [0, 0, 0, 3]),
    )?;
    revision = state.state_revision;
    if !state.accepted {
        return Err(format!("Neel University direct route rejected: {:?}", state.error).into());
    }
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
                input(&mut sequence, revision, vec![], false, [0, 0, 0, 3]),
            )?;
            revision = state.state_revision;
            if path.contains("late") {
                let _ = debug_command(debug, DebugCommand::AdvanceTime { seconds: 6 })?;
            }
        }
        state = exchange(
            backend,
            tap_input(&mut sequence, revision, tap.clone(), true, [0, 0, 0, 3]),
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
            tap_input(&mut sequence, revision, tap, false, [0, 0, 0, 3]),
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
            input(&mut sequence, revision, direct.clone(), false, [0, 0, 0, 3]),
        )?;
    }
    revision = state.state_revision;
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
        if snapshot.snapshot.story_beat != "Completed"
            || !state
                .output
                .printer_output
                .iter()
                .any(|entry| entry.text.contains("-$4 missed call line 3"))
        {
            return Err(format!(
                "Arnab patience expiry was not recorded correctly: beat={}, printer={:?}",
                snapshot.snapshot.story_beat, state.output.printer_output
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
            text: "Arnab call expired and the story ended with a -$4 printer entry.",
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
    let mut arnab_turns = turns;
    for turn in 0..2 {
        let task = if turn == 0 {
            "ask Arnab who he wants to be connected to"
        } else if path.contains("questions") {
            "ask Arnab whether Bela Bose has a cat"
        } else {
            "ask Arnab whether he knows Bela Bose's directory ID"
        };
        let utterance = player_utterance(player_command, &arnab, revision, &arnab_turns, task)?;
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
    let direct = vec![cord(PortId::Subscriber(3), PortId::Subscriber(destination))];
    state = exchange(
        backend,
        input(&mut sequence, revision, direct.clone(), false, digits),
    )?;
    revision = state.state_revision;
    if !state.accepted {
        return Err(format!("Bela route rejected: {:?}", state.error).into());
    }
    let connected_snapshot = debug_snapshot(debug)?;
    let audio_duration = connected_snapshot
        .snapshot
        .active_calls
        .iter()
        .find(|call| call.caller_line == 3)
        .map(|call| call.audio_duration_seconds)
        .ok_or("Bela connection did not become active")?;
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
    if snapshot.snapshot.story_beat != "Completed" {
        return Err(format!(
            "Neel story did not complete: beat={}, calls={:?}, state_calls={:?}, history={:?}",
            snapshot.snapshot.story_beat,
            snapshot.snapshot.active_calls,
            state.output.calls,
            snapshot.snapshot.call_history
        )
        .into());
    }
    assert_printer_balance(&state, snapshot.snapshot.money)?;
    let outcome = format!(
        "Backend final money: ${} (started at ${}; delta ${}). Final beat: {}.",
        snapshot.snapshot.money,
        initial_money,
        snapshot.snapshot.money - initial_money,
        snapshot.snapshot.story_beat,
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
            held_controls: HeldControls {
                ptt,
                ..HeldControls::default()
            },
            directory_digits: digits,
            ring_line,
            tuning: TuningState::default(),
            debug: InputDebug {
                firmware_version: Some("text-test-frontend".into()),
                transport_connected: true,
                device_faults: vec![],
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

fn select_story_thread(stream: &mut TcpStream, thread_id: &str) -> io::Result<DebugResponse> {
    write_frame(
        stream,
        &DebugRequest {
            protocol_version: DEBUG_PROTOCOL_VERSION,
            command: DebugCommand::SelectStoryThread {
                thread_id: thread_id.into(),
            },
        },
    )
    .map_err(frame_io)?;
    let response: DebugResponse = read_frame(stream).map_err(frame_io)?;
    if !response.accepted {
        return Err(io::Error::other(
            response
                .error
                .map(|error| error.message)
                .unwrap_or_else(|| "story selection rejected".into()),
        ));
    }
    Ok(response)
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
