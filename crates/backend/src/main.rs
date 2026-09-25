use std::env;
use std::io;
use std::net::{SocketAddr, TcpListener, UdpSocket};

fn main() -> io::Result<()> {
    let bind = argument_value("--bind").unwrap_or_else(|| "0.0.0.0:7878".to_string());
    let voice_bind = argument_value("--voice-bind").unwrap_or_else(|| "0.0.0.0:7879".to_string());
    let voice_upload_bind = argument_value("--voice-upload-bind").unwrap_or_else(|| "0.0.0.0:7883".to_string());
    let debug_bind = argument_value("--debug-bind");
    let text_bind = argument_value("--text-bind");
    let listener = TcpListener::bind(&bind)?;
    let voice_socket = UdpSocket::bind(&voice_bind)?;
    let voice_upload_listener = TcpListener::bind(&voice_upload_bind)?;
    let voice_port = voice_bind
        .parse::<SocketAddr>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
        .port();
    let debug_listener = debug_bind
        .as_deref()
        .map(bind_loopback_listener)
        .transpose()?;
    let text_listener = text_bind
        .as_deref()
        .map(bind_loopback_listener)
        .transpose()?;
    println!(
        "[BACKEND] game={bind} voice={voice_bind} voice_upload={voice_upload_bind} debug={:?} text={:?}",
        debug_bind, text_bind
    );
    exchange_backend::serve_with_voice_debug_and_text(
        listener,
        Some(voice_socket),
        Some(voice_upload_listener),
        SocketAddr::from(([127, 0, 0, 1], voice_port)),
        debug_listener,
        text_listener,
    )
}

fn bind_loopback_listener(address: &str) -> io::Result<TcpListener> {
    let address: SocketAddr = address.parse().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid debug bind address {address}: {error}"),
        )
    })?;
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "debug command server must bind to a loopback address",
        ));
    }
    TcpListener::bind(address)
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
    use super::bind_loopback_listener;

    #[test]
    fn debug_listener_rejects_non_loopback_addresses() {
        assert!(bind_loopback_listener("0.0.0.0:0").is_err());
        assert!(bind_loopback_listener("127.0.0.1:0").is_ok());
    }
}
