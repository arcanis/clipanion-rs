use clipanion::{core::{BuiltinCommand, Info, SelectionResult}, format::format_command_help, prelude::*};

/// Add dependencies to the project
///
/// This command adds the given packages to the manifest of the current
/// project, then installs them. Each package is added to the `dependencies`
/// unless told otherwise:
///
/// - `-D,--dev` adds them to the `devDependencies`, which are only installed
///   in development;
/// - `-P,--peer` adds them to the `peerDependencies`.
///
/// ## Examples
///
/// Add a regular package
///
/// `my-program add lodash`
///
/// Add a package to the development dependencies
///
/// `my-program add -D typescript`
#[cli::command]
#[cli::path("add")]
struct AddCommand {
    /// Add the packages to the `devDependencies`
    #[cli::option("-D,--dev", default = false)]
    dev: bool,

    /// Add the packages to the `peerDependencies`
    #[cli::option("-P,--peer", default = false)]
    peer: bool,

    /// Use the given range for the packages instead of the one the registry
    /// reports for their latest version, so that several packages can be
    /// added at the same version
    #[cli::option("--range")]
    range: Option<String>,

    packages: Vec<String>,
}

impl AddCommand {
    fn execute(&self) {
    }
}

/// Remove dependencies from the project
#[cli::command]
#[cli::path("remove")]
struct RemoveCommand {
    packages: Vec<String>,
}

impl RemoveCommand {
    fn execute(&self) {
    }
}

#[cli::program]
enum MyCli {
    Add(AddCommand),
    Remove(RemoveCommand),
}

fn info() -> Info {
    Info {
        program_name: "My Program".to_string(),
        binary_name: "my-program".to_string(),
        version: "1.0.0".to_string(),
        about: String::new(),
        colorized: false,
    }
}

#[test]
fn it_selects_the_help_of_a_single_command() {
    let builder = MyCli::build_cli().unwrap();
    let env = clipanion::Environment::default().with_argv(vec!["add".to_string(), "--help".to_string()]);

    match MyCli::parse_args(&builder, &env) {
        Ok(SelectionResult::Builtin(BuiltinCommand::Help(commands))) => {
            assert_eq!(commands, vec![AddCommand::command_spec().unwrap()]);
        },

        _ => panic!("expected the help of the add command"),
    }
}

#[test]
fn it_prints_the_description_usage_options_details_and_examples() {
    let help
        = format_command_help(&info(), AddCommand::command_spec().unwrap());

    assert_eq!(help, concat!(
        "Add dependencies to the project\n",
        "\n",
        "━━━ Usage ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n",
        "\n",
        "  › my-program add [--dev,-D] [--peer,-P] [--range <arg>] […PACKAGESN]\n",
        "\n",
        "━━━ Options ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n",
        "\n",
        "  --dev,-D         Add the packages to the `devDependencies`\n",
        "  --peer,-P        Add the packages to the `peerDependencies`\n",
        "  --range <arg>    Use the given range for the packages instead of the one the\n",
        "                   registry reports for their latest version, so that several\n",
        "                   packages can be added at the same version\n",
        "\n",
        "━━━ Details ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n",
        "\n",
        "This command adds the given packages to the manifest of the current project,\n",
        "then installs them. Each package is added to the `dependencies` unless told\n",
        "otherwise:\n",
        "\n",
        "- `-D,--dev` adds them to the `devDependencies`, which are only installed in\n",
        "  development;\n",
        "\n",
        "- `-P,--peer` adds them to the `peerDependencies`.\n",
        "\n",
        "━━━ Examples ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n",
        "\n",
        "Add a regular package\n",
        "  › my-program add lodash\n",
        "\n",
        "Add a package to the development dependencies\n",
        "  › my-program add -D typescript\n",
    ));
}

#[test]
fn it_skips_the_sections_a_command_has_nothing_for() {
    let help
        = format_command_help(&info(), RemoveCommand::command_spec().unwrap());

    assert_eq!(help, concat!(
        "Remove dependencies from the project\n",
        "\n",
        "━━━ Usage ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n",
        "\n",
        "  › my-program remove […PACKAGESN]\n",
    ));
}
