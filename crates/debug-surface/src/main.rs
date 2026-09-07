use std::env;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;

use exchange_protocol::{
    DEBUG_PROTOCOL_VERSION, DebugAudioKind, DebugCommand, DebugRequest, DebugResponse, read_frame,
    write_frame,
};
use serde::Deserialize;

const INDEX_HTML: &str = include_str!("../static/index.html");

fn main() -> io::Result<()> {
    let bind = argument_value("--bind").unwrap_or_else(|| "127.0.0.1:7881".to_string());
    let backend = argument_value("--backend").unwrap_or_else(|| "127.0.0.1:7880".to_string());
    let listener = bind_loopback_listener(&bind)?;
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

fn bind_loopback_listener(address: &str) -> io::Result<TcpListener> {
    let address: SocketAddr = address.parse().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid debug UI bind address {address}: {error}"),
        )
    })?;
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "debug UI must bind to a loopback address",
        ));
    }
    TcpListener::bind(address)
}

fn handle_http(mut stream: TcpStream, backend: &str) -> io::Result<()> {
    let request = read_http_request(&mut stream)?;
    let response = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => http_response("text/html; charset=utf-8", INDEX_HTML.as_bytes()),
        ("GET", "/api/snapshot") => match debug_command(backend, DebugCommand::Snapshot) {
            Ok(response) => json_response(&response.snapshot),
            Err(error) => http_error(502, &error.to_string()),
        },
        ("GET", path) if path.starts_with("/api/voice/") => voice_audio_response(backend, path),
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

fn voice_audio_response(backend: &str, path: &str) -> Vec<u8> {
    let Some((conversation_id, kind)) = parse_voice_audio_path(path) else {
        return http_error(
            404,
            "voice audio path must be /api/voice/{id}/capture.wav or /api/voice/{id}/tts.wav",
        );
    };
    match debug_command(
        backend,
        DebugCommand::GetVoiceAudio {
            conversation_id,
            kind,
        },
    ) {
        Ok(response) if response.accepted => response
            .audio
            .map(|audio| http_audio_response("audio/wav", &wav_bytes(&audio)))
            .unwrap_or_else(|| http_error(404, "voice audio is unavailable")),
        Ok(response) => http_error(
            404,
            &response.error.map_or_else(
                || "voice audio is unavailable".to_string(),
                |error| error.message,
            ),
        ),
        Err(error) => http_error(502, &error.to_string()),
    }
}

fn parse_voice_audio_path(path: &str) -> Option<(u64, DebugAudioKind)> {
    let mut parts = path.strip_prefix("/api/voice/")?.split('/');
    let conversation_id = parts.next()?.parse().ok()?;
    let kind = match parts.next()? {
        "capture.wav" => DebugAudioKind::Capture,
        "tts.wav" => DebugAudioKind::Tts,
        _ => return None,
    };
    (parts.next().is_none()).then_some((conversation_id, kind))
}

fn wav_bytes(audio: &exchange_protocol::DebugAudio) -> Vec<u8> {
    let data_length = (audio.samples.len() * std::mem::size_of::<i16>()) as u32;
    let block_align = audio.channels * 2;
    let byte_rate = audio.sample_rate * u32::from(block_align);
    let mut bytes = Vec::with_capacity(44 + data_length as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&u16::from(audio.channels).to_le_bytes());
    bytes.extend_from_slice(&audio.sample_rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&u16::from(block_align).to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_length.to_le_bytes());
    for sample in &audio.samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn debug_command(backend: &str, command: DebugCommand) -> io::Result<DebugResponse> {
    let mut stream = TcpStream::connect(backend)?;
    write_frame(
        &mut stream,
        &DebugRequest {
            protocol_version: DEBUG_PROTOCOL_VERSION,
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

fn http_audio_response(content_type: &str, body: &[u8]) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nContent-Disposition: inline\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_audio_paths_are_strictly_parsed() {
        assert_eq!(
            parse_voice_audio_path("/api/voice/12/capture.wav"),
            Some((12, DebugAudioKind::Capture))
        );
        assert_eq!(
            parse_voice_audio_path("/api/voice/12/tts.wav"),
            Some((12, DebugAudioKind::Tts))
        );
        assert_eq!(parse_voice_audio_path("/api/voice/12/other.wav"), None);
    }

    #[test]
    fn debug_ui_rejects_non_loopback_addresses() {
        assert!(bind_loopback_listener("0.0.0.0:0").is_err());
        assert!(bind_loopback_listener("127.0.0.1:0").is_ok());
    }

    #[test]
    fn wav_output_has_pcm_header_and_samples() {
        let bytes = wav_bytes(&exchange_protocol::DebugAudio {
            sample_rate: 24_000,
            channels: 1,
            samples: vec![1, -2],
        });

        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(&bytes[44..], &[1, 0, 254, 255]);
    }
}
