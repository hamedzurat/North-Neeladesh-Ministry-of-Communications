use std::env;
use std::io;
use std::net::TcpListener;

fn main() -> io::Result<()> {
    let bind = argument_value("--bind").unwrap_or_else(|| "127.0.0.1:7878".to_string());
    let listener = TcpListener::bind(&bind)?;
    println!("exchange backend listening on {bind}");
    println!("manual check: connect Caller, ring with the Ring Generator, then route directly");
    exchange_backend::serve(listener)
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
