//! What a word means among a command's options.
//!
//! Options aren't nodes of the state machine: the nodes where options can be
//! given are marked (`NodeOptions`), and the runner reads the words it gets
//! there against the command's spec. That keeps the machine small: it would
//! otherwise repeat every option, and the nodes for its values, at every place
//! options can appear.
//!
//! A word is an option when it's one of the command's option names, a name with
//! its value (`--rev=main`), a batch of short flags (`-abc`), `--`, or `-h` /
//! `--help`. An option's values are the words that follow its name.

use std::ops::Range;

use crate::builder::{CommandSpec, Component, OptionSpec};

/// An option a word sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OptionUse {
    pub option_id: usize,

    /// Where its name is in the word (`--rev`, or a letter of `-abc`).
    pub name: Range<usize>,

    /// Its value when it's in the same word (`--rev=main`): where the `=` is,
    /// and where the value is.
    pub value: Option<(Option<usize>, Range<usize>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OptionWord {
    /// `--`: the words after it aren't options.
    EndOfOptions,

    /// `-h` or `--help`, when the command doesn't define them.
    Help,

    /// Options (several for `-abc`). Only the last one can wait for values in
    /// the following words.
    Options(Vec<OptionUse>),
}

/// The values an option waits for, in the words following its name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PendingValues {
    /// How many it needs.
    pub required: usize,

    /// How many more it can take after those (`None`: any number).
    pub extra: Option<usize>,
}

/// Whether `option` takes values.
pub(crate) fn takes_values(option: &OptionSpec) -> bool {
    option.min_len > 0 || option.extra_len != Some(0)
}

/// Whether `option` takes a single value, which can then be in the same word as
/// its name (`--rev=main`).
pub(crate) fn takes_bound_value(option: &OptionSpec) -> bool {
    option.min_len + option.extra_len.unwrap_or(0) == 1
}

/// The values `option` waits for after its name, and whether it can also go
/// without them.
pub(crate) fn values_after_name(option: &OptionSpec) -> Option<(PendingValues, bool)> {
    if !takes_values(option) {
        return None;
    }

    let pending = PendingValues {
        required: option.min_len,
        extra: option.extra_len,
    };

    Some((pending, option.allow_boolean && option.min_len > 0))
}

fn options_of(spec: &CommandSpec) -> impl Iterator<Item = (usize, &OptionSpec)> {
    spec.components.iter()
        .enumerate()
        .filter_map(|(id, component)| match component {
            Component::Option(option) => Some((id, option)),
            _ => None,
        })
}

/// The option named `name` (`--rev`, `-n`).
fn find_option<'cmds>(spec: &'cmds CommandSpec, name: &str) -> Option<(usize, &'cmds OptionSpec)> {
    options_of(spec)
        .find(|(_, option)| option.all_names().any(|candidate| candidate == name))
}

/// The option named `-` followed by `letter`.
fn find_short_option(spec: &CommandSpec, letter: char) -> Option<(usize, &OptionSpec)> {
    options_of(spec).find(|(_, option)| option.all_names().any(|name| {
        let mut chars = name.chars();
        chars.next() == Some('-') && chars.next() == Some(letter) && chars.next().is_none()
    }))
}

fn is_help_flag(spec: &CommandSpec, arg: &str) -> bool {
    matches!(arg, "-h" | "--help") && find_option(spec, arg).is_none()
}

/// Whether `arg` can be the value an option waits for.
pub(crate) fn accepts_value(arg: &str) -> bool {
    !arg.starts_with('-')
}

/// What `arg` is among the command's options, if it's a valid use of them.
/// `help`: whether `-h` and `--help` ask for help here.
pub(crate) fn parse_option_word(spec: &CommandSpec, arg: &str, help: bool) -> Option<OptionWord> {
    if !arg.starts_with('-') {
        return None;
    }

    if arg == "--" {
        return Some(OptionWord::EndOfOptions);
    }

    if help && is_help_flag(spec, arg) {
        return Some(OptionWord::Help);
    }

    if let Some((option_id, _)) = find_option(spec, arg) {
        return Some(OptionWord::Options(vec![OptionUse {
            option_id,
            name: 0..arg.len(),
            value: None,
        }]));
    }

    // `--rev=main`
    if let Some((name, _)) = arg.split_once('=') {
        if let Some((option_id, option)) = find_option(spec, name) {
            return takes_bound_value(option).then(|| OptionWord::Options(vec![OptionUse {
                option_id,
                name: 0..name.len(),
                value: Some((Some(name.len()), name.len() + 1..arg.len())),
            }]));
        }
    }

    // `-abc`, for the flags `-a`, `-b` and `-c`.
    if arg.starts_with("--") || arg.len() <= 2 {
        return None;
    }

    let uses = arg.char_indices()
        .skip(1)
        .map(|(offset, letter)| match find_short_option(spec, letter) {
            Some((option_id, option)) if !takes_values(option) => Some(OptionUse {
                option_id,
                name: offset..offset + letter.len_utf8(),
                value: None,
            }),

            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;

    Some(OptionWord::Options(uses))
}
