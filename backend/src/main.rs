use backend::{CabinetSnapshot, Cord, MvpCore};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const CLIENT_TIMEOUT: Duration = Duration::from_millis(250);

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:48129")?;
    eprintln!("North Neeladesh MVP core listening on 127.0.0.1:48129 (offline only)");
    let mut core = MvpCore::new();
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_client(stream, &mut core) {
                    eprintln!("cabinet request failed: {error}");
                }
            }
            Err(error) => eprintln!("cabinet connection failed: {error}"),
        }
    }
    Ok(())
}

fn handle_client(stream: TcpStream, core: &mut MvpCore) -> std::io::Result<()> {
    stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let input = parse_snapshot(&line).unwrap_or_else(|| {
        eprintln!("[cabinet] malformed snapshot: {line:?}");
        CabinetSnapshot {
            sequence: 0,
            cords: Vec::new(),
            active_action: -1,
            crank_complete: false,
            directory_id: 0,
            speaker_enabled: true,
            reset: false,
        }
    });
    eprintln!(
        "[cabinet] input sequence={} cords={:?} action={} crank_complete={} directory_id={} speaker_enabled={} reset={}",
        input.sequence,
        input.cords,
        input.active_action,
        input.crank_complete,
        input.directory_id,
        input.speaker_enabled,
        input.reset,
    );
    let output = core.apply(input);
    eprintln!(
        "[cabinet] output sequence={} phase={} lamps={:?} reset_status={} printer_lines={} latest_printer={:?}",
        output.sequence,
        output.phase.label(),
        output.line_lamps,
        output.reset_status,
        output.printer.len(),
        output.printer.last(),
    );
    let lamps = output
        .line_lamps
        .map(|lit| if lit { "true" } else { "false" })
        .join(",");
    let directory = output.directory.join("|");
    let printer = output.printer.join("|");
    writeln!(
        &mut stream.try_clone()?,
        "{{\"sequence\":{},\"clock_minutes\":{},\"phase\":\"{}\",\"reset_status\":\"{}\",\"routing_status\":\"{}\",\"line_lamps\":[{}],\"directory\":\"{}\",\"printer\":\"{}\",\"monitor_active\":{},\"speaker_active\":{}}}",
        output.sequence,
        output.clock_minutes,
        output.phase.label(),
        output.reset_status,
        output.routing_status,
        lamps,
        escape(&directory),
        escape(&printer),
        output.monitor_active,
        output.speaker_active,
    )
}

fn parse_snapshot(line: &str) -> Option<CabinetSnapshot> {
    Some(CabinetSnapshot {
        sequence: number(line, "sequence")? as u64,
        cords: cords(line),
        active_action: number(line, "active_action").unwrap_or(-1),
        crank_complete: boolean(line, "crank_complete"),
        directory_id: number(line, "directory_id").unwrap_or(0) as u16,
        speaker_enabled: boolean(line, "speaker_enabled"),
        reset: boolean(line, "reset"),
    })
}

fn number(line: &str, key: &str) -> Option<i32> {
    let suffix = line.split_once(&format!("\"{key}\":"))?.1;
    let end = suffix.find(|character: char| !character.is_ascii_digit() && character != '-')?;
    suffix[..end].parse().ok()
}

fn boolean(line: &str, key: &str) -> bool {
    line.split_once(&format!("\"{key}\":"))
        .is_some_and(|(_, suffix)| suffix.starts_with("true"))
}

fn cords(line: &str) -> Vec<Cord> {
    let Some((_, suffix)) = line.split_once("\"cords\":[") else {
        return Vec::new();
    };
    let Some((body, _)) = suffix.split_once("]]") else {
        return Vec::new();
    };
    body.split("],[")
        .filter_map(|pair| {
            let pair = pair.trim_matches(|character| character == '[' || character == ']');
            let (left, right) = pair.split_once(',')?;
            Some(Cord(left.parse().ok()?, right.parse().ok()?))
        })
        .collect()
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
