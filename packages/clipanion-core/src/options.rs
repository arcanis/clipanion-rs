//! What a word means among a command's options.
//!
//! Options aren't nodes of the state machine: the nodes where options can be
//! given are marked (`NodeOptions`), and the runner reads the words it gets
//! there against the command's spec. That keeps the machine small (it would
//! otherwise repeat every option at every place options can appear), and lets
//! the parser know when a word is one of the command's options, which is what
//! decides whether a word starting with a dash can be a value:
//!
//! - a word is an option when it's one of the command's option names, a name
//!   with its value (`--rev=main`, `-n=5`, `-n5`), a cluster of short options
//!   (`-abc`), `--`, or `-h` / `--help`;
//! - an option's required values can be any other word (`-m "- Fix"`,
//!   `--offset -5`);
//! - its optional values and the positionals can be words that don't start
//!   with a dash, `-` alone, or negative numbers (`-1`, `-0.5`).

use std::ops::Range;

use crate::builder::{CommandSpec, Component, OptionSpec};

/// An option a word sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OptionUse {
    pub option_id: usize,

    /// Where its name is in the word (`--rev`, `-n`, or a letter of `-abc`).
    pub name: Range<usize>,

    /// Its value when it's in the same word (`--rev=main`, `-n5`): where the
    /// `=` is, if there's one, and where the value is.
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

    /// Whether its required values can be any word that isn't an option, rather
    /// than only those a positional could take. Options that can also go
    /// without values (`Option<Option<T>>`) only take the latter.
    pub strict: bool,
}

/// Whether `option` takes values.
pub(crate) fn takes_values(option: &OptionSpec) -> bool {
    option.min_len > 0 || option.extra_len != Some(0)
}

/// Whether `option` takes a single value, which can then be in the same word as
/// its name (`--rev=main`, `-n5`).
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
        strict: !option.allow_boolean,
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

/// `-1`, `-0.5`, `-1e3`: a value, unless the command has an option of that name.
pub(crate) fn is_negative_number(arg: &str) -> bool {
    arg.strip_prefix('-')
        .is_some_and(|number| number.starts_with(|c: char| c.is_ascii_digit() || c == '.') && number.parse::<f64>().is_ok())
}

/// Whether `arg` is one of the command's options, even used wrongly (`--json=1`
/// for a flag), or `--`, or (with `help`) `-h` or `--help`: such a word is never
/// a value.
pub(crate) fn is_option_word(spec: &CommandSpec, arg: &str, help: bool) -> bool {
    if !arg.starts_with('-') || arg == "-" {
        return false;
    }

    if arg == "--" || (help && matches!(arg, "-h" | "--help")) {
        return true;
    }

    let name = arg.split_once('=')
        .map_or(arg, |(name, _)| name);

    if find_option(spec, name).is_some() {
        return true;
    }

    // `-n5`, `-abc`: a short option, with its value or with other ones.
    !arg.starts_with("--") && arg[1..].chars().next().is_some_and(|letter| find_short_option(spec, letter).is_some())
}

/// Whether `arg` can be a positional, or an option's optional value: a word that
/// doesn't start with a dash, `-` alone, or a negative number that isn't one of
/// the command's options.
pub(crate) fn is_value_word(spec: &CommandSpec, arg: &str) -> bool {
    !arg.starts_with('-') || arg == "-" || (is_negative_number(arg) && !is_option_word(spec, arg, true))
}

/// Whether `arg` can be the value an option waits for.
pub(crate) fn accepts_value(spec: &CommandSpec, pending: &PendingValues, arg: &str) -> bool {
    if pending.required > 0 && pending.strict {
        !is_option_word(spec, arg, true)
    } else {
        is_value_word(spec, arg)
    }
}

