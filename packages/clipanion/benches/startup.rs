//! How long a program takes to get from its arguments to a command: building the state machine,
//! running it, and hydrating the command. The program is gx's (Gitaxian's command-line client):
//! 31 commands, nested paths, five options shared by every command.
//!
//! Run with `cargo bench -p clipanion --bench startup`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use clipanion::advanced::Environment;
use clipanion::details::{CliEnums, CommandProvider};
use clipanion::prelude::*;
use clipanion::core::SelectionResult;

macro_rules! command {
    ($(#[$attribute:meta])* pub struct $name:ident { $($fields:tt)* }) => {
        #[cli::command]
        $(#[$attribute])*
        #[derive(Debug)]
        pub struct $name {
            $($fields)*
            /// The tier to talk to
            #[cli::option("--url")]
            pub global_url: Option<String>,
            /// Bearer token
            #[cli::option("--token")]
            pub global_token: Option<String>,
            /// The repository
            #[cli::option("--repo")]
            pub global_repo: Option<String>,
            /// Run as if started in DIR
            #[cli::option("-C")]
            pub global_directory: Option<String>,
            /// Print the result as JSON
            #[cli::option("--json", default = false)]
            pub global_json: bool,
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    Rebase,
    Squash,
    Merge,
}

impl std::str::FromStr for Strategy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "rebase" => Ok(Self::Rebase),
            "squash" => Ok(Self::Squash),
            "merge" => Ok(Self::Merge),
            _ => Err(format!("'{value}' isn't a strategy")),
        }
    }
}

command! {
    /// Create a working copy
    #[cli::path("clone")]
    pub struct CloneArgs {
        pub repo: String,
        pub dir: Option<String>,
        #[cli::option("--rev")]
        pub rev: Option<String>,
        #[cli::option("--sparse", default = Vec::new())]
        pub sparse: Vec<String>,
    }
}

command! {
    /// Show local changes
    #[cli::path("status")]
    pub struct StatusArgs {}
}

command! {
    /// Show local changes as a diff
    #[cli::path("diff")]
    pub struct DiffArgs {
        pub paths: Vec<String>,
    }
}

command! {
    /// Create a changeset from local changes
    #[cli::path("commit")]
    pub struct CommitArgs {
        pub paths: Vec<String>,
        #[cli::option("-m,--message")]
        pub message: String,
        #[cli::option("--author")]
        pub author: Option<String>,
        #[cli::option("--land", default = false)]
        pub land: bool,
        #[cli::option("--strategy")]
        pub strategy: Option<Strategy>,
        #[cli::option("--ref")]
        pub ref_name: Option<String>,
    }
}

command! {
    /// Move the working copy to another revision
    #[cli::path("update")]
    pub struct UpdateArgs {
        pub rev: Option<String>,
        #[cli::option("-f,--force", default = false)]
        pub force: bool,
    }
}

command! {
    /// Rebase the working copy's changesets server-side
    #[cli::path("rebase")]
    pub struct RebaseArgs {
        #[cli::option("--onto")]
        pub onto: Option<String>,
        #[cli::option("--base")]
        pub base: Option<String>,
    }
}

command! {
    /// Land a changeset
    #[cli::path("land")]
    pub struct LandArgs {
        pub rev: Option<String>,
        #[cli::option("--rev")]
        pub at: Option<String>,
        #[cli::option("--strategy")]
        pub strategy: Option<Strategy>,
        #[cli::option("--ref")]
        pub ref_name: Option<String>,
        #[cli::option("-m,--message")]
        pub message: Option<String>,
        #[cli::option("--author")]
        pub author: Option<String>,
    }
}

command! {
    /// List the directories materialized in the working copy
    #[cli::path("sparse")]
    #[cli::path("sparse", "list")]
    pub struct SparseListArgs {}
}

command! {
    /// Materialize these directories too
    #[cli::path("sparse", "add")]
    pub struct SparseAddArgs {
        pub dirs: Vec<String>,
    }
}

command! {
    /// Stop materializing these directories
    #[cli::path("sparse", "remove")]
    pub struct SparseRemoveArgs {
        pub dirs: Vec<String>,
    }
}

command! {
    /// Find the first changeset where a command fails
    #[cli::path("bisect")]
    pub struct BisectArgs {
        #[cli::option("--good")]
        pub good: String,
        #[cli::option("--bad")]
        pub bad: Option<String>,
        pub command: Vec<String>,
    }
}

command! {
    /// List repositories
    #[cli::path("repos")]
    pub struct ReposArgs {}
}

command! {
    /// Create a repository
    #[cli::path("create-repo")]
    pub struct CreateRepoArgs {
        pub name: String,
        #[cli::option("--description")]
        pub description: Option<String>,
    }
}

command! {
    /// List refs
    #[cli::path("refs")]
    pub struct RefsArgs {}
}

command! {
    /// Show history
    #[cli::path("log")]
    pub struct LogArgs {
        pub rev: Option<String>,
        #[cli::option("--rev")]
        pub at: Option<String>,
        #[cli::option("--path")]
        pub path: Option<String>,
        #[cli::option("-n,--limit", default = 20)]
        pub limit: u32,
        #[cli::option("--oneline", default = false)]
        pub oneline: bool,
    }
}

command! {
    /// Show a changeset and its diff
    #[cli::path("show")]
    pub struct ShowArgs {
        pub rev: Option<String>,
        #[cli::option("--rev")]
        pub at: Option<String>,
    }
}

command! {
    /// List a directory
    #[cli::path("ls")]
    pub struct LsArgs {
        pub path: Option<String>,
        #[cli::option("--rev")]
        pub rev: Option<String>,
    }
}

command! {
    /// Print a file
    #[cli::path("cat")]
    pub struct CatArgs {
        pub path: String,
        #[cli::option("--rev")]
        pub rev: Option<String>,
    }
}

command! {
    /// Find files by path
    #[cli::path("find")]
    pub struct FindArgs {
        pub query: Option<String>,
        #[cli::option("--rev")]
        pub rev: Option<String>,
        #[cli::option("-n,--limit", default = 100)]
        pub limit: u32,
    }
}

command! {
    /// Show who last changed each line
    #[cli::path("blame")]
    pub struct BlameArgs {
        pub path: String,
        #[cli::option("--rev")]
        pub rev: Option<String>,
    }
}

command! {
    /// The change view
    #[cli::path("compare")]
    pub struct CompareArgs {
        pub base: String,
        pub head: String,
        #[cli::option("--name-only", default = false)]
        pub name_only: bool,
    }
}

command! {
    /// Revert a changeset on the default ref
    #[cli::path("revert")]
    pub struct RevertArgs {
        pub rev: String,
        #[cli::option("--onto")]
        pub onto: Option<String>,
        #[cli::option("--ref")]
        pub ref_name: Option<String>,
        #[cli::option("-m,--message")]
        pub message: Option<String>,
        #[cli::option("--author")]
        pub author: Option<String>,
    }
}

command! {
    /// Import git history
    #[cli::path("import-git")]
    pub struct ImportGitArgs {
        pub git_dir: String,
        #[cli::option("--create", default = false)]
        pub create: bool,
        #[cli::option("--rev")]
        pub rev: Option<String>,
        #[cli::option("--ref")]
        pub ref_name: Option<String>,
    }
}

command! {
    /// Show a changeset's metadata
    #[cli::path("meta")]
    #[cli::path("meta", "list")]
    pub struct MetaListArgs {
        #[cli::option("--rev")]
        pub rev: Option<String>,
    }
}

command! {
    /// A metadata key's value
    #[cli::path("meta", "get")]
    pub struct MetaGetArgs {
        pub key: String,
        #[cli::option("--rev")]
        pub rev: Option<String>,
    }
}

command! {
    /// Write a metadata key's value
    #[cli::path("meta", "set")]
    pub struct MetaSetArgs {
        pub key: String,
        pub value: String,
        #[cli::option("--merge", default = false)]
        pub merge: bool,
        #[cli::option("--expect")]
        pub expect: Option<i64>,
        #[cli::option("--rev")]
        pub rev: Option<String>,
    }
}

command! {
    /// Delete a metadata key
    #[cli::path("meta", "delete")]
    pub struct MetaDeleteArgs {
        pub key: String,
        #[cli::option("--expect")]
        pub expect: Option<i64>,
        #[cli::option("--rev")]
        pub rev: Option<String>,
    }
}

command! {
    /// Show the event log
    #[cli::path("events")]
    pub struct EventsArgs {
        #[cli::option("--follow", default = false)]
        pub follow: bool,
        #[cli::option("--cursor")]
        pub cursor: Option<String>,
    }
}

command! {
    /// Show the tier you're talking to
    #[cli::path("hello")]
    pub struct HelloArgs {}
}

command! {
    /// Show the tier's cache statistics
    #[cli::path("stats")]
    pub struct StatsArgs {}
}

command! {
    /// Print this message or the help of the given command
    #[cli::path("help")]
    pub struct HelpArgs {
        pub command: Vec<String>,
    }
}

#[clipanion::derive::cli_enum]
#[clipanion::derive::cli_provider]
#[derive(Debug)]
pub enum Command {
    Clone(CloneArgs),
    Status(StatusArgs),
    Diff(DiffArgs),
    Commit(CommitArgs),
    Update(UpdateArgs),
    Rebase(RebaseArgs),
    Land(LandArgs),
    SparseList(SparseListArgs),
    SparseAdd(SparseAddArgs),
    SparseRemove(SparseRemoveArgs),
    Bisect(BisectArgs),
    Repos(ReposArgs),
    CreateRepo(CreateRepoArgs),
    Refs(RefsArgs),
    Log(LogArgs),
    Show(ShowArgs),
    Ls(LsArgs),
    Cat(CatArgs),
    Find(FindArgs),
    Blame(BlameArgs),
    Compare(CompareArgs),
    Revert(RevertArgs),
    ImportGit(ImportGitArgs),
    MetaList(MetaListArgs),
    MetaGet(MetaGetArgs),
    MetaSet(MetaSetArgs),
    MetaDelete(MetaDeleteArgs),
    Events(EventsArgs),
    Hello(HelloArgs),
    Stats(StatsArgs),
    Help(HelpArgs),
}

fn environment(argv: &[&str]) -> Environment {
    Environment::default()
        .with_argv(argv.iter().map(|arg| arg.to_string()).collect())
}

/// What `Cli::run` does before executing the command.
fn parse(env: &Environment) -> Command {
    let builder = Command::build_cli()
        .unwrap();

    match Command::parse_args(&builder, env) {
        Ok(SelectionResult::Command(_, _, partial)) => {
            <Command as CliEnums>::Enum::try_from(partial).unwrap()
        },

        other => panic!("{:?}", other.map(|_| ())),
    }
}

fn measure(name: &str, iterations: u32, mut f: impl FnMut()) {
    // Warm up.
    for _ in 0..iterations / 10 {
        f();
    }

    let mut samples = Vec::with_capacity(iterations as usize);

    for _ in 0..iterations {
        let start = Instant::now();
        f();
        samples.push(start.elapsed());
    }

    samples.sort();

    let median = samples[samples.len() / 2];
    let mean = samples.iter().sum::<Duration>() / iterations;

    println!("{name:<50} median {:>8.1} µs   mean {:>8.1} µs", median.as_secs_f64() * 1e6, mean.as_secs_f64() * 1e6);
}

/// Where `compile` spends its time.
fn phases() {
    let specs = Command::registered_commands().unwrap();
    let iterations = 2000u32;

    let (mut build, mut merge, mut simplify) = (Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let (mut nodes_before, mut nodes_after) = (0, 0);

    for _ in 0..iterations {
        let start = Instant::now();
        let machines = specs.iter().enumerate().map(|(id, spec)| spec.build(id)).collect::<Vec<_>>();
        build += start.elapsed();

        let start = Instant::now();
        let mut machine = clipanion::core::Machine::new_any_of(machines);
        merge += start.elapsed();
        nodes_before = machine.nodes.len();

        let start = Instant::now();
        machine.simplify_machine();
        simplify += start.elapsed();
        nodes_after = machine.nodes.len();

        black_box(machine);
    }

    println!("{:<50} {:>8.1} µs", "phase: build the commands' machines", (build / iterations).as_secs_f64() * 1e6);
    println!("{:<50} {:>8.1} µs ({nodes_before} nodes)", "phase: merge them", (merge / iterations).as_secs_f64() * 1e6);
    println!("{:<50} {:>8.1} µs ({nodes_after} nodes left)", "phase: simplify", (simplify / iterations).as_secs_f64() * 1e6);
}

fn main() {
    // `cargo bench -- <filter>` runs the cases whose names contain it.
    let filter = std::env::args()
        .skip(1)
        .find(|arg| !arg.starts_with('-'));

    let selected = |name: &str| filter.as_deref().is_none_or(|filter| name.contains(filter));

    let cases: &[(&str, &[&str])] = &[
        ("status", &["status"]),
        ("log -n 5 --oneline", &["log", "-n", "5", "--oneline"]),
        ("meta set k v --expect 2 --rev main", &["meta", "set", "k", "v", "--expect", "2", "--rev", "main"]),
        ("commit -m msg src --land --strategy squash", &["commit", "-m", "msg", "src", "--land", "--strategy", "squash"]),
        ("clone (10 args)", &["clone", "acme/app", "--sparse", "a", "dir", "--sparse=b", "--rev", "main", "-C", "there"]),
    ];

    // What a fresh process pays once: the command specs are built on first use.
    let start = Instant::now();
    let builder = Command::build_cli().unwrap();
    println!("{:<50} {:>8.1} µs", "build_cli, first call", start.elapsed().as_secs_f64() * 1e6);

    let start = Instant::now();
    let machine = builder.compile();
    println!("{:<50} {:>8.1} µs ({} nodes)", "compile, first call", start.elapsed().as_secs_f64() * 1e6, machine.nodes.len());
    drop(machine);

    if selected("compile") {
        measure("compile", 2000, || {
            black_box(builder.compile());
        });
    }

    if selected("phase") {
        phases();
    }

    for (name, argv) in cases {
        let name = format!("parse: {name}");

        if selected(&name) {
            let env = environment(argv);

            measure(&name, 2000, || {
                black_box(parse(&env));
            });
        }
    }
}
