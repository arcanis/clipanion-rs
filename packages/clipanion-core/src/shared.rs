#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "tokens", derive(serde::Serialize))]
pub struct UserArg<'a> {
    pub value: &'a str,
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "tokens", derive(serde::Serialize))]
pub enum Arg<'a> {
    StartOfInput,
    User(UserArg<'a>),
    EndOfInput,
    EndOfPartialInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArgKey<'a> {
    StartOfInput,
    User(&'a str),
    EndOfInput,
    EndOfPartialInput,
}

impl<'a> From<Arg<'a>> for ArgKey<'a> {
    fn from(arg: Arg<'a>) -> Self {
        match arg {
            Arg::StartOfInput => ArgKey::StartOfInput,
            Arg::User(UserArg {value, ..}) => ArgKey::User(value),
            Arg::EndOfInput => ArgKey::EndOfInput,
            Arg::EndOfPartialInput => ArgKey::EndOfPartialInput,
        }
    }
}

pub const HELP_COMMAND_INDEX: usize = usize::MAX;

pub const INITIAL_NODE_ID: usize = 0;
pub const SUCCESS_NODE_ID: usize = 1;
pub const ERROR_NODE_ID: usize = 2;
pub const CUSTOM_NODE_ID: usize = 3;

pub fn is_terminal_node(id: usize) -> bool {
    id == SUCCESS_NODE_ID || id == ERROR_NODE_ID
}

/// Whether `CLIPANION_DEBUG` is set. It's read once: `std::env::var` locks and
/// scans the environment, and the runner would ask for every transition.
pub fn is_debug_enabled() -> bool {
    static IS_DEBUG_ENABLED: std::sync::OnceLock<bool>
        = std::sync::OnceLock::new();

    *IS_DEBUG_ENABLED.get_or_init(|| std::env::var_os("CLIPANION_DEBUG").is_some())
}
