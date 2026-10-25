use std::{convert::Infallible, fmt::Display, future::Future};

use clipanion_core::{CommandError, CommandSpec, SelectionResult};

use crate::advanced::Environment;

pub fn handle_parse_error<E: Display + 'static>(err: E) -> CommandError {
    match std::any::TypeId::of::<E>() == std::any::TypeId::of::<Infallible>() {
        true => unreachable!("Infallible error occurred"),
        false => CommandError::Custom(err.to_string()),
    }
}

/**
 * Internal trait used to convert whatever the `execute()` function returns
 * into an exit code. It makes it easier to return `()` from simple commands
 * without having to return a specific number.
 */
#[derive(Debug)]
pub struct CommandResult {
    pub exit_code: std::process::ExitCode,
    pub error_message: Option<String>,
}

impl From<()> for CommandResult {
    fn from(_: ()) -> Self {
        Self {
            exit_code: std::process::ExitCode::SUCCESS,
            error_message: None,
        }
    }
}

impl From<CommandError> for CommandResult {
    fn from(error: CommandError) -> Self {
        Self {
            exit_code: std::process::ExitCode::FAILURE,
            error_message: Some(error.to_string()),
        }
    }
}

impl From<std::process::ExitCode> for CommandResult {
    fn from(exit_code: std::process::ExitCode) -> Self {
        Self {
            exit_code,
            error_message: None,
        }
    }
}

impl From<std::process::ExitStatus> for CommandResult {
    fn from(exit_status: std::process::ExitStatus) -> Self {
        Self {
            exit_code: std::process::ExitCode::from(exit_status.code().unwrap_or(1) as u8),
            error_message: None,
        }
    }
}

#[cfg(feature = "anyhow")]
impl<T: Into<CommandResult>> From<Result<T, anyhow::Error>> for CommandResult {
    fn from(value: Result<T, anyhow::Error>) -> Self {
        match value {
            Ok(value) => value.into(),

            Err(err) => Self {
                exit_code: std::process::ExitCode::FAILURE,
                error_message: Some(format!("{:?}", err)),
            },
        }
    }
}

#[cfg(not(feature = "anyhow"))]
impl<T: Into<CommandResult>, E: std::fmt::Display> From<Result<T, E>> for CommandResult {
    fn from(value: Result<T, E>) -> Self {
        match value {
            Ok(value) => value.into(),

            Err(err) => {
                let message = format!("{:#}", err);
                let maybe_message = if message.is_empty() { None } else { Some(message) };

                Self {
                    exit_code: std::process::ExitCode::FAILURE,
                    error_message: maybe_message,
                }
            },
        }
    }
}

/**
 * Internal trait implemented by the #[command] attribute.
 */
pub trait CommandController {
    type Partial;

    fn command_usage(opts: clipanion_core::CommandUsageOptions) -> Result<clipanion_core::CommandUsageResult<'static>, clipanion_core::BuildError>;
    fn command_spec() -> Result<&'static CommandSpec, clipanion_core::BuildError>;
    fn hydrate_from_state(environment: &Environment, state: &clipanion_core::State) -> Result<Self::Partial, clipanion_core::CommandError> where Self: Sized;
}

pub trait FromCommand<T> {
    fn from_command(command: T, command_spec: &'static CommandSpec) -> Self;
}

pub trait CliEnums {
    type PartialEnum;
    type Enum: TryFrom<Self::PartialEnum, Error = CommandError>;
}

/**
 * Internal traits implemented by the `cli::program` macro. Used to
 * statically aggregate multiple commands together in a single `Cli` type.
 */
pub trait CommandProvider {
    type Command;

    fn command_usage(command_index: usize, opts: clipanion_core::CommandUsageOptions) -> Result<clipanion_core::CommandUsageResult<'static>, clipanion_core::BuildError>;

    fn registered_commands() -> Result<Vec<&'static CommandSpec>, clipanion_core::BuildError>;
    fn parse_args<'args>(builder: &clipanion_core::CliBuilder<'static>, environment: &'args Environment) -> Result<SelectionResult<'static, 'args, Self::PartialEnum>, clipanion_core::Error<'args>> where Self: Sized + CliEnums;
    fn build_cli() -> Result<clipanion_core::CliBuilder<'static>, clipanion_core::BuildError>;
}

pub trait CommandExecutor {
    fn execute(self, env: &Environment) -> crate::details::CommandResult;
}

pub trait CommandExecutorAsync {
    fn execute(self, env: &Environment) -> impl Future<Output = crate::details::CommandResult>;
}
