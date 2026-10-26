use clipanion::{prelude::*, test_cli_failure, test_cli_success, BuiltinCommand, Error};
use clipanion::core::SelectionResult;
use clipanion::details::CommandProvider;

/// Runs a command, which never starts with a dash: everything option-like
/// before it is one of `bisect`'s options.
#[cli::command(proxy_program)]
#[cli::path("bisect")]
struct BisectCommand {
    #[cli::option("--good")]
    good: String,

    #[cli::option("--repo")]
    repo: Option<String>,

    #[cli::option("-v,--verbose", default = false)]
    verbose: bool,

    command: Vec<String>,
}

impl BisectCommand {
    fn execute(&self) {
    }
}

/// The same, as the proxies that take `-h` and `--help` right after their path.
#[cli::command(proxy)]
#[cli::path("exec")]
struct ExecCommand {
    command: Vec<String>,
}

impl ExecCommand {
    fn execute(&self) {
    }
}

#[cli::program]
enum MyCli {
    Bisect(BisectCommand),
    Exec(ExecCommand),
}

/// The commands `args` ask for the help of, if they do.
fn help_for(args: &[&str]) -> Option<Vec<String>> {
    let builder = MyCli::build_cli().unwrap();
    let env = clipanion::advanced::Environment::default()
        .with_argv(args.iter().map(|arg| arg.to_string()).collect());

    match MyCli::parse_args(&builder, &env) {
        Ok(SelectionResult::Builtin(BuiltinCommand::Help(specs))) => Some(specs.iter().map(|spec| spec.primary_path.join(" ")).collect()),
        _ => None,
    }
}

// Help, right after the path too.

#[test]
fn it_gives_the_help_right_after_the_path() {
    assert_eq!(help_for(&["bisect", "--help"]), Some(vec!["bisect".to_string()]));
    assert_eq!(help_for(&["bisect", "-h"]), Some(vec!["bisect".to_string()]));
}

#[test]
fn it_gives_the_help_among_the_options() {
    assert_eq!(help_for(&["bisect", "--good", "abc", "--help"]), Some(vec!["bisect".to_string()]));
    assert_eq!(help_for(&["bisect", "--repo=r", "-h", "--good", "abc"]), Some(vec!["bisect".to_string()]));
}

#[test]
fn it_still_forwards_the_help_for_other_proxies() {
    assert_eq!(help_for(&["exec", "--help"]), None);
}

test_cli_success!(it_still_forwards_the_help_of_other_proxies, MyCli, ExecCommand, &["exec", "--help"], |command| {
    assert_eq!(command.command, vec!["--help"]);
});

// The options before the program.

test_cli_success!(it_reads_the_options_before_the_program, MyCli, BisectCommand, &["bisect", "--good", "abc", "--repo", "r", "-v", "make", "-j4"], |command| {
    assert_eq!(command.good, "abc");
    assert_eq!(command.repo.as_deref(), Some("r"));
    assert!(command.verbose);
    assert_eq!(command.command, vec!["make", "-j4"]);
});

test_cli_failure!(it_rejects_an_unknown_option_before_the_program, MyCli, &["bisect", "--good", "abc", "--frobnicate", "make"], |error| {
    assert_eq!(error, Error::NotFound(vec![BisectCommand::command_spec().unwrap()]));
});

test_cli_failure!(it_rejects_a_program_that_looks_like_an_option, MyCli, &["bisect", "--good", "abc", "-x"], |error| {
    assert_eq!(error, Error::NotFound(vec![BisectCommand::command_spec().unwrap()]));
});

// `--`, then a program that starts with a dash.

test_cli_success!(it_starts_the_program_after_the_end_of_options, MyCli, BisectCommand, &["bisect", "--good", "abc", "--", "--weird-program", "--help"], |command| {
    assert_eq!(command.command, vec!["--weird-program", "--help"]);
});

// What isn't option-like starts the program, like it would be a positional.

test_cli_success!(it_starts_the_program_at_a_negative_number_or_a_dash, MyCli, BisectCommand, &["bisect", "--good", "abc", "-", "-1"], |command| {
    assert_eq!(command.command, vec!["-", "-1"]);
});

// Once the program started, every word is its own.

test_cli_success!(it_forwards_the_programs_help, MyCli, BisectCommand, &["bisect", "--good", "abc", "make", "--help"], |command| {
    assert!(!command.verbose);
    assert_eq!(command.command, vec!["make", "--help"]);
});

test_cli_success!(it_forwards_options_and_the_end_of_options, MyCli, BisectCommand, &["bisect", "--good", "abc", "yarn", "run", "-v", "--", "--watch"], |command| {
    assert!(!command.verbose);
    assert_eq!(command.command, vec!["yarn", "run", "-v", "--", "--watch"]);
});

#[test]
fn it_marks_the_proxy() {
    let specs = MyCli::registered_commands().unwrap();

    let proxy_program = |spec: &clipanion::core::CommandSpec| spec.components.iter().any(|component| matches!(component, clipanion::core::Component::Positional(clipanion::core::PositionalSpec::Dynamic {is_proxy: true, proxy_program: true, ..})));

    assert!(proxy_program(specs[0]));
    assert!(!proxy_program(specs[1]));
}
