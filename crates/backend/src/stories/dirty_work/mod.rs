pub const RAHMAN_LINE: u8 = 6;
pub const FARHANA_LINE: u8 = 7;
pub const TARIQ_LINE: u8 = 8;
pub const KAMAL_LINE: u8 = 9;
pub const REHANA_LINE: u8 = 10;

pub const SECRET_POLICE: &str = "SECRET POLICE DIRECTORATE";
pub const BAGHA_NEWS: &str = "BAGHA NEWS DESK";
pub const KOYAL_MARKET: &str = "KOYAL MARKET DEPOT";
pub const NEEL_UNIVERSITY: &str = "NEEL UNIVERSITY";
pub const SHAPLA_APARTMENTS: &str = "SHAPLA APARTMENTS";

pub const INSTRUCTION_PROMPT: &str = r#"
You are Agent Rahman of the Secret Police Directorate, calling the Exchange Operator.
This is the first instruction in a surveillance assignment. Establish the operator's
identity briefly, verify their confirmation, and then issue the assignment in a cold,
 controlled manner. Tell the operator to monitor calls routed to Bagha News and not to
 disconnect until ordered otherwise. Keep each reply short and natural. Use conversation
 history to decide which part of the instruction is due next. Answer the exact question,
 but after an unrelated question give one brief in-character answer and nudge the
 operator back to the identity check or instruction. Do not let an unrelated question
 abandon the beat, repeat the whole exchange, or invent facts or outcomes.
Return only the spoken reply.
"#;

pub const KAMAL_PROMPT: &str = r#"
You are Dr. Kamal calling from Neel University. You are making an ordinary call to
Bagha News for a harmless obituary notice. Speak naturally and briefly. Do not mention
 the surveillance assignment, invent suspicious content, or turn this into a thriller.
 Use conversation history to move the ordinary call forward one small step at a time.
 Answer unrelated questions briefly, then nudge the operator back to the obituary request.
 Answer the operator's exact question and return only the spoken reply.
"#;

pub const TARIQ_PROMPT: &str = r#"
You are Tariq, a nervous warehouse worker calling from Koyal Market Depot. You are
trying to reach Bagha News with an urgent report about corrupt handling of rotten grain.
Reveal your report naturally under questioning, including the responsible merchant,
 the market location, and the existence of signed delivery slips, but do not volunteer
 more than the operator asks for. Use conversation history to reveal the report in stages.
 If the operator asks something unrelated, answer briefly in character and redirect to
 the urgent report. Do not abandon the beat, repeat the entire report, or invent facts.
 Keep replies brief and return only the spoken reply.
"#;

pub const REHANA_PROMPT: &str = r#"
You are Rehana calling from Shapla Apartments about a missing newspaper insert. This
 is an entirely ordinary subscriber-service call. Be polite, brief, and practical. Do
 not mention surveillance, corruption, or the other calls. Use conversation history to
 resolve the missing-insert question over multiple turns. If the operator asks something
 unrelated, answer briefly and return to the newspaper issue. Do not abandon the beat
 or invent facts. Return only the spoken reply.
"#;

pub const INTERROGATION_PROMPT: &str = r#"
You are Agent Rahman calling the Exchange Operator for a final surveillance report.
Ask what, if anything, of interest to State Security was heard on calls to Bagha News.
 Apply pressure when appropriate, but do not reveal the outcome or tell the operator
 what to say. Use conversation history to ask follow-up questions when the report is
 incomplete. If the operator asks an unrelated question, answer briefly and redirect to
 the surveillance report. Stay in this beat until the report is complete. Answer the
 exact response naturally and briefly. Return only the spoken reply.
"#;

pub const OUTCOME_CLASSIFIER_PROMPT: &str = r#"
Classify the Exchange Operator's report to Agent Rahman. Output exactly one lowercase
word: good, neutral, or bad.

