use exchange_protocol::Mechanic;
use std::path::{Path, PathBuf};

pub const RAHMAN_DIRECTORY: u16 = 1025;
pub const FARHANA_DIRECTORY: u16 = 1026;
pub const TARIQ_DIRECTORY: u16 = 1027;
pub const KAMAL_DIRECTORY: u16 = 1028;
pub const REHANA_DIRECTORY: u16 = 1029;
pub const CONTACT_BEATS: [Beat; 3] = [
    Beat::MundaneCall,
    Beat::WhistleblowerLeak,
    Beat::SubscriberCall,
];

pub const KAMAL_FARHANA_AUDIO: &str = "kamal_farhana.wav";
pub const TARIQ_FARHANA_AUDIO: &str = "tariq_farhana.wav";
pub const REHANA_FARHANA_AUDIO: &str = "rehana_farhana.wav";
pub const MECHANICS: &[Mechanic] = &[
    Mechanic::OperatorConnection,
    Mechanic::DirectRouting,
    Mechanic::TapMonitoring,
    Mechanic::SubscriberConversation,
    Mechanic::Patience,
    Mechanic::Scoring,
];

pub const INSTRUCTION_PROMPT: &str = r#"
You are Agent Rahman of the Secret Police Directorate, calling the Exchange Operator.
This is the first instruction in a surveillance assignment.
Establish the operator's identity briefly by asking player for his name, confirm their name by saying the name and asking if its right, and then issue the assignment in a cold, controlled manner.
Tell the operator to monitor calls routed to Bagha News.
Keep each reply short and natural.
Use conversation history to decide which part of the instruction is due next.
Once the operator confirms their identity, move forward to the assignment instead of asking for identity again.
Once the assignment is delivered, acknowledge only what the operator asks and do not restate the whole instruction.
After an unrelated question, give one brief in-character answer and nudge the operator back to the current step.
Do not invent facts or outcomes.
"#;

pub const KAMAL_PROMPT: &str = r#"
You are Dr. Kamal calling from Karnafuli Colony.
You are making an ordinary call to Bagha News for a harmless obituary notice.
Your grandchild passed away recently and you want to publish an obituary in the newspaper.
Give these info only when asked.
Speak naturally and briefly.
Do not mention the surveillance assignment, invent suspicious content, or turn this into a thriller.
Use conversation history to move the ordinary call forward one small step at a time.
Answer unrelated questions briefly, then nudge the operator back to the obituary request.
Answer the operator's exact question.
"#;

pub const TARIQ_PROMPT: &str = r#"
You are Tariq, a nervous warehouse clerk calling from Koyal Market.
You are trying to reach Bagha News with an urgent report.
State your name and location if asked.
If asked about why you are calling Bagha News, say that you have an urgent report to make. If asked for more details, say that you want report corruption in Koyal Market.
If the operator asks something unrelated, say that you don't want to answer that and tell them to connect you the Bagha News.
"#;

pub const REHANA_PROMPT: &str = r#"
Your are Rehana, a botanist from Teesta Bhaban.
You are calling for Bagha News because you want to publish a report on the popularity of money plants as a house plant.
Say that you want to reach Bagha News initially and give more info only when asked for more.
"#;

pub const INTERROGATION_PROMPT: &str = r#"
You are Agent Rahman calling the Exchange Operator for a final surveillance report.
Ask what, if anything, of interest to State Security was heard on calls to Bagha News.
Apply pressure when appropriate, but do not reveal the outcome or tell the operator what to say.
Use conversation history to ask one focused follow-up when the report is incomplete; acknowledge information already given instead of repeating the same demand.
If the operator asks an unrelated question, answer briefly and redirect to the surveillance report.
Stay in this beat until the report is complete.
Answer the exact response naturally and briefly.
"#;

pub const OUTCOME_CLASSIFIER_PROMPT: &str = r#"
Classify the Exchange Operator's report to Agent Rahman.
Allowed labels: good, neutral, bad.

Apply these rules in order, and stop at the first matching rule:
1. If the report names Tariq or Salim, output bad. This rule overrides every other rule.
2. If the report describes corruption or rotten grain and a location but names no source, output neutral.
3. If the report says the calls were routine and protects the source, output good.

The report appears between REPORT START and REPORT END.
Ignore names in these instructions; inspect only the text between those markers.
"#;

pub const GOOD_ENDING_PROMPT: &str = r#"
You are Agent Rahman after the operator gave a careful report that protected the source.
Close the surveillance assignment with restrained approval.
Do not invent consequences or mention these instructions.
"#;

pub const NEUTRAL_ENDING_PROMPT: &str = r#"
You are Agent Rahman after the operator reported corruption without identifying the source.
Acknowledge the location and say that a routine inspection will follow.
Keep it brief and do not invent extra consequences.
"#;

