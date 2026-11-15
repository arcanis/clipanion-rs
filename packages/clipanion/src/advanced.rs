use std::{collections::HashMap, future::Future};

use clipanion_core::{BuiltinCommand, CliBuilder, CompletionContext, Info, SelectionResult, Shell, compute_completions, generate_completion_script, is_completion_enabled};

use crate::{details::{CliEnums, CommandExecutor, CommandExecutorAsync, CommandProvider}, format::{format_command_help, write_color, write_fading_title_line, Formatter}};

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

        clipanion_core::Error::BuildError(build_error) => {
            println!("{}", Formatter::<S>::format_error(&env.info, "Error", &build_error.to_string(), vec![]));
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

fn handle_builtin<'cmds, 'args, S: CliEnums + CommandProvider>(builder: &CliBuilder<'static>, env: &'args Environment, builtin: BuiltinCommand<'cmds, 'args>) -> Result<std::process::ExitCode, clipanion_core::Error<'cmds>> {
    match builtin {
        BuiltinCommand::Describe => {
            #[cfg(not(feature = "serde"))] {
                println!("Using this command requires the 'serde' feature to be enabled.");

                return Err(clipanion_core::Error::InternalError);
            }

            #[cfg(feature = "serde")] {
                let commands
                    = S::registered_commands()?;

                let commands_json
                    = serde_json::to_string(&commands)
                        .map_err(|_| clipanion_core::Error::InternalError)?;

                println!("{}", commands_json);

                Ok(std::process::ExitCode::SUCCESS)
            }
        },

        BuiltinCommand::Tokenize(command_line) => {
            #[cfg(not(feature = "tokens"))] {
                println!("Using this command requires the 'tokens' feature to be enabled.");

                return Err(clipanion_core::Error::InternalError);
            }

            #[cfg(feature = "tokens")] {
                use clipanion_core::TokenSet;

                let mut states
                    = builder.run_partial(&command_line);

                let result = states.pop().map(|state| TokenSet {
                    command_id: state.context_id,
                    tokens: state.tokens.clone(),
                });

                let tokens_json
                    = serde_json::to_string(&result)
                        .map_err(|_| clipanion_core::Error::InternalError)?;

                println!("{}", tokens_json);

                Ok(std::process::ExitCode::SUCCESS)
            }
        },

        BuiltinCommand::Version => {
            println!("{}", env.info.version);
            Ok(std::process::ExitCode::SUCCESS)
        },

        BuiltinCommand::Help(commands) => {
            // The help of a single command shows everything about it.
            if let [command] = commands.as_slice() {
                print!("{}", format_command_help(&env.info, command));
                return Ok(std::process::ExitCode::SUCCESS);
            }

            let mut output_string
                = String::new();

            write_fading_title_line(&mut output_string, &format!("{} - {}", env.info.program_name, env.info.version), (255, 255, 255), 80, 50);

            let commands = match commands.is_empty() {
                true => S::registered_commands().unwrap(),
                false => commands,
            };

            let default_command
                = commands.iter()
                    .find(|command| command.is_default())
                    .cloned();

            if let Some(default_command) = default_command {
                output_string.push_str("\n  \x1b[1m");
                output_string.push_str(&default_command.usage().oneliner(&env.info));
                output_string.push_str("\x1b[0m\n");
            }

            let mut commands_by_category
                = HashMap::<_, Vec<_>>::new();

            for command in &commands {
                if command.documentation.is_some() {
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

                output_string.push('\n');
                write_fading_title_line(&mut output_string, category, (128, 128, 128), 80, 50);

                let mut commands_and_paths
                    = commands.into_iter()
                        .map(|command| (&command.primary_path, command))
                        .collect::<Vec<_>>();

                commands_and_paths.sort_by(|a, b| {
                    a.0.cmp(&b.0)
                });

                for command in commands {
                    if let Some(documentation) = &command.documentation {
                        output_string.push_str("\n  \x1b[1m");
                        output_string.push_str(&command.usage().oneliner(&env.info));
                        output_string.push_str("\x1b[0m\n      ");
                        write_color(&mut output_string, (128, 128, 128));
                        output_string.push_str(&documentation.description);
                        output_string.push_str("\x1b[0m\n");
                    }
                }
            }

            if !is_completion_enabled(&env.info.binary_name) {
                output_string.push('\n');
                write_color(&mut output_string, (128, 128, 128));
                output_string.push_str(&format!(
                    "Tip: Shell completions are available. Run `{} --clipanion-completion-script` to generate the setup script.\x1b[0m\n",
                    env.info.binary_name,
                ));
            }

            print!("{}", output_string);

            Ok(std::process::ExitCode::SUCCESS)
        },

        BuiltinCommand::CompletionScript { shell, command } => {
            // Determine the shell to use
            let shell
                = shell.as_ref()
                    .and_then(|s| Shell::from_str(s))
                    .or_else(Shell::detect);

            let Some(shell) = shell else {
                eprintln!("Could not detect shell. Please specify with --shell <bash|zsh|fish>");
                return Err(clipanion_core::Error::InternalError);
            };

            // Use the provided command or default to the binary name
            let command
                = command.as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or(&env.info.binary_name);

            let script
                = generate_completion_script(shell, command);

            println!("{}", script);

            Ok(std::process::ExitCode::SUCCESS)
        },

        BuiltinCommand::Complete { index, args } => {
            let commands
                = S::registered_commands()?;

            let machine
                = builder.compile();

            // Convert index + args into before/current
            let (before, current) = if args.is_empty() {
                (vec![], "")
            } else if index >= args.len() {
                // Cursor is after all args (completing a new argument)
                (args.iter().map(|s| *s).collect(), "")
            } else {
                (args[..index].to_vec(), args[index])
            };

            let context
                = CompletionContext::new(before, current);

            let result
                = compute_completions(&commands, &machine, &context);

            // Output completions, one per line (tab-separated with description if available)
            for completion in result.completions {
                if let Some(desc) = &completion.description {
                    println!("{}\t{}", completion.text, desc);
                } else {
                    println!("{}", completion.text);
                }
            }

            Ok(std::process::ExitCode::SUCCESS)
        },
    }
}

pub trait Cli {
    fn run(env: Environment) -> std::process::ExitCode;
    fn run_default() -> std::process::ExitCode;
}

impl<S> Cli for S where S: CliEnums + CommandProvider, S::Enum: CommandExecutor {
    fn run(env: Environment) -> std::process::ExitCode {
        let builder = S::build_cli()
            .unwrap();

        let parse_result
            = S::parse_args(&builder, &env);

        match parse_result {
            Ok(SelectionResult::Builtin(builtin)) => {
                handle_builtin::<S>(&builder, &env, builtin)
                    .unwrap_or_else(|err| report_error::<S>(&env, err))
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
    fn run(env: Environment) -> impl Future<Output = std::process::ExitCode>;
    fn run_default() -> impl Future<Output = std::process::ExitCode>;
}

impl<S> CliAsync for S where S: CliEnums + CommandProvider, S::Enum: CommandExecutorAsync {
    async fn run(env: Environment) -> std::process::ExitCode {
        let builder = S::build_cli()
            .unwrap();

        let parse_result
            = S::parse_args(&builder, &env);

        match parse_result {
            Ok(SelectionResult::Builtin(builtin)) => {
                handle_builtin::<S>(&builder, &env, builtin)
                    .unwrap_or_else(|err| report_error::<S>(&env, err))
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
