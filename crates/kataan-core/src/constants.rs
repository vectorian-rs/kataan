/// The on-disk schema this build reads and writes.
///
/// Bumped to 0.2.0 by nested type scopes: `folders`, `extends`, and
/// folder-level `[type_folders]`. A vault declaring a newer minor than this is
/// refused at open, because the alternative is a TOML parser reporting a
/// missing field, which tells the reader nothing about the real problem.
pub const SCHEMA_VERSION: &str = "0.2.0";
pub const VAULT_CONFIG_FILE: &str = "kataan.toml";
pub const DEFAULT_MAX_FOLDER_DEPTH: usize = 4;

/// How deep any directory walk may recurse before it refuses to go further.
///
/// A structural backstop, not a policy: `limits.max_folder_depth` is what a
/// vault configures and `validate` reports on, and it is checked *after* the
/// walk. The walkers themselves recursed per directory with no bound, so a
/// pathologically nested tree — or a symlink loop the ignore rules did not
/// catch — aborted the process on stack overflow instead of returning an
/// error. Set far above any legitimate vault so it can only be reached by
/// something already wrong.
pub const MAX_WALK_DEPTH: usize = 64;

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

pub const TYPE_INTAKE: &str = "intake";
pub const TYPE_PROJECT: &str = "project";
pub const TYPE_PERSON: &str = "person";
pub const TYPE_NOTE: &str = "note";
pub const TYPE_TOPIC: &str = "topic";
pub const TYPE_CODE: &str = "code";
pub const TYPE_DEFINITION: &str = "type-definition";
pub const CODE_FOLDER: &str = "code";
pub const CORE_TYPES: &[&str] = &[
    TYPE_INTAKE,
    TYPE_PROJECT,
    TYPE_PERSON,
    TYPE_NOTE,
    TYPE_TOPIC,
    TYPE_CODE,
    TYPE_DEFINITION,
];
