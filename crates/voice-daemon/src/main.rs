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
    let mut capture: Box<dyn MicrophoneCapture> = match env::var("NN_VOICE_CAPTURE_COMMAND") {
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
    capture.prepare()?;
    let mut relay = RelaySession {
        udp,
        capture,
        playback: &mut playback,
        session_id: 1,
        turn_id: 1,
        state_revision: 0,
        audio_packets_received: 0,
        audio_samples_received: 0,
        last_audio_sequence: None,
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
        log_voice_event(format_args!(
            "voice rx datagram bytes={} session={} turn={} revision={}",
            datagram.len(),
            relay.session_id,
            relay.turn_id,
            relay.state_revision
        ));
        if let Ok(control) = exchange_protocol::decode_voice_control(&datagram) {
            log_voice_event(format_args!(
                "voice rx control session={} turn={} revision={} control={:?}",
                control.session_id, control.turn_id, control.state_revision, control.control
            ));
            if control.protocol_version == VOICE_PROTOCOL_VERSION
                && control.session_id == relay.session_id
            {
                relay.handle_control(control)?;
            } else {
                log_voice_event("voice rx control ignored: session or protocol mismatch");
            }
            continue;
        }
        if let Ok(status) = decode_voice_status(&datagram) {
            relay.handle_status(status)?;
            continue;
        }
        if let Ok(packet) = RtpL16Packet::decode(&datagram) {
            relay.handle_audio(packet)?;
            continue;
        }
        log_voice_event("voice rx datagram ignored: unknown protocol payload");
    }
}

struct RelaySession<'a> {
    udp: UdpVoiceOutput,
    capture: Box<dyn MicrophoneCapture>,
    playback: &'a mut Box<dyn AudioPlayback>,
    session_id: u64,
    turn_id: u64,
    state_revision: u64,
    audio_packets_received: u64,
    audio_samples_received: u64,
    last_audio_sequence: Option<u16>,
}

impl RelaySession<'_> {
    fn send_status(
        &mut self,
        status: VoiceStatus,
        error: Option<exchange_protocol::ProtocolError>,
    ) -> Result<(), VoiceError> {
        log_voice_event(format_args!(
            "voice tx status session={} turn={} revision={} status={status:?}",
            self.session_id, self.turn_id, self.state_revision
        ));
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
            VoiceControl::StartPtt => {
                log_voice_event(format_args!(
                    "voice capture start session={} turn={} revision={}",
                    self.session_id, self.turn_id, self.state_revision
                ));
                match self.capture.start() {
                    Ok(()) => self.send_status(VoiceStatus::Listening, None),
                    Err(error) => {
                        log_voice_event(format_args!(
                            "voice capture start failed session={} turn={} code={} message={}",
                            self.session_id, self.turn_id, error.code, error.message
                        ));
                        self.send_status(
                            VoiceStatus::Failed,
                            Some(exchange_protocol::ProtocolError {
                                code: error.code,
                                message: error.message,
                            }),
                        )
                    }
                }
            }
            VoiceControl::ReleasePtt => {
                log_voice_event(format_args!(
                    "voice capture release session={} turn={} revision={}",
                    self.session_id, self.turn_id, self.state_revision
                ));
                match self.capture.finish() {
                    Ok(samples) => {
                        log_voice_event(format_args!(
                            "voice capture complete session={} turn={} samples={}",
                            self.session_id,
                            self.turn_id,
                            samples.len()
                        ));
                        self.send_input_audio(&samples)
                    }
                    Err(error) if error.code == "capture_not_started" => {
                        log_voice_event(format_args!(
                            "voice capture release ignored session={} turn={} revision={} because capture was not active",
                            self.session_id, self.turn_id, self.state_revision
                        ));
                        self.send_status(VoiceStatus::Ready, None)
                    }
                    Err(error) => {
                        log_voice_event(format_args!(
                            "voice capture failed session={} turn={} code={} message={}",
                            self.session_id, self.turn_id, error.code, error.message
                        ));
                        self.send_status(
                            VoiceStatus::Failed,
                            Some(exchange_protocol::ProtocolError {
                                code: error.code,
                                message: error.message,
                            }),
                        )
                    }
                }
            }
            VoiceControl::Cancel => {
                log_voice_event(format_args!(
                    "voice capture cancel session={} turn={} revision={}",
                    self.session_id, self.turn_id, self.state_revision
                ));
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
            log_voice_event(format_args!(
                "voice tx input_audio session={} turn={} revision={} chunk=0 complete=true samples=0",
                self.session_id, self.turn_id, self.state_revision
            ));
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
            log_voice_event(format_args!(
                "voice tx input_audio session={} turn={} revision={} chunk={} complete={} samples={}",
                self.session_id,
                self.turn_id,
                self.state_revision,
                index,
                index + 1 == chunks.len(),
                chunk.len()
            ));
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
        log_voice_event(format_args!(
            "voice rx status session={} turn={} revision={} status={:?} error={}",
            status.session_id,
            status.turn_id,
            status.state_revision,
            status.status,
            status
                .error
                .as_ref()
                .map_or("none", |error| error.code.as_str())
        ));
        match status.status {
            VoiceStatus::Completed | VoiceStatus::Failed | VoiceStatus::Cancelled => {
                log_voice_event(format_args!(
                    "voice playback finish session={} turn={} packets={} samples={}",
                    self.session_id,
                    self.turn_id,
                    self.audio_packets_received,
                    self.audio_samples_received
                ));
                self.playback.finish()?;
                log_voice_event(format_args!(
                    "voice playback finished session={} turn={}",
                    self.session_id, self.turn_id
                ));
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_audio(&mut self, packet: RtpL16Packet) -> Result<(), VoiceError> {
        if let Some(previous) = self.last_audio_sequence {
            let expected = previous.wrapping_add(1);
            if packet.sequence != expected {
                log_voice_event(format_args!(
                    "voice rx audio gap session={} turn={} expected_seq={} actual_seq={} previous_seq={}",
                    self.session_id, self.turn_id, expected, packet.sequence, previous
                ));
            }
        }
        self.last_audio_sequence = Some(packet.sequence);
        self.audio_packets_received += 1;
        self.audio_samples_received += packet.samples.len() as u64;
        log_voice_event(format_args!(
            "voice rx audio session={} turn={} seq={} timestamp={} ssrc={} marker={} samples={} total_packets={} total_samples={}",
            self.session_id,
            self.turn_id,
            packet.sequence,
            packet.timestamp,
            packet.ssrc,
            packet.marker,
            packet.samples.len(),
            self.audio_packets_received,
            self.audio_samples_received
        ));
        self.playback.write(&packet.samples)?;
        log_voice_event(format_args!(
            "voice playback write session={} turn={} seq={} samples={}",
            self.session_id,
            self.turn_id,
            packet.sequence,
            packet.samples.len()
        ));
        Ok(())
    }
}

fn env_value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}
