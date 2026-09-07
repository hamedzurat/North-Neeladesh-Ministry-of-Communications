use std::env;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use exchange_protocol::{
    DebugCommand, DebugRequest, DebugResponse, PROTOCOL_VERSION, read_frame, write_frame,
};
use serde::Deserialize;

const INDEX_HTML: &str = include_str!("../static/index.html");

fn main() -> io::Result<()> {
    let bind = argument_value("--bind").unwrap_or_else(|| "127.0.0.1:7881".to_string());
    let backend = argument_value("--backend").unwrap_or_else(|| "127.0.0.1:7880".to_string());
    let listener = TcpListener::bind(&bind)?;
    println!("development debug UI listening on http://{bind}");
    println!(
        "manual check: advance time, inject a Call, force an Event, enable godmode, then reset"
    );
    for connection in listener.incoming() {
        let stream = connection?;
        let backend = backend.clone();
        thread::spawn(move || {
            if let Err(error) = handle_http(stream, &backend) {
                eprintln!("debug UI request failed: {error}");
            }
        });
    }
    Ok(())
}

fn handle_http(mut stream: TcpStream, backend: &str) -> io::Result<()> {
    let request = read_http_request(&mut stream)?;
    let response = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => http_response("text/html; charset=utf-8", INDEX_HTML.as_bytes()),
        ("GET", "/api/snapshot") => match debug_command(backend, DebugCommand::Snapshot) {
            Ok(response) => json_response(&response.snapshot),
            Err(error) => http_error(502, &error.to_string()),
        },
        ("POST", "/api/command") => match serde_json::from_slice::<UiCommand>(&request.body)
            .and_then(|command| {
                command.into_debug_command().map_err(|error| {
                    serde_json::Error::io(io::Error::new(io::ErrorKind::InvalidInput, error))
                })
            }) {
            Ok(command) => match debug_command(backend, command) {
                Ok(response) => json_response(&response),
                Err(error) => http_error(502, &error.to_string()),
            },
            Err(error) => http_error(400, &error.to_string()),
        },
        _ => http_error(404, "not found"),
    };
    stream.write_all(&response)?;
    stream.flush()
}

fn debug_command(backend: &str, command: DebugCommand) -> io::Result<DebugResponse> {
    let mut stream = TcpStream::connect(backend)?;
    write_frame(
        &mut stream,
        &DebugRequest {
            protocol_version: PROTOCOL_VERSION,
            command,
        },
    )
    .map_err(|error| io::Error::other(error.to_string()))?;
    read_frame(&mut stream).map_err(|error| io::Error::other(error.to_string()))
}

#[derive(Debug, Deserialize)]
struct UiCommand {
    action: String,
    seconds: Option<u32>,
    caller_line: Option<u8>,
    callee_line: Option<u8>,
    id: Option<String>,
    enabled: Option<bool>,
}

impl UiCommand {
    fn into_debug_command(self) -> Result<DebugCommand, String> {
        match self.action.as_str() {
            "snapshot" => Ok(DebugCommand::Snapshot),
            "reset" => Ok(DebugCommand::ResetRun),
            "advance_time" => Ok(DebugCommand::AdvanceTime {
                seconds: self.seconds.ok_or("seconds is required")?,
            }),
            "inject_call" => Ok(DebugCommand::InjectCall {
                caller_line: self.caller_line.ok_or("caller_line is required")?,
                callee_line: self.callee_line.ok_or("callee_line is required")?,
            }),
            "force_story_event" => Ok(DebugCommand::ForceStoryEvent {
                event_id: self.id.ok_or("id is required")?,
            }),
            "select_story_path" => Ok(DebugCommand::SelectStoryPath {
                node_id: self.id.ok_or("id is required")?,
            }),
            "godmode" => Ok(DebugCommand::SetGodmode {
                enabled: self.enabled.ok_or("enabled is required")?,
            }),
            "bypass_restrictions" => Ok(DebugCommand::SetBypassRestrictions {
                enabled: self.enabled.ok_or("enabled is required")?,
            }),
            action => Err(format!("unknown action {action}")),
        }
    }
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn read_http_request(stream: &mut TcpStream) -> io::Result<HttpRequest> {
    let mut bytes = Vec::new();
    let header_end;
    loop {
        let mut chunk = [0_u8; 4096];
        let length = stream.read(&mut chunk)?;
        if length == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "empty HTTP request",
            ));
        }
        bytes.extend_from_slice(&chunk[..length]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            header_end = end + 4;
            break;
        }
        if bytes.len() > 64 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP headers too large",
            ));
        }
    }
    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "HTTP headers are not UTF-8"))?;
    let mut lines = headers.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP request line"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_string();
    let path = request_parts.next().unwrap_or_default().to_string();
    let content_length = lines
        .find_map(|line| line.strip_prefix("Content-Length:").map(str::trim))
        .unwrap_or("0")
        .parse::<usize>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length"))?;
    let mut body = bytes[header_end..].to_vec();
    while body.len() < content_length {
        let mut chunk = [0_u8; 4096];
        let length = stream.read(&mut chunk)?;
        if length == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated HTTP body",
            ));
        }
        body.extend_from_slice(&chunk[..length]);
    }
    body.truncate(content_length);
    Ok(HttpRequest { method, path, body })
}

fn http_response(content_type: &str, body: &[u8]) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
        body.len()
    )
    .into_bytes()
    .into_iter()
    .chain(body.iter().copied())
    .collect()
}

fn json_response<T: serde::Serialize>(value: &T) -> Vec<u8> {
    match serde_json::to_vec(value) {
        Ok(body) => http_response("application/json", &body),
        Err(error) => http_error(500, &error.to_string()),
    }
}

fn http_error(status: u16, message: &str) -> Vec<u8> {
    let body = serde_json::json!({ "error": message }).to_string();
    let reason = match status {
        400 => "Bad Request",
        404 => "Not Found",
        502 => "Bad Gateway",
        _ => "Internal Server Error",
    };
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn argument_value(name: &str) -> Option<String> {
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == name {
            return arguments.next();
        }
    }
    None
}
