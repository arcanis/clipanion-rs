---
layout: ../layouts/Guide.astro
title: How the command line is read
---

# How the command line is read

Options can come anywhere after the program's name: before the command's path,
between its words, between positionals. Only an option's values, and a proxy
once it started, can't be interrupted.

## What's an option

A word is an option when it's one of the command's options:

- one of its names: `-m`, `--message`;
- a name and its value, for options that take a single one: `--message=msg`,
  `-m=msg`, or a short name with the value attached, `-mmsg`;
- short options together, like getopt's: `-av` is `-a -v`, and the last one can
  take a value, attached or in the next word: `-am msg`, `-amsg`;
- `--`, which ends the options: every word after it is a positional, even when it
  starts with a dash;
- `-h` and `--help`, which ask for the command's help, unless the command
  defines them.

## Words that start with a dash

Any other word can be a value, even when it starts with a dash:

- an option's value can be any word that isn't an option: `-m "- Fix the thing"`,
  `--offset -5`, `-m -x`. A value that looks like one of the command's options
  needs `=`: `-m --amend` is a missing value, `--message=--amend` isn't;
- positionals, and the values an option can go without (a list option's
  additional values, an `Option<Option<T>>`), can be negative numbers (`-1`,
  `-0.5`) and `-`: `meta set retries -1`, `cat -`;
- other words starting with a dash are errors (an unknown option), never
  positionals. Use `--` to pass them: `rm -- -file`.

A command can still define options named after digits (`-1`): those words are
then that option.

```bash
git commit -am "- Fix the thing"
git commit --message=--amend
```

## Proxies

A proxy (`#[cli::command(proxy)]`) forwards the rest of the command line to
another program. It starts at the first word that isn't one of the command's
options:

- the options before it are read as the command's, and `--` ends them without
  being forwarded: `bisect --good v1 --repo r -- make test` reads `--repo` and
  forwards `make test`;
- once it started, the proxy takes every word, options and `--` included:
  `bisect --good v1 make -j4 -- --verbose` forwards `make -j4 -- --verbose`.

Right after the command's path, `-h` and `--help` are forwarded too.

### Proxying a program

When what a proxy forwards starts with a program, whose name never starts with a
dash, declare it with `proxy_program` rather than `proxy`:

```rust
#[cli::command(proxy_program)]
#[cli::path("bisect")]
struct BisectCommand {
    #[cli::option("--good")]
    good: String,

    command: Vec<String>,
}
```

The proxy then starts only at a word that isn't option-like, or after `--`, and
every option-like word before it is one of the command's options:

- the options are read as usual: `bisect --good v1 --repo r make` forwards
  `make`;
- `-h` and `--help` give the command's help, right after its path too:
  `bisect --help`, `bisect --good v1 -h`;
- an unknown one is an error, not the program: `bisect --good v1 --frobnicate`;
- after `--`, the program can start with a dash: `bisect --good v1 -- -x` forwards
  `-x`;
- once the program started, the proxy takes every word, as before:
  `bisect --good v1 make --help` forwards `make --help`.

A word is option-like when a positional couldn't take it: when it starts with a
dash, except `-` alone and negative numbers (`-1`, `-0.5`) that aren't one of the
command's options. Those start the program.
