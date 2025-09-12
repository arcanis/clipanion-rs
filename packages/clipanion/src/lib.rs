pub extern crate clipanion_derive;

pub use clipanion_core as core;
pub use clipanion_derive as derive;

pub mod cli {
    pub use clipanion_derive::command;
}

pub mod advanced;
pub mod format;
pub mod details;
pub mod prelude;

pub use advanced::Environment;

pub use clipanion_core::{
    BuiltinCommand,
    CommandError,
    Error,
};

#[macro_export]
macro_rules! program_enum {
    ($name:ident, [$($command:ty),* $(,)?]) => {
        #[clipanion::derive::cli_enum($($command),*)]
        #[clipanion::derive::cli_exec_sync($($command),*)]
        enum $name {}
    };

    ($name:ident, [$($command:ty),* $(,)?], async) => {
        #[clipanion::derive::cli_enum($($command),*)]
        #[clipanion::derive::cli_exec_async($($command),*)]
        enum $name {}
    };
}

#[macro_export]
macro_rules! program_provider {
    ($name:ident, [$($command:ty),* $(,)?]) => {
        impl $crate::details::CommandProvider for $name {
            type Command = $name;

            fn command_usage(command_index: usize, opts: $crate::core::CommandUsageOptions) -> Result<$crate::core::CommandUsageResult, $crate::core::BuildError> {
                use $crate::details::CommandController;

                const FNS: &[fn($crate::core::CommandUsageOptions) -> Result<$crate::core::CommandUsageResult, $crate::core::BuildError>] = &[
                    $(<$command>::command_usage),*
                ];

                FNS[command_index](opts)
            }

            fn registered_commands() -> Result<Vec<&'static $crate::core::CommandSpec>, $crate::core::BuildError> {
                use $crate::details::CommandController;

                Ok(vec![
                    $(<$command>::command_spec()?),*
                ])
            }

            fn parse_args<'args>(builder: &$crate::core::CliBuilder<'static>, environment: &'args $crate::advanced::Environment) -> Result<$crate::core::SelectionResult<'static, 'args, <$name as $crate::details::CliEnums>::PartialEnum>, $crate::core::Error<'args>> where $name: $crate::details::CliEnums {
                let argv
                    = environment.argv.iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>();

                let mut selector
                    = builder.run(&argv)?;

                const FNS: &[fn(&$crate::advanced::Environment, &$crate::core::State<'_>) -> Result<<$name as ::clipanion::details::CliEnums>::PartialEnum, $crate::core::CommandError>] = &[
                    $(|environment, state| {
                        use $crate::details::CommandController;

                        let partial
                            = <$command>::hydrate_from_state(environment, state)?;

                        Ok(partial.into())
                    }),*
                ];

                selector.resolve_state(|state| {
                    let command
                        = FNS[state.context_id](environment, state)?;

                    Ok(command.into())
                })
            }

            fn build_cli() -> Result<$crate::core::CliBuilder<'static>, $crate::core::BuildError> {
                use $crate::details::CommandController;

                let mut builder
                    = $crate::core::CliBuilder::new();

                $(builder.add_command(<$command>::command_spec()?);)*

                if std::env::var("CLIPANION_DEBUG").is_ok() {
                    println!("========== CLI State Machine ==========");
                    println!("{:?}", builder.compile());
                }

                Ok(builder)
            }
        }
    };
}

#[macro_export]
macro_rules! program {
    ($name:ident, [$($command:ty),* $(,)?]) => {
        $crate::program_enum!($name, [$($command),*]);
        $crate::program_provider!($name, [$($command),*]);
    };
}

#[macro_export]
macro_rules! program_async {
    ($name:ident, [$($command:ty),* $(,)?]) => {
        $crate::program_enum!($name, [$($command),*], async);
        $crate::program_provider!($name, [$($command),*]);
    };
}

#[macro_export]
macro_rules! test_cli_success {
    ($test_name:ident, $cli_name:ident, $command_name:ty, $args:expr, $fn:expr) => {
        #[test]
        fn $test_name() {
            const ARGS: &[&str] = $args;

            let cli = $cli_name::build_cli().unwrap();
            let env = $crate::advanced::Environment::default().with_argv(ARGS.iter().map(|s| s.to_string()).collect());

            println!("cli: {:?}", cli.compile());

            let result = $cli_name::parse_args(&cli, &env);
            let f: fn($command_name) -> () = $fn;

            let result = result
                .unwrap_or_else(|err| panic!("expected command, got error: {:?}", err));

            let command = match result {
                $crate::core::SelectionResult::Builtin(builtin) => {
                    panic!("expected command, got builtin: {:?}", builtin);
                },

                $crate::core::SelectionResult::Command(_, _, command) => {
                    command
                },
            };

            let command
                = <$cli_name as $crate::details::CliEnums>::Enum::try_from(command)
                    .unwrap();

            f(command.into());
        }
    };
}

#[macro_export]
macro_rules! test_cli_failure {
    ($test_name:ident, $cli_name:ident, $args:expr, $fn:expr) => {
        #[test]
        fn $test_name() {
            const ARGS: &[&str] = $args;

            let cli = $cli_name::build_cli().unwrap();
            let env = $crate::advanced::Environment::default()
                .with_argv(ARGS.iter().map(|s| s.to_string()).collect());

            println!("cli: {:?}", cli.compile());

            let result = $cli_name::parse_args(&cli, &env);
            let f: fn($crate::core::Error<'_>) -> () = $fn;

            f(match result {
                Err(error) => error,
                Ok(_) => panic!("expected error"),
            });
        }
    };
}
