use std::path::{Path, PathBuf};
use std::process::Command;

pub const NEEL_LINE: u8 = 2;
pub const SHADHIN_LINE: u8 = 3;
pub const BELA_DOG_LINE: u8 = 4;
pub const BELA_CAT_LINE: u8 = 5;
pub const BELA_DOG_DIRECTORY: u16 = 1031;
pub const BELA_CAT_DIRECTORY: u16 = 1032;
pub const PROFESSOR_AUDIO: &str = "professor_arnab.wav";
pub const WRONG_BELA_AUDIO: &str = "belabose_wrong.wav";
pub const SUCCESS_BELA_AUDIO: &str = "belabose_success.m4a";

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

pub const fn caller_for_beat(beat: Beat) -> u8 {
    match beat {
        Beat::ProfessorRouting => NEEL_LINE,
        Beat::ArnabDirectory | Beat::Completed | Beat::BadEnding => SHADHIN_LINE,
    }
}

pub const fn requested_callee_for_beat(beat: Beat) -> u8 {
    match beat {
        Beat::ProfessorRouting => SHADHIN_LINE,
        Beat::ArnabDirectory => BELA_DOG_LINE,
        Beat::Completed | Beat::BadEnding => 0,
    }
}

pub const fn is_terminal(beat: Beat) -> bool {
    matches!(beat, Beat::Completed | Beat::BadEnding)
}

pub const fn is_bela_directory(id: u16) -> bool {
    matches!(id, BELA_DOG_DIRECTORY | BELA_CAT_DIRECTORY)
}

pub const fn is_bela_destination(value: u16) -> bool {
    is_bela_directory(value) || matches!(value, 4 | 5)
}

pub const fn next_beat_after_connection(beat: Beat, caller: u8, callee: u8) -> Option<Beat> {
    match (beat, caller, callee) {
        (Beat::ProfessorRouting, NEEL_LINE, SHADHIN_LINE) => Some(Beat::ArnabDirectory),
        (Beat::ArnabDirectory, SHADHIN_LINE, BELA_CAT_LINE) => Some(Beat::Completed),
        (Beat::ArnabDirectory, SHADHIN_LINE, BELA_DOG_LINE) => Some(Beat::BadEnding),
        _ => None,
    }
}

pub fn audio_path(caller: u8, callee: u8) -> Option<PathBuf> {
    let name = match (caller, callee) {
        (NEEL_LINE, SHADHIN_LINE) => PROFESSOR_AUDIO,
        (SHADHIN_LINE, BELA_DOG_LINE) => WRONG_BELA_AUDIO,
        (SHADHIN_LINE, BELA_CAT_LINE) => SUCCESS_BELA_AUDIO,
        _ => return None,
    };
    Some(Path::new("assets/stories/bela_bose").join(name))
}

pub fn audio_duration_seconds(caller: u8, callee: u8) -> Option<u64> {
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