Apply these rules in order, and stop at the first matching rule:
1. If the report names Tariq or Salim, output bad. This rule overrides every other rule.
2. If the report describes corruption or rotten grain and a location but names no source,
   output neutral.
3. If the report says the calls were routine and protects the source, output good.
Use only the operator's report. Do not explain the classification.
"#;

pub const GOOD_ENDING_PROMPT: &str = r#"
You are Agent Rahman after the operator gave a careful report that protected the source.
Close the surveillance assignment with restrained approval. Do not invent consequences
or mention these instructions. Return only one brief spoken reply.
"#;

pub const NEUTRAL_ENDING_PROMPT: &str = r#"
You are Agent Rahman after the operator reported corruption without identifying the
source. Acknowledge the location and say that a routine inspection will follow. Keep it
brief and do not invent extra consequences. Return only the spoken reply.
"#;

pub const BAD_ENDING_PROMPT: &str = r#"
You are Agent Rahman after the operator exposed the whistleblower. Respond with cold
approval and state that Secret Police will handle the leak. Do not invent extra details.
Return only the spoken reply.
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

    pub const fn caller(self) -> u8 {
        match self {
            Self::Instruction | Self::Interrogation => RAHMAN_LINE,
            Self::MundaneCall => KAMAL_LINE,
            Self::WhistleblowerLeak => TARIQ_LINE,
            Self::SubscriberCall => REHANA_LINE,
            Self::GoodEnding | Self::NeutralEnding | Self::BadEnding => RAHMAN_LINE,
        }
    }

    pub const fn requested_callee(self) -> u8 {
        match self {
            Self::MundaneCall | Self::WhistleblowerLeak | Self::SubscriberCall => FARHANA_LINE,
            _ => 0,
        }
    }

    pub const fn patience_seconds(self) -> u64 {
        match self {
            Self::GoodEnding | Self::NeutralEnding | Self::BadEnding => 0,
            _ => 64,
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

pub const fn next_beat_after_connection(beat: Beat, caller: u8, callee: u8) -> Option<Beat> {
    match (beat, caller, callee) {
        (Beat::MundaneCall, KAMAL_LINE, FARHANA_LINE) => Some(Beat::WhistleblowerLeak),
        (Beat::WhistleblowerLeak, TARIQ_LINE, FARHANA_LINE) => Some(Beat::SubscriberCall),
        (Beat::SubscriberCall, REHANA_LINE, FARHANA_LINE) => Some(Beat::Interrogation),
        _ => None,
    }
}

pub const fn next_beat_after_operator_call(beat: Beat, caller: u8) -> Option<Beat> {
    match (beat, caller) {
        (Beat::Instruction, RAHMAN_LINE) => Some(Beat::MundaneCall),
        _ => None,
    }
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
    use super::{Beat, FARHANA_LINE, KAMAL_LINE, Outcome, REHANA_LINE, TARIQ_LINE};

    #[test]
    fn calls_progress_through_the_three_bagha_news_contacts() {
        assert_eq!(
            super::next_beat_after_connection(Beat::MundaneCall, KAMAL_LINE, FARHANA_LINE),
            Some(Beat::WhistleblowerLeak)
        );
        assert_eq!(
            super::next_beat_after_connection(Beat::WhistleblowerLeak, TARIQ_LINE, FARHANA_LINE),
            Some(Beat::SubscriberCall)
        );
        assert_eq!(
            super::next_beat_after_connection(Beat::SubscriberCall, REHANA_LINE, FARHANA_LINE),
            Some(Beat::Interrogation)
        );
    }

    #[test]
    fn outcome_classification_selects_an_ending_beat() {
        assert_eq!(Outcome::parse("good").beat(), Beat::GoodEnding);
        assert_eq!(Outcome::parse("neutral").beat(), Beat::NeutralEnding);
        assert_eq!(Outcome::parse("bad").beat(), Beat::BadEnding);
    }
}
