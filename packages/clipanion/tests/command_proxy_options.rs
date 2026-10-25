use clipanion::{prelude::*, test_cli_success};

#[cli::command(proxy)]
#[cli::path("bisect")]
struct BisectCommand {
    #[cli::option("--good")]
    good: String,

    #[cli::option("--repo")]
    repo: Option<String>,

    #[cli::option("--json", default = false)]
    json: bool,

    command: Vec<String>,
}

impl BisectCommand {
    fn execute(&self) {
    }
}

#[cli::program]
enum MyCli {
    Bisect(BisectCommand),
}

// The options before the proxy are read as options; `--` ends them, and isn't
// part of what the proxy takes.

test_cli_success!(it_reads_options_before_the_end_of_options, MyCli, BisectCommand, &["bisect", "--good", "x", "--repo", "r", "--", "make"], |command| {
    assert_eq!(command.good, "x");
    assert_eq!(command.repo.as_deref(), Some("r"));
    assert_eq!(command.command, vec!["make"]);
});

test_cli_success!(it_forwards_options_after_the_end_of_options, MyCli, BisectCommand, &["bisect", "--good", "x", "--", "--repo", "r"], |command| {
    assert_eq!(command.repo, None);
    assert_eq!(command.command, vec!["--repo", "r"]);
});

test_cli_success!(it_starts_the_proxy_at_the_first_word_that_isnt_an_option, MyCli, BisectCommand, &["bisect", "--good", "x", "--json", "make", "-j4", "--repo", "r"], |command| {
    assert!(command.json);
    assert_eq!(command.repo, None);
    assert_eq!(command.command, vec!["make", "-j4", "--repo", "r"]);
});

test_cli_success!(it_starts_the_proxy_at_an_unknown_option, MyCli, BisectCommand, &["bisect", "--good", "x", "--frobnicate", "--json"], |command| {
    assert!(!command.json);
    assert_eq!(command.command, vec!["--frobnicate", "--json"]);
});

test_cli_success!(it_keeps_the_end_of_options_once_the_proxy_started, MyCli, BisectCommand, &["bisect", "--good", "x", "yarn", "run", "--", "--watch"], |command| {
    assert_eq!(command.command, vec!["yarn", "run", "--", "--watch"]);
});

test_cli_success!(it_takes_options_given_with_their_values, MyCli, BisectCommand, &["bisect", "--repo=r", "--good", "x", "make"], |command| {
    assert_eq!(command.repo.as_deref(), Some("r"));
    assert_eq!(command.command, vec!["make"]);
});

test_cli_success!(it_takes_nothing_when_there_is_nothing_to_take, MyCli, BisectCommand, &["bisect", "--good", "x", "--"], |command| {
    assert_eq!(command.command, Vec::<String>::new());
});
