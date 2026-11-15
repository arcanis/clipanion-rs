use clipanion::{prelude::*, test_cli_failure, test_cli_success, BuiltinCommand, Error};
use clipanion::core::SelectionResult;

/// Runs a script: once its name is read, every word is the script's.
#[cli::command(default, proxy)]
#[cli::path("run")]
struct RunCommand {
    #[cli::option("-p,--parallel", default = false)]
    parallel: bool,

    name: String,
    args: Vec<String>,
}

impl RunCommand {
    fn execute(&self) {
    }
}

/// The same, with two positionals before the proxy.
#[cli::command(proxy)]
#[cli::path("copy")]
struct CopyCommand {
    #[cli::option("-f,--force", default = false)]
    force: bool,

    from: String,
    to: String,
    args: Vec<String>,
}

impl CopyCommand {
    fn execute(&self) {
    }
}

/// The same, with an optional positional before the proxy.
#[cli::command(proxy)]
#[cli::path("init")]
struct InitCommand {
    #[cli::option("-y,--yes", default = false)]
    yes: bool,

    template: Option<String>,
    args: Vec<String>,
}

impl InitCommand {
    fn execute(&self) {
    }
}

/// The same, as a proxy to a program.
#[cli::command(proxy_program)]
#[cli::path("exec")]
struct ExecCommand {
    #[cli::option("-q,--quiet", default = false)]
    quiet: bool,

    env: String,
    command: Vec<String>,
}

impl ExecCommand {
    fn execute(&self) {
    }
}

