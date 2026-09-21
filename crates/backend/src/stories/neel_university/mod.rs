pub const NEEL_LINE: u8 = 2;
pub const SHADHIN_LINE: u8 = 3;
pub const BELA_DOG_LINE: u8 = 4;
pub const BELA_CAT_LINE: u8 = 5;

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

    pub const fn opening_dialogue(self) -> Option<&'static str> {
        match self {
            Self::ProfessorRouting => Some("I want to talk to Shadhin Housing."),
            Self::ArnabDirectory => {
                Some("I want to talk to Bela Bose, but I do not know where she lives now.")
            }
            Self::Completed => None,
        }
    }

    pub const fn dialogue_prompt(self) -> &'static str {
        match self {
            Self::ProfessorRouting => {
                "You are Prof. Kashem calling from Neel University. You want to speak with Shadhin Housing. Answer the operator's exact question briefly and naturally. If asked where to connect you, say you want Shadhin Housing. Do not invent routing or game facts."
            }
            Self::ArnabDirectory => {
                "You are Arnab Bhattacharjee calling from Shadhin Housing. You want to speak with Bela Bose, but do not know her current housing. Support a short multi-turn conversation. If asked about her ID, say it is around 1024 or something like that. If asked whether she has a dog or cat, say you remember that Bela has a cat. Do not reveal the directory ID directly unless the operator obtains it through lookup. Do not invent routing or game facts."
            }
            Self::Completed => "The story is complete; do not generate another story reply.",
        }
    }
}
