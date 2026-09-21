use std::env;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

use exchange_protocol::{
    CordConnection, HeldControls, InputDebug, InputMessage, InputState, PROTOCOL_VERSION, PortId,
    StateMessage, TEXT_PROTOCOL_VERSION, TextInputMessage, TextResponseMessage, TextStatus,
    TuningState, read_frame, write_frame,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct PlayerPrompt<'a> {
    caller_line: u8,
    destination_line: u8,
    state_revision: u64,
    conversation: &'a [Turn],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Turn {
    speaker: String,
    text: String,
}

struct CsvLog {
    file: std::fs::File,
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

impl CsvLog {
    fn open(path: &str) -> io::Result<Self> {
        let new = !std::path::Path::new(path).exists();
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        if new {
            writeln!(
                file,
                "event,sequence,state_revision,caller_line,destination_line,status,text"
            )?;
        }
        Ok(Self { file })
    }

    fn row(&mut self, row: CsvRow<'_>) -> io::Result<()> {
        writeln!(
            self.file,
            "{},{},{},{},{},{},\"{}\"",
            csv(row.event),
            row.sequence,
            row.revision,
            row.caller,
            row.destination,
            csv(row.status),
            csv(row.text)
        )
    }
}

fn csv(value: &str) -> String {
    value.replace('"', "\"\"")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend_address = argument("--connect").unwrap_or_else(|| "127.0.0.1:7878".into());
    let text_address = argument("--text-connect").unwrap_or_else(|| "127.0.0.1:7880".into());
    let csv_path = argument("--csv").unwrap_or_else(|| "test-frontend.csv".into());
    let player_command = argument("--player-command");

    let mut backend = TcpStream::connect(&backend_address)?;
    let mut text = TcpStream::connect(&text_address)?;
    let mut log = CsvLog::open(&csv_path)?;
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
            true,
            [0, 0, 0, call.requested_callee_line],
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

    let utterance = player_utterance(&player_command, &call, revision, &turns)?;
    turns.push(Turn {
        speaker: "player".into(),
        text: utterance.clone(),
    });
    let text_response = send_text(
        &mut text,
        TextInputMessage {
            protocol_version: TEXT_PROTOCOL_VERSION,
            session_id: 1,
            turn_id: 1,
            state_revision: revision,
            text: utterance.clone(),
        },
    )?;
    let response_text = text_response.response_text.clone().unwrap_or_default();
    if text_response.status == TextStatus::Completed {
        turns.push(Turn {
            speaker: "subscriber".into(),
            text: response_text.clone(),
        });
    }
    log.row(CsvRow {
        event: "text_turn",
        sequence,
        revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status: &format!("{:?}", text_response.status),
        text: &format!("player={utterance} | subscriber={response_text}"),
    })?;
    if text_response.status != TextStatus::Completed {
        return Err(format!("text turn failed: {:?}", text_response.error).into());
    }

    state = exchange(
        &mut backend,
        input(
            &mut sequence,
            revision,
            vec![
                cord(PortId::Subscriber(call.caller_line), PortId::Operator),
                cord(
                    PortId::Subscriber(call.requested_callee_line),
                    PortId::RingGenerator,
                ),
            ],
            true,
            [0, 0, 0, call.requested_callee_line],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "ring",
        sequence,
        revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status: "accepted",
        text: "destination ringing",
    })?;
    state = exchange(
        &mut backend,
        input(
            &mut sequence,
            revision,
            vec![cord(
                PortId::Subscriber(call.caller_line),
                PortId::Subscriber(call.requested_callee_line),
            )],
            true,
            [0, 0, 0, call.requested_callee_line],
        ),
    )?;
    revision = state.state_revision;
    log.row(CsvRow {
        event: "route",
        sequence,
        revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status: "accepted",
        text: "direct circuit connected",
    })?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    state = exchange(
        &mut backend,
        input(&mut sequence, revision, vec![], false, [0, 0, 0, 1]),
    )?;
    let finished = state
        .output
        .calls
        .iter()
        .all(|active| active.caller_line != call.caller_line);
    let status = if finished { "accepted" } else { "failed" };
    log.row(CsvRow {
        event: "finish",
        sequence,
        revision: state.state_revision,
        caller: call.caller_line,
        destination: call.requested_callee_line,
        status,
        text: "call completion checked",
    })?;
    if !finished {
        return Err("backend did not complete the call".into());
    }
    println!("test completed; log written to {csv_path}");
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

fn frame_io(error: exchange_protocol::FrameError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn player_utterance(
    command: &Option<String>,
    call: &exchange_protocol::CallStatus,
    revision: u64,
    turns: &[Turn],
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(command) = command {
        let prompt = serde_json::to_vec(&PlayerPrompt {
            caller_line: call.caller_line,
            destination_line: call.requested_callee_line,
            state_revision: revision,
            conversation: turns,
        })?;
        let mut child = Command::new(command)
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
