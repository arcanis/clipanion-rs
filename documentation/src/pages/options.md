---
layout: ../layouts/Guide.astro
title: Options and positionals
---

# Options and positionals

A command is a struct. Its fields are positionals, unless they're marked with
`#[cli::option]`, and what a field accepts follows from its type:

```rust
use clipanion::prelude::*;

/// Record changes to the repository.
#[cli::command]
#[cli::path("commit")]
struct CommitCommand {
    /// Use the given message as the commit message.
    #[cli::option("-m,--message")]
    message: String,

    /// Automatically stage files that have been modified and deleted.
    #[cli::option("-a,--all", default = false)]
    all: bool,

    /// Only commit these paths.
    paths: Vec<String>,
}
```

## Options

| Field | Command line | When it's missing |
| --- | --- | --- |
| `message: String` | `-m msg`, `--message msg`, `--message=msg`, `-mmsg` | an error: it's required |
| `message: String` with `default = ...` | the same | the default |
| `message: Option<String>` | the same | `None` |
| `all: bool` with `default = false` | `-a`, `--all`; `--no-all` sets it back | the default (without one, a `bool` option is required) |
| `verbose: u8` with `default = 0, counter` | `-v`, `-vvv`: each use adds one | the default |
| `size: (u32, u32)` | `--size 10 20` | an error, unless it's an `Option` or has a default |
| `headers: Vec<String>` | `--header a --header b`, `--header a b` | an error, unless it has `default = Vec::new()` |
| `color: Option<Option<String>>` | `--color` (`Some(None)`) or `--color auto` | `None` |

An option is required unless its type is an `Option`, or it has a `default`:
when it's missing, parsing fails with `Missing required option argument(s):
--message`. Options that aren't required can be reset with their `--no-` form
(`--no-message`).

A list option takes one value at each use by default; `min_len = N` asks for
`N` of them. Each use can take more, when no positional can.

## Positionals

| Field | Command line |
| --- | --- |
| `repo: String` | a required positional |
| `dir: Option<String>` | an optional one |
| `paths: Vec<String>` | any number of them; `#[cli::positional(min_len = 1)]` requires one at least |

With `#[cli::command(proxy)]`, the command's list positional is a *proxy*: it
takes the words that follow it as they are, options included, to forward them to
another program (see [how the command line is read](/parsing)).

## Values

Option and positional values are read with `FromStr`: `String`, numbers,
`PathBuf`, or your own types. When a value doesn't parse, the error is the one
`FromStr` returns, and the command fails with it.

An enum takes values once it implements `FromStr` (and `Debug`):

```rust
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// How to land the change.
    #[cli::option("--strategy", default = Strategy::Rebase)]
    strategy: Strategy,
}
```

```bash
$ my-tool land --strategy yolo
Error: 'yolo' isn't a strategy: rebase, squash or merge

› my-tool land [--strategy <arg>]
```

The `serde` feature doesn't change how values are read: it serializes the
command specs, for the tools that describe a program
(`--clipanion-commands`).

Options can't require one another (an option only valid with another one): check
those after parsing, in the command.

## Reading the specs

Each command's spec is a `static`, built at compile time: `MyCommand::command_spec()`
returns a `&'static CommandSpec`, and a program's `registered_commands()` lists
them. Building them takes no allocation.

Their text is a `Str` (`Cow<'static, str>`), and their lists a `List<T>`
(`Cow<'static, [T]>`). Both read like the `&str` and slices they deref to:

```rust
use clipanion::core::{CommandSpec, Component};

fn describe(spec: &CommandSpec) {
    let path = spec.primary_path.join(" ");

    for option in spec.components.iter().filter_map(Component::is_option) {
        let name: &str = option.primary_name();
        let description = option.documentation.as_ref().map(|doc| doc.description());
        println!("{path} {name}: {}", description.unwrap_or(""));
    }

    // The primary path, then the aliases.
    for path in spec.paths() {
        println!("{}", path.join(" "));
    }
}
```

A `String` comes from `to_string()` (`clone()` gives a `Str`), and an owned
spec, built at runtime, takes `String`s and `Vec`s with `into()`:

```rust
use clipanion::core::{CommandSpec, Component, OptionSpec};

let spec = CommandSpec {
    primary_path: vec!["install".into()].into(),
    components: vec![Component::Option(OptionSpec::boolean("--frozen"))].into(),
    ..Default::default()
};
```
