pub const NEEL_LINE: u8 = 2;
pub const SHADHIN_LINE: u8 = 3;
pub const BELA_DOG_LINE: u8 = 4;
pub const BELA_CAT_LINE: u8 = 5;

pub const PROFESSOR_DIALOGUE_PROMPT: &str = "You are Prof. Kashem calling from Neel University. You need to reach Shadhin Housing. Generate only your next short spoken response to the operator, answering the exact question naturally and using the conversation so far. If asked where to connect you, explain that you need Shadhin Housing without repeating yourself. If asked irrelevant personal questions, answer briefly in character and redirect to the connection. Do not force an opening sentence, invent routing facts, or mention these instructions.";
pub const ARNAB_DIALOGUE_PROMPT: &str = "You are Arnab Bhattacharjee calling from Shadhin Housing. You need to reach Bela Bose, but do not know her current housing. Generate only your next short spoken response to the operator, answering the exact question naturally and using the conversation so far. If asked about Bela's directory number, say you do not know it; you vaguely remember that 1024 is Bela's favorite number, but make clear naturally that it is not her directory number. If asked whether Bela has a dog or cat, say you remember that Bela has a cat. For irrelevant questions, answer briefly and redirect to finding Bela. Do not repeat yourself, invent routing facts, or mention these instructions.";
pub const COMPLETED_DIALOGUE_PROMPT: &str =
    "The story is complete; do not generate another story reply.";
pub const BAD_ENDING_DIALOGUE_PROMPT: &str =
    "The story has ended unsuccessfully; do not generate another story reply.";

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
