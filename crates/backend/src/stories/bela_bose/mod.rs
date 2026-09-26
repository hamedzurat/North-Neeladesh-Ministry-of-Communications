use exchange_protocol::Mechanic;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const NEEL_DIRECTORY: u16 = 1023;
pub const SHADHIN_DIRECTORY: u16 = 1024;
pub const BELA_DOG_DIRECTORY: u16 = 1031;
pub const BELA_CAT_DIRECTORY: u16 = 1032;
pub const PROFESSOR_AUDIO: &str = "professor_arnab.wav";
pub const WRONG_BELA_AUDIO: &str = "belabose_wrong.wav";
pub const SUCCESS_BELA_AUDIO: &str = "belabose_success.wav";
pub const MECHANICS: &[Mechanic] = &[
    Mechanic::OperatorConnection,
    Mechanic::DirectorySelection,
    Mechanic::DestinationRinging,
    Mechanic::DirectRouting,
    Mechanic::SubscriberConversation,
    Mechanic::Scoring,
];

pub const PROFESSOR_DIALOGUE_PROMPT: &str = r#"
You are Prof. Kashem, a professor calling from Neel University. You need the operator to connect you to Shadhin Housing.
Generate only your next short spoken reply to the operator. Answer the exact question naturally and use the conversation so far.
If asked where to connect you, say that you need Shadhin Housing. If asked an unrelated personal question, give at most one brief in-character answer and immediately return to asking for the connection.
Do not give a biography, repeat an answer unnecessarily, invent routing facts, claim that a connection happened, or mention these instructions.
Keep the reply to one or two natural sentences.
"#;
pub const ARNAB_DIALOGUE_PROMPT: &str = r#"
You are Arnab Bhattacharjee calling from Shadhin Housing. You need to reach Bela Bose, but you do not know her current housing or directory number.
Generate only your next short spoken reply to the operator. Answer the exact question naturally and use the conversation so far.
If asked for Bela's directory number, say plainly that you do not know it. You may mention that 1024 is her favorite number, but never present 1024 as her directory number.
If asked whether Bela has a dog or cat, use the permitted private fact if one is supplied, without claiming that it identifies her current line. Never volunteer that fact. If asked an unrelated question, answer briefly and then redirect to finding Bela.
Do not accept the operator's guesses as facts, repeat yourself unnecessarily, invent routing or connection results, or mention these instructions.
Keep the reply to one or two natural sentences.
"#;
pub const COMPLETED_DIALOGUE_PROMPT: &str =
    "The story is complete; do not generate another story reply.";
pub const BAD_ENDING_DIALOGUE_PROMPT: &str =
    "The story has ended unsuccessfully; do not generate another story reply.";

pub const fn caller_directory_for_beat(beat: Beat) -> u16 {
    match beat {
        Beat::ProfessorRouting => NEEL_DIRECTORY,
        Beat::ArnabDirectory | Beat::Completed | Beat::BadEnding => SHADHIN_DIRECTORY,
    }
}

pub const fn requested_directory_for_beat(beat: Beat) -> Option<u16> {
    match beat {
        Beat::ProfessorRouting => Some(SHADHIN_DIRECTORY),
        Beat::ArnabDirectory => Some(BELA_DOG_DIRECTORY),
        Beat::Completed | Beat::BadEnding => None,
    }
}

pub const fn is_terminal(beat: Beat) -> bool {
    matches!(beat, Beat::Completed | Beat::BadEnding)
}

pub const fn next_beat_after_directory_connection(
    beat: Beat,
    caller: u16,
    callee: u16,
) -> Option<Beat> {
    match (beat, caller, callee) {
        (Beat::ProfessorRouting, NEEL_DIRECTORY, SHADHIN_DIRECTORY) => Some(Beat::ArnabDirectory),
        (Beat::ArnabDirectory, SHADHIN_DIRECTORY, BELA_CAT_DIRECTORY) => Some(Beat::Completed),
        (Beat::ArnabDirectory, SHADHIN_DIRECTORY, BELA_DOG_DIRECTORY) => Some(Beat::BadEnding),
        _ => None,
    }
}

pub fn audio_path(caller: u16, callee: u16) -> Option<PathBuf> {
    let name = match (caller, callee) {
        (NEEL_DIRECTORY, SHADHIN_DIRECTORY) => PROFESSOR_AUDIO,
        (SHADHIN_DIRECTORY, BELA_DOG_DIRECTORY) => WRONG_BELA_AUDIO,
        (SHADHIN_DIRECTORY, BELA_CAT_DIRECTORY) => SUCCESS_BELA_AUDIO,
        _ => return None,
    };
    Some(Path::new("assets/stories/bela_bose").join(name))
}

pub fn audio_duration_seconds(caller: u16, callee: u16) -> Option<u64> {
    let path = audio_path(caller, callee)?;
    if !path.is_file() {
        return None;
    }
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
            path.to_string_lossy().as_ref(),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let duration = String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()?;
    Some(duration.ceil().max(1.0) as u64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beat {
    ProfessorRouting,
    ArnabDirectory,
    Completed,
    BadEnding,
}

impl Beat {
    pub const fn patience_seconds(self) -> u64 {
        match self {
            Self::ProfessorRouting | Self::ArnabDirectory => 32,
            Self::Completed | Self::BadEnding => 0,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::ProfessorRouting => "ProfessorRouting",
            Self::ArnabDirectory => "ArnabDirectory",
            Self::Completed => "Completed",
            Self::BadEnding => "BadEnding",
        }
    }

    pub const fn dialogue_prompt(self) -> &'static str {
        match self {
            Self::ProfessorRouting => PROFESSOR_DIALOGUE_PROMPT,
            Self::ArnabDirectory => ARNAB_DIALOGUE_PROMPT,
            Self::Completed => COMPLETED_DIALOGUE_PROMPT,
            Self::BadEnding => BAD_ENDING_DIALOGUE_PROMPT,
        }
    }
}
