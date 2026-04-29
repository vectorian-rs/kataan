pub const SCHEMA_VERSION: &str = "0.1.0";
pub const VAULT_CONFIG_FILE: &str = "kataan.toml";
pub const DEFAULT_MAX_FOLDER_DEPTH: usize = 4;

pub const ACTOR_HUMAN: &str = "human";
pub const ACTOR_AGENT: &str = "agent";
pub const ACTOR_SYSTEM: &str = "system";
pub const ACTOR_VALUES: &[&str] = &[ACTOR_HUMAN, ACTOR_AGENT, ACTOR_SYSTEM];

pub const STATUS_DRAFT: &str = "draft";
pub const STATUS_ACTIVE: &str = "active";
pub const STATUS_PAUSED: &str = "paused";
pub const STATUS_DONE: &str = "done";
pub const STATUS_ARCHIVED: &str = "archived";
pub const STATUS_VALUES: &[&str] = &[
    STATUS_DRAFT,
    STATUS_ACTIVE,
    STATUS_PAUSED,
    STATUS_DONE,
    STATUS_ARCHIVED,
];

pub const TYPE_RAW: &str = "raw";
pub const TYPE_PROJECT: &str = "project";
pub const TYPE_PERSON: &str = "person";
pub const TYPE_NOTE: &str = "note";
pub const TYPE_TOPIC: &str = "topic";
pub const TYPE_CODE: &str = "code";
pub const TYPE_DEFINITION: &str = "type-definition";
pub const CODE_FOLDER: &str = "code";
pub const CORE_TYPES: &[&str] = &[
    TYPE_RAW,
    TYPE_PROJECT,
    TYPE_PERSON,
    TYPE_NOTE,
    TYPE_TOPIC,
    TYPE_CODE,
    TYPE_DEFINITION,
];

pub fn is_code_folder(folder: &str) -> bool {
    folder == CODE_FOLDER
}
