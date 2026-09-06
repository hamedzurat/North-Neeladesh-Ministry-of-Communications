use std::env;
use std::error::Error;
use std::io::{self, BufRead};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use exchange_protocol::{
    VOICE_INPUT_SAMPLE_RATE, VOICE_PROTOCOL_VERSION, VoiceControl, VoiceStatusMessage,
};
use exchange_voice_daemon::{
    CommandAudioPlayback, CommandDialogueGenerator, CommandMicrophone, CommandSpec,
    CommandSpeechToText, CpalAudioPlayback, CpalMicrophone, KnowledgeRecord, MicrophoneCapture,
    OperatorSession, PersistentQwen3TtsCommand, Qwen3TtsCommand, RelationshipNote, ResponseContext,
    SubscriberProfile, TeeVoiceOutput, TextToSpeech, UdpVoiceOutput, VoiceOutput, log_voice_event,
    request_worker_cancellation,
};

fn main() -> Result<(), Box<dyn Error>> {
    let backend = env_value("NN_VOICE_BACKEND_ADDRESS", "127.0.0.1:7879");
    let stt = command("NN_VOICE_STT_COMMAND")?;
    let dialogue = command("NN_VOICE_DIALOGUE_COMMAND")?;
    let tts = command("NN_VOICE_TTS_COMMAND")?;
    let udp_output = UdpVoiceOutput::connect(&backend, 1, 0)?;
    let control_socket = udp_output.try_clone()?;
    let capture: Box<dyn MicrophoneCapture> = match env::var("NN_VOICE_CAPTURE_COMMAND") {
        Ok(command) => Box::new(CommandMicrophone::new(CommandSpec::from_words(&command)?)),
        Err(_) => Box::new(CpalMicrophone::new(VOICE_INPUT_SAMPLE_RATE as usize * 15)?),
    };
    let output: Box<dyn VoiceOutput> = match env::var("NN_VOICE_PLAYBACK_COMMAND") {
        Ok(command) => Box::new(TeeVoiceOutput::new(
            udp_output,
            Box::new(CommandAudioPlayback::new(CommandSpec::from_words(
                &command,
            )?)?),
        )),
        Err(_) => Box::new(TeeVoiceOutput::new(
            udp_output,
            Box::new(CpalAudioPlayback::new(Duration::from_secs(30))?),
        )),
    };
    let output: Box<dyn VoiceOutput> = Box::new(ConsoleVoiceOutput { inner: output });
    let tts: Box<dyn TextToSpeech> = if env::var_os("NN_VOICE_TTS_PERSISTENT").is_some() {
        Box::new(PersistentQwen3TtsCommand::new(tts)?)
    } else {
        Box::new(Qwen3TtsCommand::new(tts))
    };
    let context = demo_context();
    log_voice_event(format_args!(
        "subscriber voice: {}",
        context.profile.voice_id
    ));
    let mut session = OperatorSession::new(
        1,
        0,
        context,
        capture,
        Box::new(CommandSpeechToText::new(stt)),
        Box::new(CommandDialogueGenerator::new(dialogue)),
        tts,
        output,
    )?;

    session.announce_ready()?;
    log_voice_event("voice daemon ready; type ptt, release, or quit");
    let (commands, receiver) = mpsc::channel();
    let (controls, control_receiver) = mpsc::channel();
    let control_monitor = control_socket.try_clone()?;
    thread::spawn(move || {
        loop {
            let Ok(control) = control_monitor.receive_control() else {
                break;
            };
            if control.protocol_version == VOICE_PROTOCOL_VERSION && control.session_id == 1 {
                if control.control == VoiceControl::Cancel {
                    request_worker_cancellation();
                }
                if controls.send(control).is_err() {
                    break;
                }
            }
        }
    });
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            if let Ok(line) = &line
                && line.trim() == "cancel"
            {
                request_worker_cancellation();
            }
            if commands.send(line).is_err() {
                break;
            }
        }
    });
    loop {
        if let Ok(control) = control_receiver.try_recv()
            && control.protocol_version == VOICE_PROTOCOL_VERSION
            && control.session_id == 1
        {
            session.set_state_revision(control.state_revision);
            match control.control {
                VoiceControl::StartPtt => {
                    log_voice_event(format_args!("voice request speaker: {}", control.voice_id));
                    session.set_voice_id(&control.voice_id)?;
                    session.start_ptt()?;
                }
                VoiceControl::ReleasePtt => {
                    match session.release_ptt() {
                        Ok(response) => log_voice_event(format_args!(
                            "transcript complete; subscriber response: {}",
                            response.dialogue
                        )),
                        Err(error) if error.code == "worker_cancelled" => break,
                        Err(error) => return Err(error.into()),
                    }
                    session.prepare_next_turn()?;
                }
                VoiceControl::Cancel => {
                    session.cancel()?;
                    break;
                }
            }
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => match line?.trim() {
                "ptt" => session.start_ptt()?,
                "release" => {
                    match session.release_ptt() {
                        Ok(response) => log_voice_event(format_args!(
                            "transcript complete; subscriber response: {}",
                            response.dialogue
                        )),
                        Err(error) if error.code == "worker_cancelled" => break,
                        Err(error) => return Err(error.into()),
                    }
                    session.prepare_next_turn()?;
                }
                "cancel" => {
                    session.cancel()?;
                    break;
                }
                "quit" => break,
                _ => log_voice_event("commands: ptt | release | quit"),
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

struct ConsoleVoiceOutput {
    inner: Box<dyn VoiceOutput>,
}

impl VoiceOutput for ConsoleVoiceOutput {
    fn status(
        &mut self,
        message: VoiceStatusMessage,
    ) -> Result<(), exchange_voice_daemon::VoiceError> {
        let summary = format!(
            "voice status: {:?} turn={} transcript={:?} response={:?}",
            message.status, message.turn_id, message.transcript, message.response_text
        );
        let result = self.inner.status(message);
        match &result {
            Ok(()) => log_voice_event(summary),
            Err(error) => log_voice_event(format_args!("voice status send failed: {error}")),
        }
        result
    }

    fn audio(
        &mut self,
        packet: exchange_protocol::RtpL16Packet,
    ) -> Result<(), exchange_voice_daemon::VoiceError> {
        if packet.marker {
            log_voice_event(format_args!(
                "voice audio: first packet ({} samples)",
                packet.samples.len()
            ));
        }
        self.inner.audio(packet)
    }

    fn finish(&mut self) -> Result<(), exchange_voice_daemon::VoiceError> {
        self.inner.finish()
    }
}

fn command(name: &str) -> Result<CommandSpec, Box<dyn Error>> {
    let value = env::var(name).map_err(|_| format!("{name} must name a local worker command"))?;
    Ok(CommandSpec::from_words(&value)?)
}

fn env_value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

fn demo_context() -> ResponseContext {
    let voice_id = if env::var_os("NN_VOICE_DEMO_RANDOM_VOICE").is_some() {
        random_demo_voice()
    } else {
        "Ryan".to_string()
    };
    ResponseContext {
        profile: SubscriberProfile {
            subscriber_id: 0,
            name: "Taren Kesh".to_string(),
            voice_id,
            personality: "precise railway dispatcher under pressure".to_string(),
            baseline_goals: vec!["Keep the railway moving".to_string()],
            initial_perspective: "The exchange is under observation".to_string(),
            relationships: vec![RelationshipNote {
                subject: "Vira Dhal".to_string(),
                note: "A trusted records clerk".to_string(),
            }],
            permitted_actions: vec!["request_routing".to_string()],
        },
        subscriber_goal: "Reach the requested Callee".to_string(),
        call_premise: "A railway dispatch is waiting".to_string(),
        story_beat_direction: "Ask for an ordinary connection".to_string(),
        permitted_knowledge: vec![KnowledgeRecord {
            fact: "The directory lists Vira Dhal".to_string(),
            learned_from: "directory_terminal".to_string(),
        }],
        beliefs: Vec::new(),
        relationship_notes: Vec::new(),
        memories: Vec::new(),
        recent_conversation: Vec::new(),
        current_input: None,
    }
}

fn random_demo_voice() -> String {
    const VOICES: [&str; 9] = [
        "Vivian", "Serena", "Uncle_Fu", "Dylan", "Eric", "Ryan", "Aiden", "Ono_Anna", "Sohee",
    ];
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.subsec_nanos() as usize);
    VOICES[nanos % VOICES.len()].to_string()
}
