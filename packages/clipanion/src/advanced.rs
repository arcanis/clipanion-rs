use std::{collections::HashMap, future::Future};

use clipanion_core::{BuiltinCommand, Info, SelectionResult};

use crate::{details::{CliEnums, CommandExecutor, CommandExecutorAsync, CommandProvider}, format::{format_fading_title_line, Formatter}};

/**
 * Used to define the properties of the CLI. In general you can ignore this and
 * just use the `run_with_default()` function instead.
 */

 #[derive(Debug, Clone)]
 pub struct Environment {
    pub info: Info,
    pub argv: Vec<String>,
 }
 
impl Environment {
    pub fn with_program_name(mut self, program_name: String) -> Self {
        self.info.program_name = program_name;
        self
    }

    pub fn with_binary_name(mut self, binary_name: String) -> Self {
        self.info.binary_name = binary_name;
        self
    }

    pub fn with_version(mut self, version: String) -> Self {
        self.info.version = version;
        self
    }

    pub fn with_about(mut self, about: String) -> Self {
        self.info.about = about;
        self
    }

    pub fn with_argv(mut self, argv: Vec<String>) -> Self {
        self.argv = argv;
        self
    }
}

impl Default for Environment {
    fn default() -> Self {
        let binary_name = std::env::args()
            .next().unwrap()
            .split('/').last().unwrap()
            .to_string();

        let argv
            = std::env::args()
                .skip(1)
                .collect();

        Self {
            argv,
            info: Info {
                program_name: "my-program".to_string(),
                binary_name,
                version: "1.0.0".to_string(),
                about: "my-program is a program that does something".to_string(),
                colorized: true,
            },
        }
    }
}

fn report_error<'cmds, 'args, S: CommandProvider>(env: &Environment, err: clipanion_core::Error<'cmds>) -> std::process::ExitCode {
    match err {
        clipanion_core::Error::CommandError(command_spec, command_error) => {
            println!("{}", Formatter::<S>::format_error(&env.info, "Error", &command_error.to_string(), vec![command_spec]));
            std::process::ExitCode::FAILURE
        },

        clipanion_core::Error::AmbiguousSyntax(command_specs) => {
            println!("{}", Formatter::<S>::format_error(&env.info, "Error", &"The provided arguments are ambiguous and need to be refined further. Possible options are:", command_specs));
            std::process::ExitCode::FAILURE
        },

        clipanion_core::Error::InternalError => {
            std::process::ExitCode::FAILURE
        },

        clipanion_core::Error::NotFound(command_specs) => {
            println!("{}", Formatter::<S>::format_error(&env.info, "Error", &"The provided arguments don't match any known syntax; use `--help` to get a list of possible options", command_specs));
            std::process::ExitCode::FAILURE
        },
    }
}

fn handle_builtin<S: CommandProvider>(builtin: BuiltinCommand, env: &Environment) -> std::process::ExitCode {
    match builtin {
        BuiltinCommand::Version => {
            println!("{}", env.info.version);
            std::process::ExitCode::SUCCESS
        },

        BuiltinCommand::Help(commands) => {
            println!("{}", format_fading_title_line(&format!("{} - {}", env.info.program_name, env.info.version), 80, 50));

            let commands = match commands.is_empty() {
                true => S::registered_commands().unwrap(),
                false => commands,
            };

            let default_command
                = commands.iter()
                    .find(|command| command.is_default())
                    .cloned();

            if let Some(default_command) = default_command {
                println!("");
                println!("  {}", default_command.usage().oneliner(&env.info));
            }

            let mut commands_by_category
                = HashMap::<_, Vec<_>>::new();

            for command in &commands {
                if command.description.is_some() {
                    let category = command.category
                        .as_ref()
                        .map(|category| category.as_ref());

                    commands_by_category.entry(category)
                        .or_default()
                        .push(command);
                }
            }

            let mut categories = commands_by_category.into_iter()
                .collect::<Vec<_>>();

            categories.sort_by(|a, b| {
                a.0.cmp(&b.0)
            });

            for (category, commands) in &categories {
                let category = category
                    .unwrap_or("General commands");

                println!("");
                println!("{}", format_fading_title_line(category, 80, 50));

                let mut commands_and_paths
                    = commands.into_iter()
                        .map(|command| (command.longest_path(), command))
                        .collect::<Vec<_>>();

                commands_and_paths.sort_by(|a, b| {
                    a.0.cmp(&b.0)
                });

                for command in commands {
                    if let Some(usage) = &command.description {
                        println!("");
                        println!("  \x1b[1m{}\x1b[0m", command.usage().oneliner(&env.info));
                        println!("    {}", usage);
                    }
                }
            }

            std::process::ExitCode::SUCCESS
        },
    }
}

