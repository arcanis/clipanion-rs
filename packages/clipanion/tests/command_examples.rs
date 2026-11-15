use clipanion::prelude::*;

/// Add dependencies to the project
///
/// This command adds the given packages to the manifest.
///
/// ## Examples
///
/// Add a regular package
///
/// `my-program add lodash`
///
/// Add a package to the development
/// dependencies
///
/// `my-program add -D typescript`
///
#[cli::command]
#[cli::path("add")]
struct AddCommand {
    #[cli::option("-D,--dev", default = false)]
    dev: bool,

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

#[test]
fn it_reads_the_examples_section() {
    let examples = AddCommand::command_spec().unwrap().examples.iter()
        .map(|example| (&*example.description, &*example.command))
        .collect::<Vec<_>>();

    assert_eq!(examples, vec![
        ("Add a regular package", "my-program add lodash"),
        ("Add a package to the development dependencies", "my-program add -D typescript"),
    ]);
}

#[test]
fn it_leaves_the_examples_out_of_the_details() {
    let documentation
        = AddCommand::command_spec().unwrap().documentation.as_ref().unwrap();

    assert_eq!(documentation.description(), "Add dependencies to the project");
    assert_eq!(documentation.details(), Some("This command adds the given packages to the manifest."));
}

#[test]
fn it_has_no_examples_without_a_section() {
    assert!(RemoveCommand::command_spec().unwrap().examples.is_empty());
}
