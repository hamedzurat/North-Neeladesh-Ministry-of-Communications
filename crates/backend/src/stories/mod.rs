pub mod neel_university;
pub mod shapla_apartments;

pub fn known_thread(thread_id: &str) -> bool {
    matches!(thread_id, "shapla_apartments" | "neel_university")
}
