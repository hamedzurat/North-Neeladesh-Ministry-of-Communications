//! Story-owned configuration and state machines.
//!
//! Each story module owns its callers, beats, prompts, authored audio, and
//! story-specific directory facts. Shared exchange mechanics belong outside
//! this module so adding a story does not duplicate the physical game rules.

pub mod neel_university;
pub mod shapla_apartments;
