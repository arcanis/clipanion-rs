use clipanion::{core::{Component, PositionalSpec}, prelude::*};

/// Install the project's dependencies, then
/// build its artifacts
///
/// This command reads the manifest of the
///   project and installs what it lists.
///
/// Its modes:
/// - `fast`, which skips the
///   build step;
/// - `strict`, which fails on warnings.
#[cli::command]
#[cli::path("install")]
struct InstallCommand {
    /// Skip the build step of the packages that
    /// have one, as `--ignore-scripts` does
    #[cli::option("--skip-build", default = false)]
    skip_build: bool,

    /// Install the dependencies in this mode
    ///
    /// The default mode installs everything.
    #[cli::option("--mode")]
    mode: Option<String>,

    /// The packages
    /// to install
    packages: Vec<String>,
}

impl InstallCommand {
    fn execute(&self) {
    }
}

#[cli::program]
enum MyCli {
    Install(InstallCommand),
}

fn option_documentation(name: &str) -> &'static clipanion::core::Documentation {
    InstallCommand::command_spec().unwrap().components.iter()
        .find_map(|component| match component {
            Component::Option(option) if option.primary_name() == name => option.documentation.as_ref(),
            _ => None,
        })
        .unwrap()
}

#[test]
fn it_reads_the_first_paragraph_as_the_description() {
    let documentation
        = InstallCommand::command_spec().unwrap().documentation.as_ref().unwrap();

    assert_eq!(documentation.description(), "Install the project's dependencies, then build its artifacts");
}

#[test]
fn it_reads_the_following_paragraphs_and_list_items_as_the_details() {
    let documentation
        = InstallCommand::command_spec().unwrap().documentation.as_ref().unwrap();

    assert_eq!(documentation.details(), Some(concat!(
        "This command reads the manifest of the project and installs what it lists.\n\n",
        "Its modes:\n\n",
        "- `fast`, which skips the build step;\n\n",
        "- `strict`, which fails on warnings.",
    )));
}

#[test]
fn it_reads_a_multiline_option_description_in_full() {
    let documentation
        = option_documentation("--skip-build");

    assert_eq!(documentation.description(), "Skip the build step of the packages that have one, as `--ignore-scripts` does");
    assert_eq!(documentation.details(), None);
}

#[test]
fn it_reads_the_details_of_an_option() {
    let documentation
        = option_documentation("--mode");

    assert_eq!(documentation.description(), "Install the dependencies in this mode");
    assert_eq!(documentation.details(), Some("The default mode installs everything."));
}

#[test]
fn it_reads_a_multiline_positional_description_in_full() {
    let documentation = InstallCommand::command_spec().unwrap().components.iter()
        .find_map(|component| match component {
            Component::Positional(PositionalSpec::Dynamic {documentation, ..}) => documentation.as_ref(),
            _ => None,
        })
        .unwrap();

    assert_eq!(documentation.description(), "The packages to install");
}
