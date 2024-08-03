use std::collections::HashSet;

use partially::Partial;

use crate::{runner::{OptionValue, PartialRunState, Positional, RunState, Token}, shared::Arg};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reducer {
    None,
    InhibateOptions,
    PushBatch,
    PushBound,
    PushExtra,
    PushFalse(String),
    PushNone(String),
    PushPath,
    PushPositional,
    PushRest,
    PushStringValue,
    PushTrue(String),
    SetCandidateState(PartialRunState),
    SetError(String),
    SetOptionArityError,
    SetSelectedIndex(isize),
    SetStringValue,
    UseHelp(usize),
}

impl Default for Reducer {
    fn default() -> Self {
        Reducer::None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    Always,
    IsBatchOption(HashSet<String>),
    IsBoundOption(HashSet<String>),
    IsExact(String),
    IsExactString(String),
    IsHelp,
    IsNotOptionLike,
    IsOptionLike,
    IsUnsupportedOption(HashSet<String>),
    IsInvalidOption,
}

pub fn apply_reducer(reducer: &Reducer, state: &RunState, arg: &Arg, segment_index: usize) -> RunState {
    match reducer {
        Reducer::InhibateOptions => {
            let mut state = state.clone();
            state.ignore_options = true;
            state
        }

        Reducer::PushBatch => {
            let arg = arg.unwrap_user();
            let mut state = state.clone();

            for t in 1..arg.len() {
                let name = format!("-{}", &arg[t..t + 1]);

                let slice = match t == 1 {
                    true => (0, 2),
                    false => (t, t + 1),
                }; 

                state.options.push((
                    name.clone(),
                    OptionValue::Bool(true),
                ));

                state.tokens.push(Token::Option {
                    segment_index,
                    slice: Some(slice),
                    option: name,
                });
            }

            state
        }

        Reducer::PushBound => {
            let arg = arg.unwrap_user();
            let mut state = state.clone();

            let (name, value) = arg.split_at(arg.find('=').unwrap());

            state.options.push((
                name.to_string(),
                OptionValue::String(value[1..].to_string()),
            ));

            state.tokens.push(Token::Option {
                segment_index,
                slice: Some((0, name.len())),
                option: name.to_string(),
            });

            state.tokens.push(Token::Assign {
                segment_index,
                slice: (name.len(), name.len() + 1),
            });

            state.tokens.push(Token::Value {
                segment_index,
                slice: Some((name.len() + 1, value.len())),
            });

            state
        }

        Reducer::PushExtra => {
            let arg = arg.unwrap_user();
            let mut state = state.clone();
            state.positionals.push(Positional::Optional(arg.to_string()));
            state
        }

        Reducer::PushFalse(name) => {
            let mut state = state.clone();

            state.options.push((
                name.to_string(),
                OptionValue::Bool(false),
            ));

            state.tokens.push(Token::Option {
                segment_index,
                slice: None,
                option: name.to_string(),
            });

            state
        }

        Reducer::PushNone(name) => {
            let mut state = state.clone();

            state.options.push((
                name.to_string(),
                OptionValue::None,
            ));

            state.tokens.push(Token::Option {
                segment_index,
                slice: None,
                option: name.to_string(),
            });

            state
        }

        Reducer::PushPath => {
            let arg = arg.unwrap_user();
            let mut state = state.clone();
            state.path.push(arg.to_string());
            state
        }

        Reducer::PushPositional => {
            let arg = arg.unwrap_user();
            let mut state = state.clone();
            state.positionals.push(Positional::Required(arg.to_string()));
            state
        }

        Reducer::PushRest => {
            let arg = arg.unwrap_user();
            let mut state = state.clone();
            state.positionals.push(Positional::Rest(arg.to_string()));
            state
        }

        Reducer::PushStringValue => {
            let arg = arg.unwrap_user();
            let mut state = state.clone();

            let last_option = state.options.last_mut().unwrap();

            match last_option.1 {
                OptionValue::None => {
                    last_option.1 = OptionValue::Array(vec![arg.to_string()]);
                }

                OptionValue::Array(ref mut values) => {
                    values.push(arg.to_string());
                }

                _ => {
                    panic!("Expected None or Array");
                }
            }

            state.tokens.push(Token::Value {
                segment_index,
                slice: None,
            });

            state
        }

        Reducer::PushTrue(name) => {
            let mut state = state.clone();

            state.options.push((
                name.to_string(),
                OptionValue::Bool(true),
            ));

            state.tokens.push(Token::Option {
                segment_index,
                slice: None,
                option: name.to_string(),
            });

            state
        }

        Reducer::SetError(message) => {
            let mut state = state.clone();

            state.error_message = match arg {
                Arg::EndOfInput | Arg::EndOfPartialInput => format!("{}.", message),
                _ => format!("{} (\"{}\").", message, arg.unwrap_user()),
            };

            state
        }

        Reducer::SetOptionArityError => {
            let last_option_name = &state.options.last().unwrap().0;

            let mut state = state.clone();
            state.error_message = format!("Not enough arguments to option {}.", last_option_name);
            state
        }

        Reducer::SetSelectedIndex(index) => {
            let mut state = state.clone();
            state.selected_index = Some(*index);
            state
        }

        Reducer::SetStringValue => {
            let arg = arg.unwrap_user();
            let mut state = state.clone();

            let last_option = state.options.last_mut().unwrap();
            last_option.1 = OptionValue::String(arg.to_string());

            state.tokens.push(Token::Value {
                segment_index,
                slice: None,
            });

            state
        }

        Reducer::UseHelp(index) => {
            let mut state = state.clone();
            state.options = vec![("-c".to_string(), OptionValue::String(format!("{}", *index)))];
            state
        }

        Reducer::SetCandidateState(partial) => {
            let mut state = state.clone();
            state.apply_some(partial.clone());
            state
        }

        Reducer::None => {
            state.clone()
        }
    }
}

fn is_valid_option(option: &str) -> bool {
    if option.starts_with("--") {
        option.chars().skip(2).all(|c| c.is_alphanumeric() || c == '-')
    } else if option.starts_with("-") {
        option.chars().skip(1).all(|c| c.is_alphabetic())
    } else {
        false
    }
}

pub fn apply_check(check: &Check, state: &RunState, arg: &Arg, _segment_index: usize) -> bool {
    match check {
        Check::Always => true,

        Check::IsBatchOption(options) => {
            let arg = arg.unwrap_user();
            !state.ignore_options && arg.starts_with('-') && arg.len() > 2 && arg.chars().skip(1).all(|c| c.is_ascii_alphanumeric() && options.contains(&format!("-{}", &c.to_string())))
        }

        Check::IsBoundOption(options) => {
            let arg = arg.unwrap_user();

            !state.ignore_options && arg.find('=').map_or(false, |i| {
                options.contains(arg.split_at(i).0)
            })
        }

        Check::IsExact(needle) => {
            let arg = arg.unwrap_user();
            !state.ignore_options && arg == needle
        }

        Check::IsHelp => {
            let arg = arg.unwrap_user();
            !state.ignore_options && (arg == "--help" || arg == "-h" || arg.starts_with("--help="))
        }

        Check::IsExactString(needle) => {
            let arg = arg.unwrap_user();
            !state.ignore_options && arg == needle.as_str()
        }

        Check::IsNotOptionLike => {
            let arg = arg.unwrap_user();
            state.ignore_options || arg == "-" || !arg.starts_with('-')
        }

        Check::IsOptionLike => {
            let arg = arg.unwrap_user();
            !state.ignore_options && arg != "-" && arg.starts_with('-')
        }

        Check::IsUnsupportedOption(options) => {
            let arg = arg.unwrap_user();
            !state.ignore_options && arg.starts_with("-") && is_valid_option(arg) && !options.contains(arg)
        }

        Check::IsInvalidOption => {
            let arg = arg.unwrap_user();
            !state.ignore_options && arg.starts_with("-") && !is_valid_option(arg)
        }
    }
}
