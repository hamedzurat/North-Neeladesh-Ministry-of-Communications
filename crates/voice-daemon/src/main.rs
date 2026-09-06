use std::env;
use std::error::Error;
use std::io::{self, BufRead};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use exchange_protocol::{VOICE_PROTOCOL_VERSION, VoiceControl};
use exchange_voice_daemon::{
    CommandAudioPlayback, CommandDialogueGenerator, CommandMicrophone, CommandSpec,
    CommandSpeechToText, KnowledgeRecord, OperatorSession, Qwen3TtsCommand, RelationshipNote,
    ResponseContext, SessionPhase, SubscriberProfile, TeeVoiceOutput, UdpVoiceOutput, VoiceOutput,
    request_worker_cancellation,
};

fn main() -> Result<(), Box<dyn Error>> {
    let backend = env_value("NN_VOICE_BACKEND_ADDRESS", "127.0.0.1:7879");
    let capture = command("NN_VOICE_CAPTURE_COMMAND")?;
    let stt = command("NN_VOICE_STT_COMMAND")?;
    let dialogue = command("NN_VOICE_DIALOGUE_COMMAND")?;
    let tts = command("NN_VOICE_TTS_COMMAND")?;
    let udp_output = UdpVoiceOutput::connect(&backend, 1, 0)?;
    let control_socket = udp_output.try_clone()?;
    let output: Box<dyn VoiceOutput> = match env::var("NN_VOICE_PLAYBACK_COMMAND") {
        Ok(command) => Box::new(TeeVoiceOutput::new(
            udp_output,
            CommandAudioPlayback::new(CommandSpec::from_words(&command)?)?,
        )),
        Err(_) => Box::new(udp_output),
    };
    let mut session = OperatorSession::new(
        1,
        0,
        demo_context(),
        Box::new(CommandMicrophone::new(capture)),
        Box::new(CommandSpeechToText::new(stt)),
        Box::new(CommandDialogueGenerator::new(dialogue)),
        Box::new(Qwen3TtsCommand::new(tts)),
        output,
    )?;

    session.announce_ready()?;
    println!("voice daemon ready; type ptt, release, or quit");
    let (commands, receiver) = mpsc::channel();
    let (controls, control_receiver) = mpsc::channel();
    let control_monitor = control_socket.try_clone()?;
    thread::spawn(move || {
        loop {
            if let Ok(Some(control)) = control_monitor.try_receive_control()
                && control.protocol_version == VOICE_PROTOCOL_VERSION
                && control.session_id == 1
            {
                if control.control == VoiceControl::Cancel {
                    request_worker_cancellation();
                }
                if controls.send(control).is_err() {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(5));
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
                VoiceControl::StartPtt => session.start_ptt()?,
                VoiceControl::ReleasePtt => {
                    match session.release_ptt() {
                        Ok(response) => println!(
                            "transcript complete; subscriber response: {}",
                            response.dialogue
                        ),
                        Err(error) if error.code == "worker_cancelled" => break,
                        Err(error) => return Err(error.into()),
                    }
                    break;
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
                        Ok(response) => println!(
                            "transcript complete; subscriber response: {}",
                            response.dialogue
                        ),
                        Err(error) if error.code == "worker_cancelled" => break,
                        Err(error) => return Err(error.into()),
                    }
                    break;
                }
                "cancel" => {
                    session.cancel()?;
                    break;
                }
                "quit" => break,
                _ => println!("commands: ptt | release | quit"),
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if matches!(
            session.phase(),
            SessionPhase::Completed | SessionPhase::Failed
        ) {
            break;
        }
    }
    Ok(())
}

fn command(name: &str) -> Result<CommandSpec, Box<dyn Error>> {
    let value = env::var(name).map_err(|_| format!("{name} must name a local worker command"))?;
    Ok(CommandSpec::from_words(&value)?)
}

fn env_value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_string())
}

fn demo_context() -> ResponseContext {
    ResponseContext {
        profile: SubscriberProfile {
            subscriber_id: 0,
            name: "Taren Kesh".to_string(),
            voice_id: "taren".to_string(),
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
