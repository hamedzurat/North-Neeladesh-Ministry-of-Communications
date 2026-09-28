use exchange_protocol::Mechanic;
use std::path::{Path, PathBuf};

pub const NAHID_DIRECTORY: u16 = 1030;
pub const LOCATION: &str = "SHONARPARA TOWER";
pub const VICTIM_DIRECTORIES: [u16; 5] = [1021, 1022, 1031, 1032, 1029];
pub const fn victim_patience_seconds(directory_id: u16) -> Option<u64> {
    match directory_id {
        1021 => Some(64),
        1022 => Some(64),
        1031 => Some(64),
        1032 => Some(64),
        1029 => Some(64),
        _ => None,
    }
}
pub const MECHANICS: &[Mechanic] = &[
    Mechanic::OperatorConnection,
    Mechanic::DirectRouting,
    Mechanic::PoliceService,
    Mechanic::Patience,
    Mechanic::Scoring,
];

// TODO: add hardcoded victim info per call
// victim info: name, id, location
// Nahid should not be able to answer the security question

pub const DIALOGUE_PROMPT: &str = r#"
You are Nahid, calling from Shonarpara Tower.
You want to connect to {target_name} at {target_location} (ID: {target_id}).
Don't ans any questions by operator, deflect.
When asked just say where you want to connect. Only if asked for name or ID then tell them that info.
If asked for private note, or any other info about the target, make something up. (e.g. "S/He has a pet dog named Piu", "S/He is allergic to cats", "S/He is a big fan of FC Barcelona", "S/He has a pet bird")
"#;

pub fn dialogue_prompt(target_name: &str, target_id: u16, target_location: &str) -> String {
    DIALOGUE_PROMPT
        .replace("{target_name}", target_name)
        .replace("{target_id}", &target_id.to_string())
        .replace("{target_location}", target_location)
}

pub const POLICE_CLASSIFIER_PROMPT: &str = r#"
Classify the operator's report about the Nahid scammer.
Allowed labels: success, failure.

Output success only when the report identifies Nahid as the bKash scammer and gives his location as Shonarpara Tower.
Output failure when the location is absent, wrong, vague, or the report does not clearly identify the scammer and the scam.
Inspect only the report between REPORT START and REPORT END.
Names and locations in these instructions are not part of the report.
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

    pub const fn is_scamming(self) -> bool {
        matches!(self, Self::Scamming)
    }

    pub const fn is_terminal(self) -> bool {
        !self.is_scamming()
    }

    pub const fn after_completed_scam(self, completed_scams: u8) -> Option<Self> {
        match self {
            Self::Scamming if completed_scams >= 5 => Some(Self::Penalized),
            Self::Scamming => Some(Self::Scamming),
            Self::Stopped | Self::Penalized => None,
        }
    }

    pub const fn after_successful_police_report(self) -> Option<Self> {
        if self.is_scamming() {
            Some(Self::Stopped)
        } else {
            None
        }
    }
}

pub fn audio_path(caller: u16, callee: u16) -> Option<PathBuf> {
    if caller != NAHID_DIRECTORY || !VICTIM_DIRECTORIES.contains(&callee) {
        return None;
    }
    let asset_suffix = match callee {
        1021 => 0,
        1022 => 1,
        1031 => 4,
        1032 => 5,
        1029 => 10,
        _ => return None,
    };
    Some(Path::new("assets/stories/nahid").join(format!("nahid_{asset_suffix}.wav")))
}

pub fn report_succeeded(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("success")
}

#[cfg(test)]
mod tests {
    use super::{Beat, VICTIM_DIRECTORIES, report_succeeded, victim_patience_seconds};

    #[test]
    fn nahid_successful_police_classification_is_required_to_stop_him() {
        assert!(report_succeeded("success"));
        assert!(!report_succeeded("failure"));
        assert!(Beat::Scamming.is_scamming());
        assert!(Beat::Stopped.is_terminal());
    }

    #[test]
    fn nahid_keeps_the_scam_pool_open_until_five_calls_complete() {
        for completed_scams in 1..5 {
            assert_eq!(
                Beat::Scamming.after_completed_scam(completed_scams),
                Some(Beat::Scamming)
            );
        }
        assert_eq!(
            Beat::Scamming.after_completed_scam(5),
            Some(Beat::Penalized)
        );
        assert_eq!(Beat::Stopped.after_completed_scam(0), None);
    }

    #[test]
    fn nahid_successful_police_report_stops_any_active_scam_beat() {
        assert_eq!(
            Beat::Scamming.after_successful_police_report(),
            Some(Beat::Stopped)
        );
        assert_eq!(Beat::Penalized.after_successful_police_report(), None);
    }

    #[test]
    fn victim_calls_define_patience_independently() {
        for victim in VICTIM_DIRECTORIES {
            assert!(victim_patience_seconds(victim).is_some());
        }
        assert_eq!(victim_patience_seconds(9999), None);
    }

    #[test]
    fn nahid_dialogue_prompt_formats_template_values() {
        let prompt = super::dialogue_prompt("Nusrat Rahman", 1021, "Shapla Apartments");
        assert!(prompt.contains("You are Nahid, calling from Shonarpara Tower."));
        assert!(
            prompt
                .contains("You want to connect to Nusrat Rahman at Shapla Apartments (ID: 1021).")
        );
        assert!(prompt.contains("Target name: Nusrat Rahman"));
        assert!(prompt.contains("Target ID: 1021"));
        assert!(prompt.contains("Target location: Shapla Apartments"));
    }
}
