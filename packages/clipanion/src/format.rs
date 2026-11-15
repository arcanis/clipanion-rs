use std::{fmt::Display, marker::PhantomData};

use clipanion_core::{CommandSpec, Info};
use colored::Colorize;

use crate::details::CommandProvider;

use std::collections::HashMap;
use std::env;
use std::fmt::Write;

pub type Color = (u8, u8, u8);

pub fn write_color<T: Write>(output: &mut T, color: Color) {
    let r = color.0;
    let g = color.1;
    let b = color.2;

    write!(output, "\x1b[38;2;{r};{g};{b}m").unwrap();
}

pub fn write_fading_title_line<T: Write>(output: &mut T, title: &str, main_color: (u8, u8, u8), total_length: usize, fade_len: usize) {
    // Background target map (RGB)
    let target_colors: HashMap<&'static str, (u8, u8, u8)> = [
        ("ghostty", (39, 45, 52)),
        ("xterm",   (0, 0, 0)),
    ].iter().cloned().collect();

    let term_program
        = env::var("TERM_PROGRAM")
            .unwrap_or_default()
            .to_lowercase();

    let (r_target, g_target, b_target) = target_colors
        .get(term_program.as_str())
        .copied()
        .unwrap_or((0, 0, 0)); // Default: black

    // Decorative left part
    let left = "━━━ ";
    let title_str = format!("{}{} ", left, title);
    let visible_len = title_str.chars().count();

    // Calculate how many ━ are needed on the right
    let remaining = total_length.saturating_sub(visible_len);
    let fade_len = fade_len.min(remaining);
    let solid_len = remaining.saturating_sub(fade_len);

    output.write_str("\x1b[1m").unwrap();
    write_color(output, main_color);
    output.write_str(left).unwrap();
    write_color(output, (255, 255, 255));
    output.write_str(&title).unwrap();
    write_color(output, main_color);
    output.write_char(' ').unwrap();

    // Solid ━ characters before the fade starts
    for _ in 0..solid_len {
        output.write_char('━').unwrap();
    }

    // Fading right side (from white to target background color)
    for i in 0..fade_len {
        let t = i as f32 / fade_len as f32;
        let r = ((1.0 - t) * main_color.0 as f32 + t * r_target as f32) as u8;
        let g = ((1.0 - t) * main_color.1 as f32 + t * g_target as f32) as u8;
        let b = ((1.0 - t) * main_color.2 as f32 + t * b_target as f32) as u8;

        write_color(output, (r, g, b));
        output.write_char('━').unwrap();
    }

    output.write_str("\x1b[0m\n").unwrap(); // Reset
}

/// The width the help wraps its text to.
const HELP_WIDTH: usize = 80;

/// A description goes on the line after its option's definition when it
/// would have fewer columns than this next to it.
const MIN_DESCRIPTION_WIDTH: usize = 30;

fn write_section_header(output: &mut String, info: &Info, title: &str) {
    output.push('\n');

    if info.colorized {
        write_fading_title_line(output, title, (128, 128, 128), HELP_WIDTH, 50);
    } else {
        let header
            = format!("━━━ {} ", title);

        output.push_str(&header);

        for _ in header.chars().count()..HELP_WIDTH {
            output.push('━');
        }

        output.push('\n');
    }

    output.push('\n');
}

/// Writes the paragraphs of `text` (separated by empty lines), wrapped to
/// `width` columns, with an empty line between them if `spaced`. Every line
/// but the first one of the text starts with `indent`, and the lines that
/// follow the first one of a list item are indented two more columns.
fn write_paragraphs(output: &mut String, text: &str, indent: &str, width: usize, spaced: bool) {
    for (index, paragraph) in text.split("\n\n").enumerate() {
        if index > 0 {
            if spaced {
                output.push('\n');
            }

            output.push_str(indent);
        }

        let continuation_indent = match paragraph.starts_with("- ") || paragraph.starts_with("* ") {
            true => format!("{}  ", indent),
            false => indent.to_string(),
        };

        let available_width
            = width.saturating_sub(indent.chars().count());

        let mut line_width
            = 0;

        for word in paragraph.split_whitespace() {
            let word_width
                = word.chars().count();

            if line_width > 0 && line_width + 1 + word_width > available_width {
                output.push('\n');
                output.push_str(&continuation_indent);
                line_width = continuation_indent.chars().count() - indent.chars().count();
            } else if line_width > 0 {
                output.push(' ');
                line_width += 1;
            }

            output.push_str(word);
            line_width += word_width;
        }

        output.push('\n');
    }
}

