use std::env;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

pub const LINES: u8 = 12;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GameConfig {
    pub(crate) subscribers: Vec<SubscriberConfig>,
    pub(crate) active_calls: usize,
    pub(crate) patience_min_seconds: u64,
    pub(crate) patience_max_seconds: u64,
    pub(crate) ring_grace_seconds: u64,
    pub(crate) shift_duration_seconds: u64,
    pub(crate) call_arrival_interval_seconds: u64,
    pub(crate) story_seed: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SubscriberConfig {
    pub(crate) id: u16,
    pub(crate) line: u8,
    pub(crate) place: String,
    pub(crate) name: String,
    pub(crate) role: String,
    pub(crate) private_info: String,
    #[serde(default)]
    pub(crate) voice_id: String,
}

impl GameConfig {
    pub(crate) fn load() -> Self {
        let path = env::var("NN_EXCHANGE_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../exchange.toml")
            });
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read exchange config {path:?}: {error}"));
        let config: Self = toml::from_str(&contents)
            .unwrap_or_else(|error| panic!("invalid exchange config {path:?}: {error}"));
        config.validate(&path);
        config
    }

    fn validate(&self, path: &PathBuf) {
        assert!(
            self.active_calls > 0,
            "exchange config {path:?}: active_calls must be positive"
        );
        assert!(
            self.patience_min_seconds <= self.patience_max_seconds,
            "exchange config {path:?}: patience_min_seconds exceeds patience_max_seconds"
        );
        assert!(
            !self.subscribers.is_empty(),
            "exchange config {path:?}: subscribers are required"
        );
        let mut seen = [false; LINES as usize];
        let mut seen_ids = std::collections::HashSet::new();
        for subscriber in &self.subscribers {
            assert!(
                seen_ids.insert(subscriber.id),
                "exchange config {path:?}: duplicate subscriber id {}",
                subscriber.id
            );
            assert!(
                usize::from(subscriber.line) < seen.len(),
                "exchange config {path:?}: subscriber line {} is outside 0..{}",
                subscriber.line,
                LINES - 1
            );
            assert!(
                !seen[usize::from(subscriber.line)],
                "exchange config {path:?}: duplicate subscriber line {}",
                subscriber.line
            );
            assert!(
                !subscriber.place.trim().is_empty(),
                "exchange config {path:?}: line {} has no place",
                subscriber.line
            );
            assert!(
                !subscriber.name.trim().is_empty(),
                "exchange config {path:?}: line {} has no name",
                subscriber.line
            );
            assert!(
                !subscriber.role.trim().is_empty(),
                "exchange config {path:?}: line {} has no role",
                subscriber.line
            );
            assert!(
                !subscriber.private_info.trim().is_empty(),
                "exchange config {path:?}: line {} has no private_info",
                subscriber.line
            );
            assert!(
                !subscriber.voice_id.trim().is_empty(),
                "exchange config {path:?}: line {} has no voice_id",
                subscriber.line
            );
            seen[usize::from(subscriber.line)] = true;
        }
        assert!(
            seen.into_iter().all(|present| present),
            "exchange config {path:?}: every line 0..{} must be configured",
            LINES - 1
        );
    }
}
