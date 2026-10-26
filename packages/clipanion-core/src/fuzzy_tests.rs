use std::{iter::once, ops::Range};

use itertools::Itertools;
use rand::Rng;
use rand_seeder::SipHasher;

use crate::{builder::{CliBuilder, CommandSpec, Component, OptionSpec, PositionalSpec}, SelectionResult};

fn gen_string<R: Rng>(rng: &mut R, len: Range<usize>) -> String {
    let mut s = String::new();
    for _ in 0..rng.random_range(len) {
        s.push(rng.random_range(b'a'..=b'z') as char);
    }
    s
}

fn gen_optional<R: Rng, T>(rng: &mut R, f: impl Fn(&mut R) -> T) -> Option<T> {
    if rng.random_bool(0.5) {
        Some(f(rng))
    } else {
        None
    }
}

fn gen_random_keyword<R: Rng>(rng: &mut R) -> String {
    format!("keyword-{}", gen_string(rng, 1..10))
}

fn gen_random_option_name<R: Rng>(rng: &mut R) -> String {
    format!("--option-{}", gen_string(rng, 1..10))
}

fn gen_random_value<R: Rng>(rng: &mut R) -> String {
    format!("value-{}", gen_string(rng, 1..10))
}

fn gen_random_values<R: Rng>(rng: &mut R, min_len: usize, extra_len: Option<usize>) -> Vec<String> {
    let mut values = vec![];

    let extra_len
        = rng.random_range(0..=extra_len.unwrap_or(4));

    for _ in 0..min_len+extra_len {
        values.push(gen_random_value(rng));
    }

    values
}

fn gen_random_positional_spec<R: Rng>(rng: &mut R, use_optional_positionals: bool) -> PositionalSpec {
    match rng.random_range(0..=1) {
        0 => PositionalSpec::Keyword {
            expected: gen_random_keyword(rng).into(),
        },

        1 => PositionalSpec::Dynamic {
            name: "positional".into(),
            documentation: None,
            min_len: rng.random_range(0..3),
            extra_len: match use_optional_positionals {
                true => gen_optional(rng, |rng| rng.random_range(0..3)),
                false => Some(0),
            },
            is_prefix: false,
            is_proxy: false,
            proxy_program: false,
            completer: None,
        },

        _ => unreachable!(),
    }
}

fn gen_random_option_spec<R: Rng>(rng: &mut R, use_optional_positionals: bool) -> OptionSpec {
    OptionSpec {
        primary_name: gen_random_option_name(rng).into(),
        aliases: vec![].into(),
        documentation: None,
        default_value: None,
        min_len: rng.random_range(0..3),
        allow_binding: rng.random_bool(0.5),
        is_hidden: false,
        is_required: rng.random_bool(0.5),

        // We can't use optional values for options when the command has optional positionals,
        // as it becomes ambiguous. For example, let's say we have this:
        //
        //   my-cmd [foo] [...foo] [--opt <val> [...val]]
        //
        // How should we parse `my-cmd hello --opt world`? What about `my-cmd --opt hello world`? And
        // it's the same thing with allow_boolean.

        extra_len: match use_optional_positionals {
            true => Some(0),
            false => gen_optional(rng, |rng| rng.random_range(0..3)),
        },

        allow_boolean: match use_optional_positionals {
            true => false,
            false => rng.random_bool(0.5),
        },
    }
}

fn gen_random_command_spec<R: Rng>(rng: &mut R, use_optional_positionals: bool) -> CommandSpec {
    let mut components = vec![];

    for _ in 0..rng.random_range(0..10) {
        components.push(match rng.random_range(0..=1) {
            0 => Component::Positional(gen_random_positional_spec(rng, use_optional_positionals)),
            1 => Component::Option(gen_random_option_spec(rng, use_optional_positionals)),
            _ => unreachable!(),
        });
    }

    CommandSpec {
        components: components.into(),
        ..Default::default()
    }
}

fn gen_random_command_values<R: Rng>(rng: &mut R, command_spec: &CommandSpec) -> Vec<(usize, Vec<String>)> {
    let mut values = vec![];
    let mut allow_extra_values = true;

    for (i, component) in command_spec.components.iter().enumerate() {
        match component {
            Component::Positional(positional_spec) => {
                match positional_spec {
                    PositionalSpec::Keyword {..} => {
                        allow_extra_values = true;
                    },

                    PositionalSpec::Dynamic {min_len, extra_len, ..} => {
                        let extra_len = if allow_extra_values {
                            *extra_len
                        } else {
                            Some(0)
                        };

                        let positional_values
                            = gen_random_values(rng, *min_len, extra_len);

                        if positional_values.len() > 0 {
                            values.push((i, positional_values));
                        }

                        allow_extra_values = false;
                    },
                }
            },

            Component::Option(option_spec) => {
                for _ in 0..rng.random_range(0..=3) {
                    let option_args = if option_spec.allow_boolean && rng.random_bool(0.5) {
                        vec![]
                    } else {
                        gen_random_values(rng, option_spec.min_len, option_spec.extra_len)
                    };

                    values.push((i, option_args));
                }
            }
        }
    }

    values
}