/// The help of a single command, as printed by `my-program my-command --help`:
/// its description, its usage, its options, its details, and its examples.
pub fn format_command_help(info: &Info, command_spec: &CommandSpec) -> String {
    let mut output
        = String::new();

    let usage
        = command_spec.usage();

    if let Some(documentation) = &command_spec.documentation {
        let description = match info.colorized {
            true => documentation.description().bold().to_string(),
            false => documentation.description().to_string(),
        };

        output.push_str(&description);
        output.push('\n');
    }

    write_section_header(&mut output, info, "Usage");

    output.push_str("  ");
    output.push_str(&usage.oneliner(info));
    output.push('\n');

    let options
        = usage.options();

    if !options.is_empty() {
        write_section_header(&mut output, info, "Options");

        let definition_width = options.iter()
            .map(|option| option.definition.chars().count())
            .max()
            .unwrap_or(0);

        let description_column
            = 2 + definition_width + 4;

        let is_inline
            = description_column + MIN_DESCRIPTION_WIDTH <= HELP_WIDTH;

        let description_indent = match is_inline {
            true => " ".repeat(description_column),
            false => " ".repeat(6),
        };

        for option in &options {
            let definition = match info.colorized {
                true => option.definition.bold().to_string(),
                false => option.definition.clone(),
            };

            output.push_str("  ");
            output.push_str(&definition);

            if option.description.is_empty() {
                output.push('\n');
                continue;
            }

            if is_inline {
                output.push_str(&" ".repeat(description_column - 2 - option.definition.chars().count()));
            } else {
                output.push('\n');
                output.push_str(&description_indent);
            }

            write_paragraphs(&mut output, &option.description, &description_indent, HELP_WIDTH, false);
        }
    }

    if let Some(details) = command_spec.documentation.as_ref().and_then(|documentation| documentation.details()) {
        write_section_header(&mut output, info, "Details");
        write_paragraphs(&mut output, details, "", HELP_WIDTH, true);
    }

    if !command_spec.examples.is_empty() {
        write_section_header(&mut output, info, "Examples");

        for (index, example) in command_spec.examples.iter().enumerate() {
            if index > 0 {
                output.push('\n');
            }

            write_paragraphs(&mut output, &example.description, "", HELP_WIDTH, false);

            let command = match info.colorized {
                true => format!("› {}", example.command).bright_white().to_string(),
                false => format!("› {}", example.command),
            };

            output.push_str("  ");
            output.push_str(&command);
            output.push('\n');
        }
    }

    output
}

pub struct Formatter<S> {
    phantom: PhantomData<S>,
}

impl<S: CommandProvider> Formatter<S> {
    pub fn format_error<'cmds>(info: &Info, err_type: &str, err: &impl Display, command_specs: impl IntoIterator<Item = &'cmds CommandSpec>) -> String {
        let mut result = String::new();
    
        result += &match info.colorized {
            true => format!("{}:", err_type).bright_red().to_string(),
            false => format!("{}:", err_type).to_string(),
        };
    
        result += " ";
        result += &err.to_string();

        let mut usage_lines
            = command_specs.into_iter()
                .map(|command_spec| command_spec.usage().oneliner(info))
                .collect::<Vec<_>>();

        usage_lines.sort();

        if !usage_lines.is_empty() {
            result += "\n\n";
            result += &usage_lines.join("\n");
        }
    
        result
    }

    pub fn format_parse_error(info: &Info, err: &clipanion_core::Error) -> String {
        match err {
            clipanion_core::Error::AmbiguousSyntax(candidate_specs)
                => Self::format_error(info, "Usage Error", &"The provided arguments are ambiguous and need to be refined further. Possible options are:", candidate_specs.iter().cloned()),

            clipanion_core::Error::BuildError(build_error)
                => Self::format_error(info, "Build Error", &build_error.to_string(), []),

            clipanion_core::Error::CommandError(command_spec, err)
                => Self::format_error(info, "Usage Error", err, [*command_spec]),

            clipanion_core::Error::InternalError
                => Self::format_error(info, "Usage Error", &"An internal error occurred.", []),

            clipanion_core::Error::NotFound(suggested_specs)
                => Self::format_error(info, "Usage Error", &"The specified command was not found. Did you mean one of those commands?", suggested_specs.iter().cloned()),
        }
    }
}

