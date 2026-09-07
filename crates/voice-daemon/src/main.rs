use std::env;
use std::error::Error;
use std::thread;
use std::time::Duration;

use exchange_protocol::{
    RtpL16Packet, VOICE_INPUT_AUDIO_PACKET_SAMPLES, VOICE_INPUT_SAMPLE_RATE,
    VOICE_PROTOCOL_VERSION, VoiceControl, VoiceInputAudioMessage, VoiceStatus, VoiceStatusMessage,
    decode_voice_status,
};
use exchange_voice_daemon::{
    AudioPlayback, CommandAudioPlayback, CommandMicrophone, CommandSpec, CpalAudioPlayback,
    CpalMicrophone, MicrophoneCapture, UdpVoiceOutput, VoiceError, VoiceOutput, log_voice_event,
};

fn main() -> Result<(), Box<dyn Error>> {
    run_relay_only()
}

fn run_relay_only() -> Result<(), Box<dyn Error>> {
    loop {
        if let Err(error) = run_relay_connection() {
            log_voice_event(format_args!("voice relay recovering after error: {error}"));
            thread::sleep(Duration::from_secs(1));
        }
    }
}

fn run_relay_connection() -> Result<(), Box<dyn Error>> {
    let backend = env_value("NN_VOICE_BACKEND_ADDRESS", "127.0.0.1:7879");
    let udp = UdpVoiceOutput::connect(&backend, 1, 0)?;
    let receiver = udp.try_clone()?;
    receiver.set_receive_timeout(Duration::from_secs(5))?;
    let capture: Box<dyn MicrophoneCapture> = match env::var("NN_VOICE_CAPTURE_COMMAND") {
        Ok(command) if command.trim().is_empty() => {
            Box::new(CpalMicrophone::new(VOICE_INPUT_SAMPLE_RATE as usize * 15)?)
        }
        Ok(command) => Box::new(CommandMicrophone::new(CommandSpec::from_words(&command)?)),
        Err(_) => Box::new(CpalMicrophone::new(VOICE_INPUT_SAMPLE_RATE as usize * 15)?),
    };
    let mut playback: Box<dyn AudioPlayback> = match env::var("NN_VOICE_PLAYBACK_COMMAND") {
        Ok(command) if command.trim().is_empty() => {
            Box::new(CpalAudioPlayback::new(Duration::from_secs(30))?)
        }
        Ok(command) => Box::new(CommandAudioPlayback::new(CommandSpec::from_words(
            &command,
        )?)?),
        Err(_) => Box::new(CpalAudioPlayback::new(Duration::from_secs(30))?),
    };
    let mut relay = RelaySession {
        udp,
        capture,
        playback: &mut playback,
        session_id: 1,
        turn_id: 1,
        state_revision: 0,
    };
    relay.send_status(VoiceStatus::Ready, None)?;
    log_voice_event("voice relay ready; backend owns STT, dialogue, and Qwen3-TTS");
    loop {
        let datagram = match receiver.receive_datagram() {
            Ok(datagram) => datagram,
            Err(error) if error.code == "voice_datagram_timeout" => {
                relay.send_status(VoiceStatus::Ready, None)?;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        if let Ok(control) = exchange_protocol::decode_voice_control(&datagram) {
            if control.protocol_version == VOICE_PROTOCOL_VERSION
                && control.session_id == relay.session_id
            {
                relay.handle_control(control)?;
            }
            continue;
        }
        if let Ok(status) = decode_voice_status(&datagram) {
            relay.handle_status(status)?;
            continue;
        }
        if let Ok(packet) = RtpL16Packet::decode(&datagram) {
            relay.playback.write(&packet.samples)?;
        }
    }
}

struct RelaySession<'a> {
    udp: UdpVoiceOutput,
    capture: Box<dyn MicrophoneCapture>,
    playback: &'a mut Box<dyn AudioPlayback>,
    session_id: u64,
    turn_id: u64,
    state_revision: u64,
}

impl RelaySession<'_> {
    fn send_status(
        &mut self,
        status: VoiceStatus,
        error: Option<exchange_protocol::ProtocolError>,
    ) -> Result<(), VoiceError> {
        self.udp.status(VoiceStatusMessage {
            protocol_version: VOICE_PROTOCOL_VERSION,
            session_id: self.session_id,
            turn_id: self.turn_id,
            state_revision: self.state_revision,
            status,
            transcript: None,
            response_text: None,
            error,
        })
    }

    fn handle_control(
        &mut self,
        control: exchange_protocol::VoiceControlMessage,
    ) -> Result<(), VoiceError> {
        self.turn_id = control.turn_id;
        self.state_revision = control.state_revision;
        match control.control {
            VoiceControl::StartPtt => match self.capture.start() {
                Ok(()) => self.send_status(VoiceStatus::Listening, None),
                Err(error) => self.send_status(
                    VoiceStatus::Failed,
                    Some(exchange_protocol::ProtocolError {
                        code: error.code,
                        message: error.message,
                    }),
                ),
            },
            VoiceControl::ReleasePtt => match self.capture.finish() {
                Ok(samples) => self.send_input_audio(&samples),
                Err(error) => self.send_status(
                    VoiceStatus::Failed,
                    Some(exchange_protocol::ProtocolError {
                        code: error.code,
                        message: error.message,
                    }),
                ),
            },
            VoiceControl::Cancel => {
                let _ = self.capture.finish();
                self.send_status(VoiceStatus::Cancelled, None)
            }
        }
    }

    fn send_input_audio(&self, samples: &[i16]) -> Result<(), VoiceError> {
        let chunks = samples
            .chunks(VOICE_INPUT_AUDIO_PACKET_SAMPLES)
            .collect::<Vec<_>>();
        if chunks.is_empty() {
            return self.udp.send_input_audio(&VoiceInputAudioMessage {
                protocol_version: VOICE_PROTOCOL_VERSION,
                session_id: self.session_id,
                turn_id: self.turn_id,
                state_revision: self.state_revision,
                chunk_index: 0,
                complete: true,
                samples: Vec::new(),
            });
        }
        for (index, chunk) in chunks.iter().enumerate() {
            self.udp.send_input_audio(&VoiceInputAudioMessage {
                protocol_version: VOICE_PROTOCOL_VERSION,
                session_id: self.session_id,
                turn_id: self.turn_id,
                state_revision: self.state_revision,
                chunk_index: index as u32,
                complete: index + 1 == chunks.len(),
                samples: chunk.to_vec(),
            })?;
        }
        Ok(())
    }

    fn handle_status(&mut self, status: VoiceStatusMessage) -> Result<(), VoiceError> {
        self.state_revision = status.state_revision;
        match status.status {
            VoiceStatus::Completed | VoiceStatus::Failed | VoiceStatus::Cancelled => {
                self.playback.finish()?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn env_value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}
