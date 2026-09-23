use std::env;
use std::fs;

use serde::Deserialize;

pub const LINES: u8 = 12;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GameConfig {
    #[serde(default = "default_subscribers")]
    pub(crate) subscribers: Vec<SubscriberConfig>,
    pub(crate) active_calls: usize,
    pub(crate) patience_min_seconds: u64,
    pub(crate) patience_max_seconds: u64,
    pub(crate) ring_grace_seconds: u64,
    pub(crate) shift_duration_seconds: u64,
    pub(crate) call_arrival_interval_seconds: u64,
    #[serde(default = "default_story_seed")]
    pub(crate) story_seed: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SubscriberConfig {
    pub(crate) line: u8,
    pub(crate) place: String,
    pub(crate) name: String,
    pub(crate) role: String,
    #[serde(default)]
    pub(crate) voice_id: String,
}

fn default_story_seed() -> u64 {
    1
}

fn default_subscribers() -> Vec<SubscriberConfig> {
    (0..LINES)
        .map(|line| SubscriberConfig {
            line,
            place: format!("LINE {line:02}"),
            name: format!("SUBSCRIBER {line:02}"),
            role: "unassigned".into(),
            voice_id: format!("pocket-line-{line}"),
        })
        .collect()
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            subscribers: default_subscribers(),
            active_calls: 3,
            patience_min_seconds: 32,
            patience_max_seconds: 64,
            ring_grace_seconds: 16,
            shift_duration_seconds: 90,
            call_arrival_interval_seconds: 0,
            story_seed: default_story_seed(),
        }
    }
}

impl GameConfig {
    pub(crate) fn load() -> Self {
        let path = env::var("NN_EXCHANGE_CONFIG").unwrap_or_else(|_| "exchange.toml".into());
        let Ok(contents) = fs::read_to_string(path) else {
            return Self::default();
        };
        let mut config: Self = toml::from_str(&contents).unwrap_or_else(|error| {
            eprintln!("exchange config ignored: {error}");
            Self::default()
        });
        let fallback_names = [
            "Ayesha Rahman",
            "Mithun Das",
            "Farzana Akter",
            "Rafiq Hasan",
        ];
        for subscriber in &mut config.subscribers {
            if subscriber.name.trim().is_empty() || subscriber.name.starts_with("SUBSCRIBER ") {
                subscriber.name =
                    fallback_names[usize::from(subscriber.line) % fallback_names.len()].into();
            }
            if subscriber.role.trim().is_empty() || subscriber.role == "unassigned" {
                subscriber.role = "senior emergency physician".into();
            }
        }
        config
    }
}
