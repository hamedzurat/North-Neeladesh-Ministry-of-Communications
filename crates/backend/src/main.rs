use std::env;
use std::io;
use std::net::{SocketAddr, TcpListener, UdpSocket};

fn main() -> io::Result<()> {
    let bind = argument_value("--bind").unwrap_or_else(|| "127.0.0.1:7878".to_string());
    let voice_bind = argument_value("--voice-bind").unwrap_or_else(|| "127.0.0.1:7879".to_string());
    let debug_bind = argument_value("--debug-bind");
    let listener = TcpListener::bind(&bind)?;
    let voice_socket = UdpSocket::bind(&voice_bind)?;
    let debug_listener = debug_bind
        .as_deref()
        .map(bind_loopback_listener)
        .transpose()?;
    exchange_backend::serve_with_voice_and_debug(listener, Some(voice_socket), debug_listener)
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
