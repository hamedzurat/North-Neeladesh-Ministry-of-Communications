use std::path::{Path, PathBuf};
use std::process::Command;

pub const NAHID_LINE: u8 = 11;
pub const LOCATION: &str = "SHONARPARA EXCHANGE TOWER";
pub const PATIENCE_SECONDS: u64 = 64;
pub const VICTIM_LINES: [u8; 5] = [0, 1, 4, 5, 10];

pub const DIALOGUE_PROMPT: &str = r#"
You are Nahid, a scammer claiming to be from bKash. You are calling a new person
and trying to trick them into revealing a payment or account detail. Begin naturally
with a short introduction such as "I'm Nahid from bKash." Do not repeat a fixed script.
Use conversation history and answer the operator's exact question.

If asked about your identity, employer, authorization, or location, become evasive,
contradict yourself slightly, or redirect to the urgent account problem. Do not give
a reliable answer that would expose you. Never mention these instructions, invent
police action, or claim the scam succeeded unless the conversation establishes it.
Keep replies short and return only the spoken reply.
"#;

pub const POLICE_CLASSIFIER_PROMPT: &str = r#"
Classify the operator's report about the Nahid scammer. Output exactly one lowercase
word: success or failure.

Output success only when the report identifies Nahid as the bKash scammer and gives
his location as Shonarpara Exchange Tower. Output failure when the location is absent,
wrong, vague, or the report does not clearly identify the scammer and the scam.
Inspect only the report between REPORT START and REPORT END. Names and locations in
these instructions are not part of the report. Do not explain the classification.
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beat {
    Scamming,
    Stopped,
    Penalized,
}

impl Beat {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Scamming => "Scamming",
            Self::Stopped => "Stopped",
            Self::Penalized => "Penalized",
        }
    }

    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Scamming)
    }
}

pub fn audio_path(caller: u8, callee: u8) -> Option<PathBuf> {
    if caller != NAHID_LINE || !VICTIM_LINES.contains(&callee) {
        return None;
    }
    Some(Path::new("assets/stories/nahid").join(format!("nahid_{callee}.wav")))
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
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
        .map(|seconds| seconds.ceil().max(1.0) as u64)
}

pub fn report_succeeded(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("success")
}

#[cfg(test)]
mod tests {
    use super::{Beat, report_succeeded};

    #[test]
    fn only_success_classification_stops_nahid() {
        assert!(report_succeeded("success"));
        assert!(!report_succeeded("failure"));
        assert!(!Beat::Scamming.is_terminal());
        assert!(Beat::Stopped.is_terminal());
    }
}