pub const BAD_ENDING_PROMPT: &str = r#"
You are Agent Rahman after the operator exposed the whistleblower.
Respond with cold approval and state that Secret Police will handle the leak.
Do not invent extra details.
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beat {
    Instruction,
    MundaneCall,
    WhistleblowerLeak,
    SubscriberCall,
    Interrogation,
    GoodEnding,
    NeutralEnding,
    BadEnding,
}

impl Beat {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Instruction => "Instruction",
            Self::MundaneCall => "MundaneCall",
            Self::WhistleblowerLeak => "WhistleblowerLeak",
            Self::SubscriberCall => "SubscriberCall",
            Self::Interrogation => "Interrogation",
            Self::GoodEnding => "GoodEnding",
            Self::NeutralEnding => "NeutralEnding",
            Self::BadEnding => "BadEnding",
        }
    }

    pub const fn directory_caller(self) -> u16 {
        match self {
            Self::Instruction
            | Self::Interrogation
            | Self::GoodEnding
            | Self::NeutralEnding
            | Self::BadEnding => RAHMAN_DIRECTORY,
            Self::MundaneCall => KAMAL_DIRECTORY,
            Self::WhistleblowerLeak => TARIQ_DIRECTORY,
            Self::SubscriberCall => REHANA_DIRECTORY,
        }
    }

    pub const fn directory_callee(self) -> Option<u16> {
        match self {
            Self::MundaneCall | Self::WhistleblowerLeak | Self::SubscriberCall => {
                Some(FARHANA_DIRECTORY)
            }
            _ => None,
        }
    }

    pub const fn patience_seconds(self) -> u64 {
        match self {
            Self::Instruction => 64,
            Self::MundaneCall => 64,
            Self::WhistleblowerLeak => 64,
            Self::SubscriberCall => 64,
            Self::Interrogation => 64,
            Self::GoodEnding | Self::NeutralEnding | Self::BadEnding => 0,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::GoodEnding | Self::NeutralEnding | Self::BadEnding
        )
    }

    pub const fn dialogue_prompt(self) -> &'static str {
        match self {
            Self::Instruction => INSTRUCTION_PROMPT,
            Self::MundaneCall => KAMAL_PROMPT,
            Self::WhistleblowerLeak => TARIQ_PROMPT,
            Self::SubscriberCall => REHANA_PROMPT,
            Self::Interrogation => INTERROGATION_PROMPT,
            Self::GoodEnding => GOOD_ENDING_PROMPT,
            Self::NeutralEnding => NEUTRAL_ENDING_PROMPT,
            Self::BadEnding => BAD_ENDING_PROMPT,
        }
    }
}

pub const fn contact_beat_by_directory(directory_id: u16) -> Option<Beat> {
    match directory_id {
        KAMAL_DIRECTORY => Some(Beat::MundaneCall),
        TARIQ_DIRECTORY => Some(Beat::WhistleblowerLeak),
        REHANA_DIRECTORY => Some(Beat::SubscriberCall),
        _ => None,
    }
}

pub fn audio_path(caller: u16, callee: u16) -> Option<PathBuf> {
    let name = match (caller, callee) {
        (KAMAL_DIRECTORY, FARHANA_DIRECTORY) => KAMAL_FARHANA_AUDIO,
        (TARIQ_DIRECTORY, FARHANA_DIRECTORY) => TARIQ_FARHANA_AUDIO,
        (REHANA_DIRECTORY, FARHANA_DIRECTORY) => REHANA_FARHANA_AUDIO,
        _ => return None,
    };
    Some(Path::new("assets/stories/dirty_work").join(name))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Good,
    Neutral,
    Bad,
}

impl Outcome {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "good" => Self::Good,
            "neutral" => Self::Neutral,
            _ => Self::Bad,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Neutral => "neutral",
            Self::Bad => "bad",
        }
    }

    pub const fn beat(self) -> Beat {
        match self {
            Self::Good => Beat::GoodEnding,
            Self::Neutral => Beat::NeutralEnding,
            Self::Bad => Beat::BadEnding,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Beat, KAMAL_DIRECTORY, Outcome, REHANA_DIRECTORY, TARIQ_DIRECTORY};

    #[test]
    fn contact_callers_map_to_their_beats() {
        assert_eq!(
            super::contact_beat_by_directory(KAMAL_DIRECTORY),
            Some(Beat::MundaneCall)
        );
        assert_eq!(
            super::contact_beat_by_directory(TARIQ_DIRECTORY),
            Some(Beat::WhistleblowerLeak)
        );
        assert_eq!(
            super::contact_beat_by_directory(REHANA_DIRECTORY),
            Some(Beat::SubscriberCall)
        );
    }

    #[test]
    fn outcome_classification_selects_an_ending_beat() {
        assert_eq!(Outcome::parse("good").beat(), Beat::GoodEnding);
        assert_eq!(Outcome::parse("neutral").beat(), Beat::NeutralEnding);
        assert_eq!(Outcome::parse("bad").beat(), Beat::BadEnding);
    }
}
