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
    let initial_debug = select_story_thread(&mut debug, "shapla_apartments")?;
    if initial_debug.snapshot.story_thread != "shapla_apartments"
        || initial_debug.snapshot.story_beat != "EmergencyCall"
    {
        return Err("thread selection did not reset to EmergencyCall".into());
    }
    let initial_money = initial_debug.snapshot.money;
    let mut log = GameLog::open(&log_path)?;
    writeln!(
        log.file,
        "=== STORY THREAD: shapla_apartments / PATH: {path} ==="
    )?;
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
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        child.stdin.take().unwrap().write_all(&prompt)?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(format!("player command failed: {}", output.status).into());
        }
        #[derive(Deserialize)]
        struct ResultText {
            text: String,
        }
        return Ok(serde_json::from_slice::<ResultText>(&output.stdout)?.text);
    }
    let stdin = io::stdin();
    eprint!("player> ");
    io::stderr().flush()?;
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}
