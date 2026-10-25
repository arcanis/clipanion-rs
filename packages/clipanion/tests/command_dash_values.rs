use clipanion::{prelude::*, test_cli_failure, test_cli_success, Error};

#[cli::command]
#[cli::path("commit")]
struct CommitCommand {
    #[cli::option("-m,--message")]
    message: Option<String>,

    #[cli::option("-n,--count", default = 1)]
    count: i64,

    #[cli::option("--offset")]
    offset: Option<i64>,

    #[cli::option("-v,--verbose", default = false)]
    verbose: bool,

    #[cli::option("-a,--all", default = false)]
    all: bool,

    paths: Vec<String>,
}

impl CommitCommand {
    fn execute(&self) {
    }
}

#[cli::command]
#[cli::path("set")]
struct SetCommand {
    key: String,
    value: String,
}

impl SetCommand {
    fn execute(&self) {
    }
}

#[cli::command]
#[cli::path("add")]
struct AddCommand {
    left: i64,
    right: f64,
}

impl AddCommand {
    fn execute(&self) {
    }
}

#[cli::program]
enum MyCli {
    Commit(CommitCommand),
    Set(SetCommand),
    Add(AddCommand),
}

// An option's value can start with a dash.

test_cli_success!(it_takes_a_value_starting_with_a_dash, MyCli, CommitCommand, &["commit", "-m", "- Fix the thing"], |command| {
    assert_eq!(command.message.as_deref(), Some("- Fix the thing"));
});

test_cli_success!(it_takes_a_negative_number_as_a_value, MyCli, CommitCommand, &["commit", "--offset", "-5", "--count", "-1"], |command| {
    assert_eq!(command.offset, Some(-5));
    assert_eq!(command.count, -1);
});

test_cli_success!(it_takes_a_word_that_isnt_an_option_as_a_value, MyCli, CommitCommand, &["commit", "-m", "-x", "--verbose"], |command| {
    assert_eq!(command.message.as_deref(), Some("-x"));
    assert!(command.verbose);
});

test_cli_failure!(it_doesnt_take_an_option_as_a_value, MyCli, &["commit", "-m", "--verbose"], |error| {
    assert!(matches!(error, Error::NotFound(_)), "{error:?}");
});

test_cli_failure!(it_doesnt_take_the_end_of_options_as_a_value, MyCli, &["commit", "-m", "--", "x"], |error| {
    assert!(matches!(error, Error::NotFound(_)), "{error:?}");
});

test_cli_success!(it_takes_an_option_as_a_value_when_bound, MyCli, CommitCommand, &["commit", "--message=--verbose"], |command| {
    assert_eq!(command.message.as_deref(), Some("--verbose"));
    assert!(!command.verbose);
});

// Short options can have their value attached.

test_cli_success!(it_reads_short_options_with_a_separate_value, MyCli, CommitCommand, &["commit", "-n", "5"], |command| {
    assert_eq!(command.count, 5);
});

test_cli_success!(it_reads_short_options_with_a_bound_value, MyCli, CommitCommand, &["commit", "-n=5"], |command| {
    assert_eq!(command.count, 5);
});

test_cli_success!(it_reads_short_options_with_an_attached_value, MyCli, CommitCommand, &["commit", "-n5"], |command| {
    assert_eq!(command.count, 5);
});

test_cli_success!(it_reads_short_options_with_an_attached_negative_value, MyCli, CommitCommand, &["commit", "-n-5"], |command| {
    assert_eq!(command.count, -5);
});

test_cli_success!(it_reads_flags_before_an_attached_value, MyCli, CommitCommand, &["commit", "-van5"], |command| {
    assert!(command.verbose);
    assert!(command.all);
    assert_eq!(command.count, 5);
});

test_cli_success!(it_reads_flags_before_a_separate_value, MyCli, CommitCommand, &["commit", "-am", "Fix it", "src"], |command| {
    assert!(command.all);
    assert_eq!(command.message.as_deref(), Some("Fix it"));
    assert_eq!(command.paths, vec!["src"]);
});

test_cli_success!(it_takes_the_rest_of_a_cluster_as_the_value, MyCli, CommitCommand, &["commit", "-mv"], |command| {
    assert_eq!(command.message.as_deref(), Some("v"));
    assert!(!command.verbose);
});

test_cli_failure!(it_rejects_unknown_letters_in_a_cluster, MyCli, &["commit", "-vx"], |error| {
    assert!(matches!(error, Error::NotFound(_)), "{error:?}");
});

// Positionals can be negative numbers, or `-`, but not unknown options.

test_cli_success!(it_takes_a_negative_number_as_a_positional, MyCli, SetCommand, &["set", "retries", "-1"], |command| {
    assert_eq!(command.key, "retries");
    assert_eq!(command.value, "-1");
});

test_cli_success!(it_takes_negative_numbers_as_numeric_positionals, MyCli, AddCommand, &["add", "-2", "-0.5"], |command| {
    assert_eq!(command.left, -2);
    assert_eq!(command.right, -0.5);
});

test_cli_success!(it_takes_a_dash_as_a_positional, MyCli, SetCommand, &["set", "-", "-"], |command| {
    assert_eq!(command.key, "-");
    assert_eq!(command.value, "-");
});

test_cli_success!(it_reads_options_between_negative_positionals, MyCli, CommitCommand, &["commit", "-1", "-v", "-2"], |command| {
    assert!(command.verbose);
    assert_eq!(command.paths, vec!["-1", "-2"]);
});

test_cli_failure!(it_doesnt_take_an_unknown_option_as_a_positional, MyCli, &["set", "key", "--bogus"], |error| {
    assert!(matches!(error, Error::NotFound(_)), "{error:?}");
});

// `--` ends the options.

test_cli_success!(it_reads_options_until_the_end_of_options, MyCli, CommitCommand, &["commit", "-v", "--", "-a", "--", "-m"], |command| {
    assert!(command.verbose);
    assert!(!command.all);
    assert_eq!(command.paths, vec!["-a", "--", "-m"]);
});