#[cli::program]
enum MyCli {
    Run(RunCommand),
    Copy(CopyCommand),
    Init(InitCommand),
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

// Once the positional is read, the proxy started: it takes the command's own
// options, the help, and `--`.

test_cli_success!(it_forwards_the_commands_options_after_the_positional, MyCli, RunCommand, &["run", "build", "-p", "x"], |command| {
    assert!(!command.parallel);
    assert_eq!(command.name, "build");
    assert_eq!(command.args, vec!["-p", "x"]);
});

test_cli_success!(it_forwards_long_options_after_the_positional, MyCli, RunCommand, &["run", "build", "--parallel"], |command| {
    assert!(!command.parallel);
    assert_eq!(command.args, vec!["--parallel"]);
});

test_cli_success!(it_forwards_the_help_after_the_positional, MyCli, RunCommand, &["run", "build", "--help"], |command| {
    assert_eq!(command.args, vec!["--help"]);
});

test_cli_success!(it_forwards_the_short_help_after_the_positional, MyCli, RunCommand, &["run", "build", "-h"], |command| {
    assert_eq!(command.args, vec!["-h"]);
});

test_cli_success!(it_forwards_the_end_of_options_after_the_positional, MyCli, RunCommand, &["run", "build", "--", "--watch"], |command| {
    assert_eq!(command.args, vec!["--", "--watch"]);
});

test_cli_success!(it_forwards_unknown_options_after_the_positional, MyCli, RunCommand, &["run", "build", "--frobnicate", "-p"], |command| {
    assert!(!command.parallel);
    assert_eq!(command.args, vec!["--frobnicate", "-p"]);
});

#[test]
fn it_doesnt_give_the_help_after_the_positional() {
    assert_eq!(help_for(&["run", "build", "--help"]), None);
    assert_eq!(help_for(&["run", "build", "-h"]), None);
}

// Before the positional, the options are read, and `--` ends them.

test_cli_success!(it_reads_the_options_before_the_positional, MyCli, RunCommand, &["run", "-p", "build", "x"], |command| {
    assert!(command.parallel);
    assert_eq!(command.name, "build");
    assert_eq!(command.args, vec!["x"]);
});

test_cli_success!(it_ends_the_options_before_the_positional, MyCli, RunCommand, &["run", "--", "build", "-p"], |command| {
    assert!(!command.parallel);
    assert_eq!(command.name, "build");
    assert_eq!(command.args, vec!["-p"]);
});

test_cli_success!(it_reads_a_dashed_positional_after_the_end_of_options, MyCli, RunCommand, &["run", "-p", "--", "-build", "--", "x"], |command| {
    assert!(command.parallel);
    assert_eq!(command.name, "-build");
    assert_eq!(command.args, vec!["--", "x"]);
});

test_cli_failure!(it_rejects_an_unknown_option_before_the_positional, MyCli, &["run", "--frobnicate", "build"], |error| {
    assert!(matches!(error, Error::NotFound(_)), "{:?}", error);
});

#[test]
fn it_gives_the_help_before_the_positional() {
    assert_eq!(help_for(&["run", "--help"]), Some(vec!["run".to_string()]));
    assert_eq!(help_for(&["run", "-h"]), Some(vec!["run".to_string()]));
    assert_eq!(help_for(&["run", "-p", "--help"]), Some(vec!["run".to_string()]));
}

// Without the path: the same.

test_cli_success!(it_forwards_the_commands_options_without_the_path, MyCli, RunCommand, &["build", "-p", "--help", "--", "x"], |command| {
    assert!(!command.parallel);
    assert_eq!(command.name, "build");
    assert_eq!(command.args, vec!["-p", "--help", "--", "x"]);
});

test_cli_success!(it_reads_the_options_before_the_positional_without_the_path, MyCli, RunCommand, &["-p", "build", "-p"], |command| {
    assert!(command.parallel);
    assert_eq!(command.args, vec!["-p"]);
});

// With several positionals, the options are read until the last one.

test_cli_success!(it_reads_the_options_between_the_positionals, MyCli, CopyCommand, &["copy", "a", "-f", "b", "-f", "--", "x"], |command| {
    assert!(command.force);
    assert_eq!(command.from, "a");
    assert_eq!(command.to, "b");
    assert_eq!(command.args, vec!["-f", "--", "x"]);
});

// An optional positional starts the proxy when it's given; when it isn't, the
// proxy starts at the first word that isn't one of the command's options.

test_cli_success!(it_forwards_the_options_after_an_optional_positional, MyCli, InitCommand, &["init", "tpl", "-y", "--help"], |command| {
    assert!(!command.yes);
    assert_eq!(command.template.as_deref(), Some("tpl"));
    assert_eq!(command.args, vec!["-y", "--help"]);
});

test_cli_success!(it_reads_the_options_before_an_optional_positional, MyCli, InitCommand, &["init", "-y", "tpl", "-y"], |command| {
    assert!(command.yes);
    assert_eq!(command.template.as_deref(), Some("tpl"));
    assert_eq!(command.args, vec!["-y"]);
});

test_cli_success!(it_starts_the_proxy_without_the_optional_positional, MyCli, InitCommand, &["init", "-y", "--frobnicate", "-y"], |command| {
    assert!(command.yes);
    assert_eq!(command.template, None);
    assert_eq!(command.args, vec!["--frobnicate", "-y"]);
});

#[test]
fn it_gives_the_help_before_an_optional_positional() {
    assert_eq!(help_for(&["init", "--help"]), Some(vec!["init".to_string()]));
    assert_eq!(help_for(&["init", "tpl", "--help"]), None);
}

// A proxy to a program: the positional starts it too.

test_cli_success!(it_forwards_option_like_words_to_a_program_after_the_positional, MyCli, ExecCommand, &["exec", "prod", "-q", "--help"], |command| {
    assert!(!command.quiet);
    assert_eq!(command.env, "prod");
    assert_eq!(command.command, vec!["-q", "--help"]);
});

test_cli_success!(it_reads_the_options_before_the_programs_positional, MyCli, ExecCommand, &["exec", "-q", "prod", "make", "-q"], |command| {
    assert!(command.quiet);
    assert_eq!(command.command, vec!["make", "-q"]);
});

#[test]
fn it_gives_the_programs_help_before_the_positional() {
    assert_eq!(help_for(&["exec", "--help"]), Some(vec!["exec".to_string()]));
    assert_eq!(help_for(&["exec", "prod", "--help"]), None);
}