pub trait Cli {
    fn run_builtin(env: Environment, builtin: BuiltinCommand) -> std::process::ExitCode;
    fn run(env: Environment) -> std::process::ExitCode;
    fn run_default() -> std::process::ExitCode;
}

impl<S> Cli for S where S: CliEnums + CommandProvider, S::Enum: CommandExecutor {
    fn run_builtin(env: Environment, builtin: BuiltinCommand) -> std::process::ExitCode {
        handle_builtin::<S>(builtin, &env)
    }

    fn run(env: Environment) -> std::process::ExitCode {
        let builder = S::build_cli()
            .unwrap();

        let parse_result
            = S::parse_args(&builder, &env);

        match parse_result {
            Ok(SelectionResult::Builtin(builtin)) => {
                Self::run_builtin(env, builtin)
            },

            Ok(SelectionResult::Command(command_spec, _, partial_command)) => {
                let full_command = match <S as CliEnums>::Enum::try_from(partial_command) {
                    Ok(full_command)
                        => full_command,

                    Err(err)
                        => return report_error::<S>(&env, clipanion_core::Error::CommandError(command_spec, err)),
                };

                let command_result
                    = full_command.execute(&env);

                if let Some(error_message) = &command_result.error_message {
                    return report_error::<S>(&env, clipanion_core::Error::CommandError(command_spec, error_message.to_string().into()));
                }

                command_result.exit_code
            },

            Err(err) => {
                report_error::<S>(&env, err)
            },
        }
    }

    fn run_default() -> std::process::ExitCode {
        Self::run(Default::default())
    }
}

pub trait CliAsync {
    fn run_builtin(env: Environment, builtin: BuiltinCommand<'_>) -> impl Future<Output = std::process::ExitCode>;
    fn run(env: Environment) -> impl Future<Output = std::process::ExitCode>;
    fn run_default() -> impl Future<Output = std::process::ExitCode>;
}

impl<S> CliAsync for S where S: CliEnums + CommandProvider, S::Enum: CommandExecutorAsync {
    async fn run_builtin(env: Environment, builtin: BuiltinCommand<'_>) -> std::process::ExitCode {
        handle_builtin::<S>(builtin, &env)
    }

    async fn run(env: Environment) -> std::process::ExitCode {
        let builder = S::build_cli()
            .unwrap();

        let parse_result
            = S::parse_args(&builder, &env);

        match parse_result {
            Ok(SelectionResult::Builtin(builtin)) => {
                Self::run_builtin(env, builtin).await
            },

            Ok(SelectionResult::Command(command_spec, _, partial_command)) => {
                let full_command = match <S as CliEnums>::Enum::try_from(partial_command) {
                    Ok(full_command)
                        => full_command,

                    Err(err)
                        => return report_error::<S>(&env, clipanion_core::Error::CommandError(command_spec, err)),
                };

                let command_result
                    = full_command.execute(&env).await;

                if let Some(error_message) = &command_result.error_message {
                    return report_error::<S>(&env, clipanion_core::Error::CommandError(command_spec, error_message.to_string().into()));
                }

                command_result.exit_code
            },

            Err(err) => {
                report_error::<S>(&env, err)
            },
        }
    }

    async fn run_default() -> std::process::ExitCode {
        Self::run(Default::default()).await
    }
}