fn insert_b_into_a_randomly<R: Rng, T: Clone>(rng: &mut R, mut a: Vec<T>, b: Vec<T>) -> Vec<T> {
    let mut insert_positions = vec![];

    for _ in 0..b.len() {
        insert_positions.push(rng.random_range(0..=a.len()));
    }

    let mut insert_positions = insert_positions;
    insert_positions.sort(); // Ensure order to preserve B's sequence

    a.reserve(a.len() + b.len());

    for (i, b) in b.into_iter().enumerate() {
        a.insert(insert_positions[i] + i, b);
    }

    a
}

fn gen_random_command_line<R: Rng>(rng: &mut R, command_spec: &CommandSpec, command_values: &[(usize, Vec<String>)]) -> Vec<String> {
    let mut indexed_command_values
        = command_values.iter()
            .into_group_map_by(|(i, _)| *i);

    let mut positionals = vec![];

    // Options that can be set anywhere in the command line without ambiguities.
    let mut interlaced_options = vec![];

    // Options that MUST be at the end of the command line because they could tolerate more values than we provide.
    let mut trailing_options = vec![];

    for (i, component) in command_spec.components.iter().enumerate() {
        match component {
            Component::Positional(positional_spec) => {
                match positional_spec {
                    PositionalSpec::Keyword {expected, ..} => {
                        positionals.push(expected.to_string());
                    },

                    PositionalSpec::Dynamic {..} => {
                        for (_, values) in indexed_command_values.entry(i).or_default() {
                            positionals.extend(values.iter().cloned());
                        }
                    },
                }
            },

            Component::Option(option_spec) => {
                let mut force_trailing
                    = false;

                let all_names
                    = once(&option_spec.primary_name)
                        .chain(option_spec.aliases.iter())
                        .collect::<Vec<_>>();

                for (_, values) in indexed_command_values.entry(i).or_default() {
                    let name
                        = all_names[rng.random_range(0..all_names.len())];

                    let mut args
                        = vec![name.to_string()];

                    if values.len() == 0 && option_spec.allow_boolean && (option_spec.min_len > 0 || option_spec.extra_len != Some(0)) {
                        force_trailing = true;
                    } else {
                        let current_extra_len
                            = values.len() - option_spec.min_len;

                        args.extend(values.iter().cloned());

                        if option_spec.extra_len != Some(0) || current_extra_len > 0 {
                            force_trailing = true;
                        }
                    }

                    if force_trailing {
                        trailing_options.push(args);
                    } else {
                        interlaced_options.push(args);
                    }
                }
            },
        }
    }

    let mut command_line_segments
        = positionals.into_iter()
            .map(|positional| vec![positional])
            .collect::<Vec<_>>();

    command_line_segments
        = insert_b_into_a_randomly(rng, command_line_segments, interlaced_options);

    command_line_segments
        .extend(trailing_options);

    let command_line = command_line_segments.into_iter()
        .flatten()
        .collect();

    command_line
}

#[test]
fn test_gen_random_command_line() {
    let mut rng = SipHasher::from("testx")
        .into_rng();

    let selected_test = std::env::var("SELECTED_TEST")
        .ok()
        .map(|s| s.split("/").map(|s| s.parse::<usize>().unwrap()).collect::<Vec<_>>());

    for n1 in 0..100 {
        let use_optional_positionals
            = rng.random_bool(0.5);

        let command_spec
            = gen_random_command_spec(&mut rng, use_optional_positionals);

        for n2 in 0..100 {
            let command_values
                = gen_random_command_values(&mut rng, &command_spec);

            for n3 in 0..100 {
                let command_line
                    = gen_random_command_line(&mut rng, &command_spec, &command_values);

                if selected_test.as_ref().map(|state| &vec![n1, n2, n3] != state).unwrap_or(false) {
                    continue;
                }

                let result = std::panic::catch_unwind(|| {
                    let mut cli_builder
                        = CliBuilder::new();

                    cli_builder
                        .add_command(&command_spec);

                    let command_line_args
                        = command_line.iter().map(|s| s.as_str()).collect::<Vec<_>>();

                    let result
                        = cli_builder
                            .run(&command_line_args);

                    let Ok(mut selector) = result else {
                        panic!("Expected a selector result; got this instead: {:#?}", result);
                    };

                    let selection_result
                        = selector.resolve_state(|_| Ok(())).unwrap();

                    let SelectionResult::Command(_, state, _) = selection_result else {
                        panic!("Expected a command result");
                    };

                    let command_values_str
                        = command_values.iter()
                            .map(|(i, values)| (*i, values.iter().map(|s| s.as_str()).collect::<Vec<_>>()))
                            .collect::<Vec<_>>();

                    println!();
                    println!("==========");
                    println!();
                    println!("{}", command_spec);
                    println!();
                    println!("{:?}", command_values);
                    println!();
                    println!("{:?}", command_line);
                    println!();
                    println!("{:?}", state.values());

                    assert_eq!(state.values(), command_values_str);
                });

                if let Err(err) = result {
                    println!("{} / {} / {}", n1, n2, n3);
                    std::panic::resume_unwind(err);
                }
            }
        }
    }
}
