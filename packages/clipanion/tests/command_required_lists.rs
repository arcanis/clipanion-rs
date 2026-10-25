use clipanion::{prelude::*, test_cli_failure, test_cli_success, CommandError, Error};

#[cli::command]
#[cli::path("add")]
struct AddCommand {
    #[cli::positional(min_len = 1)]
    dirs: Vec<String>,
}

impl AddCommand {
    fn execute(&self) {
    }
}

#[cli::command(proxy)]
#[cli::path("exec")]
struct ExecCommand {
    #[cli::option("--retries", default = 0)]
    retries: u32,

    #[cli::positional(min_len = 1)]
    command: Vec<String>,
}

impl ExecCommand {
    fn execute(&self) {
    }
}

#[cli::command]
#[cli::path("fetch")]
struct FetchCommand {
    // A list option without a default is required: it must be given at least once.
    #[cli::option("--header")]
    headers: Vec<String>,
}

impl FetchCommand {
    fn execute(&self) {
    }
}

#[cli::program]
enum MyCli {
    Add(AddCommand),
    Exec(ExecCommand),
    Fetch(FetchCommand),
}

test_cli_success!(it_takes_the_values_of_a_required_list, MyCli, AddCommand, &["add", "a", "b"], |command| {
    assert_eq!(command.dirs, vec!["a", "b"]);
});

test_cli_failure!(it_requires_a_value_for_a_required_list, MyCli, &["add"], |error| {
    assert_eq!(error, Error::NotFound(vec![AddCommand::command_spec().unwrap()]));
});

test_cli_success!(it_takes_the_values_of_a_required_proxy, MyCli, ExecCommand, &["exec", "--retries", "2", "--", "make", "-j4"], |command| {
    assert_eq!(command.retries, 2);
    assert_eq!(command.command, vec!["make", "-j4"]);
});

test_cli_failure!(it_requires_a_value_for_a_required_proxy, MyCli, &["exec", "--retries", "2", "--"], |error| {
    assert_eq!(error, Error::NotFound(vec![ExecCommand::command_spec().unwrap()]));
});

test_cli_success!(it_takes_the_values_of_a_required_list_option, MyCli, FetchCommand, &["fetch", "--header", "a", "--header=b"], |command| {
    assert_eq!(command.headers, vec!["a", "b"]);
});

test_cli_failure!(it_requires_a_list_option_without_a_default, MyCli, &["fetch"], |error| {
    assert_eq!(error, Error::CommandError(FetchCommand::command_spec().unwrap(), CommandError::MissingOptionArguments(vec!["--header".to_string()])));
});