/// What `arg` is among the command's options, if it's a valid use of them.
/// `help`: whether `-h` and `--help` ask for help here.
pub(crate) fn parse_option_word(spec: &CommandSpec, arg: &str, help: bool) -> Option<OptionWord> {
    if !arg.starts_with('-') || arg == "-" {
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

    // `--rev=main`, `-n=5`
    if let Some((name, _)) = arg.split_once('=') {
        if let Some((option_id, option)) = find_option(spec, name) {
            return takes_bound_value(option).then(|| OptionWord::Options(vec![OptionUse {
                option_id,
                name: 0..name.len(),
                value: Some((Some(name.len()), name.len() + 1..arg.len())),
            }]));
        }
    }

    if arg.starts_with("--") {
        return None;
    }

    // Short options, getopt-style: `-abc` sets `-a`, `-b` and `-c`, `-n5` sets
    // `-n` to 5, `-vn5` sets `-v` and `-n` to 5, and `-vn 5` too.
    let mut uses = vec![];

    for (offset, letter) in arg.char_indices().skip(1) {
        let (option_id, option) = find_short_option(spec, letter)?;

        let name = match offset {
            1 => 0..2,
            _ => offset..offset + letter.len_utf8(),
        };

        let rest_offset = offset + letter.len_utf8();

        if !takes_values(option) || rest_offset == arg.len() {
            uses.push(OptionUse {option_id, name, value: None});

            if takes_values(option) {
                break;
            }

            continue;
        }

        if !takes_bound_value(option) {
            return None;
        }

        let (assign, value_offset) = match arg[rest_offset..].starts_with('=') {
            true => (Some(rest_offset), rest_offset + 1),
            false => (None, rest_offset),
        };

        uses.push(OptionUse {option_id, name, value: Some((assign, value_offset..arg.len()))});
        break;
    }

    Some(OptionWord::Options(uses))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> CommandSpec {
        CommandSpec {
            components: vec![
                Component::Option(OptionSpec::boolean("-v,--verbose")),
                Component::Option(OptionSpec::boolean("-a")),
                Component::Option(OptionSpec::parametrized("-n,--count")),
                Component::Option(OptionSpec::parametrized("-m,--message")),
            ],
            ..Default::default()
        }
    }

    fn uses(arg: &str) -> Vec<(usize, &str, Option<&str>)> {
        match parse_option_word(&spec(), arg, true) {
            Some(OptionWord::Options(uses)) => uses.into_iter()
                .map(|option_use| (option_use.option_id, &arg[option_use.name], option_use.value.map(|(_, value)| &arg[value])))
                .collect(),

            other => panic!("{arg}: {other:?}"),
        }
    }

    #[test]
    fn it_reads_names_and_bound_values() {
        assert_eq!(uses("--verbose"), [(0, "--verbose", None)]);
        assert_eq!(uses("-n"), [(2, "-n", None)]);
        assert_eq!(uses("--count=5"), [(2, "--count", Some("5"))]);
        assert_eq!(uses("-n=5"), [(2, "-n", Some("5"))]);
        assert_eq!(uses("--message="), [(3, "--message", Some(""))]);
    }

    #[test]
    fn it_reads_short_option_clusters() {
        assert_eq!(uses("-va"), [(0, "-v", None), (1, "a", None)]);
        assert_eq!(uses("-n5"), [(2, "-n", Some("5"))]);
        assert_eq!(uses("-n-5"), [(2, "-n", Some("-5"))]);
        assert_eq!(uses("-vn5"), [(0, "-v", None), (2, "n", Some("5"))]);
        assert_eq!(uses("-vn=5"), [(0, "-v", None), (2, "n", Some("5"))]);
        assert_eq!(uses("-vm"), [(0, "-v", None), (3, "m", None)]);
        assert_eq!(uses("-mv"), [(3, "-m", Some("v"))]);
    }

    #[test]
    fn it_tells_options_from_values() {
        let spec = spec();

        assert_eq!(parse_option_word(&spec, "--", true), Some(OptionWord::EndOfOptions));
        assert_eq!(parse_option_word(&spec, "-h", true), Some(OptionWord::Help));
        assert_eq!(parse_option_word(&spec, "-h", false), None);

        // Not options: they can be values.
        for arg in ["-1", "-0.5", "-", "- Fix the thing", "--bogus", "-x"] {
            assert_eq!(parse_option_word(&spec, arg, true), None, "{arg}");
        }

        // Options used wrongly aren't values either.
        for arg in ["--verbose=1", "-vx", "-vq"] {
            assert_eq!(parse_option_word(&spec, arg, true), None, "{arg}");
            assert!(is_option_word(&spec, arg, true), "{arg}");
        }

        assert!(is_value_word(&spec, "-1"));
        assert!(is_value_word(&spec, "-"));
        assert!(!is_value_word(&spec, "-x"));
        assert!(!is_value_word(&spec, "--bogus"));

        let strict = PendingValues {required: 1, extra: Some(0), strict: true};
        assert!(accepts_value(&spec, &strict, "- Fix the thing"));
        assert!(accepts_value(&spec, &strict, "-x"));
        assert!(!accepts_value(&spec, &strict, "--verbose"));
        assert!(!accepts_value(&spec, &strict, "--"));
        assert!(!accepts_value(&spec, &strict, "--help"));
    }

    #[test]
    fn it_lets_commands_define_digits() {
        let spec = CommandSpec {
            components: vec![Component::Option(OptionSpec::boolean("-1"))],
            ..Default::default()
        };

        assert!(is_option_word(&spec, "-1", true));
        assert!(!is_value_word(&spec, "-1"));
        assert!(is_value_word(&spec, "-2"));
    }
}
