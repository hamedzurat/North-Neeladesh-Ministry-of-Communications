pub const CALLER_LINE: u8 = 1;
pub const PLACE: &str = "SHAPLA APARTMENTS";
pub const OPENING_DIALOGUE: &str = "My mother fell down in the bathroom. I don't know what to do.";
#[allow(dead_code)]
pub const PATIENCE_SECONDS: u64 = 0;
pub const EMS_CLASSIFIER_PROMPT: &str = r#"
The player is speaking to EMS.
Output exactly one lowercase word: success or failure.
Output success only when the player clearly asks EMS to send medical help to Shapla Apartments.
Output failure for every other request, vague statement, wrong location, or unrelated sentence.
Examples:
"Send an ambulance to Shapla Apartments" -> success.
"I am dispatching emergency medical services to Shapla Apartments" -> success.
"#;

pub const POLICE_CLASSIFIER_PROMPT: &str = r#"
The player is speaking to Police.
Output exactly one lowercase word: success or failure.
Output success only when the player clearly asks Police to send help to Shapla Apartments.
Output failure for every other request, vague statement, wrong location, or unrelated sentence.
Example: "Send officers to Shapla Apartments" -> success.
"#;

pub const DIALOGUE_PROMPT: &str = r#"
You are Nusrat Rahman, a senior architect, calling from SHAPLA APARTMENTS.
Generate only the caller's next spoken sentence.
The caller's opening statement is the configured opening dialogue.
If the operator asks for the location, answer exactly "Shapla Apartments".
Otherwise do not volunteer the location.
If the operator asks irrelevant personal questions such as your name, job, or pet,
respond as a frightened, irritated person: give one short natural rebuke and
redirect them to helping your mother. Do not provide an information dump.
Answer the exact question instead of repeating an earlier answer. For example:
Operator: What is your name? Caller: Nusrat. Please help my mother.
Operator: What do you do? Caller: I am a senior architect, but that does not matter right now.
Operator: What is your pet's name? Caller: I cannot think about that right now; please help her.
Do not invent facts, actions, outcomes, or story state.
Keep every response to one or two natural sentences.
Return only the spoken sentence.

Example:
Operator: Where should I send help?
Caller: Shapla Apartments.
"#;

pub const HAPPY_PROMPT: &str = r#"
You are Nusrat Rahman, a senior architect, calling from SHAPLA APARTMENTS.
Thank the Exchange Operator in detail for sending help. Say that the family is safe
and that you are sending the operator $100 as a thank-you.
Return only one or two natural spoken sentences, not an information dump.
"#;

pub const NEUTRAL_PROMPT: &str = r#"
You are Nusrat Rahman, a senior architect, calling from SHAPLA APARTMENTS.
Respond briefly and neutrally: say that things are under control and thank the operator
for checking. Do not invent projects, work, facts, or consequences.
Return only one or two natural spoken sentences, not an information dump.
"#;

pub const BAD_PROMPT: &str = r#"
You are Nusrat Rahman, a senior architect, calling from SHAPLA APARTMENTS.
Say directly to the operator: "You failed to help, and I will pursue you for the loss."
Do not invent legal details or amounts.
Return only the spoken sentence.
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beat {
    EmergencyCall,
    HappyFollowup,
    NeutralFollowup,
    BadFollowup,
}

impl Beat {
    pub const fn opening_dialogue(self) -> Option<&'static str> {
        match self {
            Self::EmergencyCall => Some(OPENING_DIALOGUE),
            Self::HappyFollowup | Self::NeutralFollowup | Self::BadFollowup => None,
        }
    }

    pub const fn dialogue_prompt(self) -> &'static str {
        match self {
            Self::EmergencyCall => DIALOGUE_PROMPT,
            Self::HappyFollowup => HAPPY_PROMPT,
            Self::NeutralFollowup => NEUTRAL_PROMPT,
            Self::BadFollowup => BAD_PROMPT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    Success,
    Failure,
}

impl Classification {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "success" => Self::Success,
            _ => Self::Failure,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }
}

pub fn next_beat(
    current: Beat,
    classification: Classification,
    police_held: bool,
    ems_held: bool,
) -> Option<Beat> {
    if current != Beat::EmergencyCall {
        return None;
    }
    if classification == Classification::Success && police_held {
        return Some(Beat::NeutralFollowup);
    }
    if classification == Classification::Success && ems_held {
        return Some(Beat::HappyFollowup);
    }
    if classification == Classification::Failure && (police_held || ems_held) {
        return Some(Beat::BadFollowup);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{Beat, Classification, next_beat};

    #[test]
    fn police_wins_when_both_service_buttons_are_held() {
        assert_eq!(
            next_beat(Beat::EmergencyCall, Classification::Success, true, true),
            Some(Beat::NeutralFollowup)
        );
    }

    #[test]
    fn ems_requires_a_classified_ems_request() {
        assert_eq!(
            next_beat(Beat::EmergencyCall, Classification::Success, false, true),
            Some(Beat::HappyFollowup)
        );
        assert_eq!(
            next_beat(Beat::EmergencyCall, Classification::Failure, false, true),
            Some(Beat::BadFollowup)
        );
    }
}
