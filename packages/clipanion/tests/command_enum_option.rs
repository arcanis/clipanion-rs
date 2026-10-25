use std::str::FromStr;

use clipanion::{prelude::*, test_cli_failure, test_cli_success, CommandError, Error};

// Option and positional values are read with `FromStr`: an enum takes values
// once it implements it. (The `serde` feature only serializes the command
// specs, for `--clipanion-commands`.)
#[derive(Debug, PartialEq, Eq)]
enum Strategy {
    Rebase,
    Squash,
    Merge,
}

impl FromStr for Strategy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "rebase" => Ok(Self::Rebase),
            "squash" => Ok(Self::Squash),
            "merge" => Ok(Self::Merge),
            _ => Err(format!("'{value}' isn't a strategy: rebase, squash or merge")),
        }
    }
}

#[cli::command]
#[cli::path("land")]
struct LandCommand {
    #[cli::option("--strategy", default = Strategy::Rebase)]
    strategy: Strategy,

    #[cli::option("--fallback")]
    fallback: Option<Strategy>,
}

impl LandCommand {
    fn execute(&self) {
    }
}

#[cli::program]
enum MyCli {
    Land(LandCommand),
}

test_cli_success!(it_uses_the_default, MyCli, LandCommand, &["land"], |command| {
    assert_eq!(command.strategy, Strategy::Rebase);
    assert_eq!(command.fallback, None);
});

test_cli_success!(it_reads_the_value, MyCli, LandCommand, &["land", "--strategy", "squash", "--fallback=merge"], |command| {
    assert_eq!(command.strategy, Strategy::Squash);
    assert_eq!(command.fallback, Some(Strategy::Merge));
});

test_cli_failure!(it_reports_an_invalid_value, MyCli, &["land", "--strategy", "yolo"], |error| {
    assert_eq!(error, Error::CommandError(LandCommand::command_spec().unwrap(), CommandError::Custom("'yolo' isn't a strategy: rebase, squash or merge".to_string())));
});
