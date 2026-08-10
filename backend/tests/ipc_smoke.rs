use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

struct RunningCore(Child);

impl Drop for RunningCore {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn connect_to_core(port: u16) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => return stream,
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("MVP core did not accept a loopback connection: {error}"),
        }
    }
}

fn request(port: u16, snapshot: &str) -> String {
    let mut stream = connect_to_core(port);
    stream.write_all(snapshot.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).unwrap();
    response
}

#[test]
fn loopback_protocol_returns_backend_state_and_accepts_a_reset_on_a_new_connection() {
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("reserve a test loopback port")
        .local_addr()
        .expect("read reserved test port")
        .port();
    let binary = env!("CARGO_BIN_EXE_backend");
    let _core = RunningCore(
        Command::new(binary)
            .env("NN_MVP_BACKEND_PORT", port.to_string())
            .spawn()
            .expect("start MVP core"),
    );

    let initial = request(
        port,
        r#"{"sequence":1,"cords":[],"active_action":-1,"crank_complete":false,"directory_id":4101,"speaker_enabled":true,"reset":false}"#,
    );
    assert!(initial.contains(r#""sequence":1"#));
    assert!(initial.contains(r#""phase":"INCOMING CALLER""#));
    assert!(initial.contains("NILA DAS"));

    let reset = request(
        port,
        r#"{"sequence":2,"cords":[],"active_action":-1,"crank_complete":false,"directory_id":9999,"speaker_enabled":true,"reset":true}"#,
    );
    assert!(reset.contains(r#""sequence":2"#));
    assert!(reset.contains(r#""reset_status":"RESET COMPLETE""#));
    assert!(reset.contains("NO RECORD"));
}
