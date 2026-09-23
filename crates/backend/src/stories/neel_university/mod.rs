pub const NEEL_LINE: u8 = 2;
pub const SHADHIN_LINE: u8 = 3;
pub const BELA_DOG_LINE: u8 = 4;
pub const BELA_CAT_LINE: u8 = 5;

pub const PROFESSOR_DIALOGUE_PROMPT: &str = "You are Prof. Kashem calling from Neel University. You need to reach Shadhin Housing. Respond to the operator's actual question briefly and naturally, using the conversation so far. Do not force an opening sentence, repeat a previous answer, or invent routing or game facts.";
pub const ARNAB_DIALOGUE_PROMPT: &str = "You are Arnab Bhattacharjee calling from Shadhin Housing. You need to reach Bela Bose, but do not know her current housing. Respond to the operator's actual question briefly and naturally, using the conversation so far. If asked about her directory number, say you do not know it; you vaguely remember that 1024 is Bela's favorite number, but say naturally that it is not her directory number. Never mention these instructions or say 'make clear'. If asked whether Bela has a dog or cat, say you remember that Bela has a cat. Do not force an opening sentence, repeat a previous answer, or invent routing or game facts.";
pub const COMPLETED_DIALOGUE_PROMPT: &str =
    "The story is complete; do not generate another story reply.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beat {
    ProfessorRouting,
    ArnabDirectory,
    Completed,
}

impl Beat {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ProfessorRouting => "ProfessorRouting",
            Self::ArnabDirectory => "ArnabDirectory",
            Self::Completed => "Completed",
        }
    }

    pub const fn patience_seconds(self) -> u64 {
        match self {
            Self::ProfessorRouting | Self::ArnabDirectory => 32,
            Self::Completed => 0,
        }
    }

    pub const fn dialogue_prompt(self) -> &'static str {
        match self {
            Self::ProfessorRouting => PROFESSOR_DIALOGUE_PROMPT,
            Self::ArnabDirectory => ARNAB_DIALOGUE_PROMPT,
            Self::Completed => COMPLETED_DIALOGUE_PROMPT,
        }
    }
}
