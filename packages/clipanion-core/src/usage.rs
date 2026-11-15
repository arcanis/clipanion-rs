use std::iter::once;

use colored::Colorize;

use crate::{CommandSpec, Component, Info, OptionDefinition};

pub struct CommandUsageOptions {
    pub detailed: bool,
    pub inline_options: bool,
}

/// A command's usage, borrowing its spec.
pub struct CommandUsageResult<'cmds> {
    pub command_spec: &'cmds CommandSpec,
}

impl<'cmds> CommandUsageResult<'cmds> {
    pub fn new(command_spec: &'cmds CommandSpec) -> Self {
        Self {
            command_spec,
        }
    }

    pub fn oneliner(&self, info: &Info) -> String {
        let usage_line
            = format!("› {} {}", info.binary_name, self.command_spec);

        let usage_line = match info.colorized {
            true => usage_line.bright_white().to_string(),
            false => usage_line.to_string(),
        };

        usage_line
    }

    /// The options that the command's help lists: all of them but the hidden
    /// ones. Their description includes their details, if they have any.
    pub fn options(&self) -> Vec<OptionUsage> {
        self.command_spec.components.iter()
            .filter_map(Component::is_option)
            .filter(|option| !option.is_hidden)
            .map(|option| {
                let description = match &option.documentation {
                    Some(documentation) => once(documentation.description())
                        .chain(documentation.details())
                        .collect::<Vec<_>>()
                        .join("\n\n"),

                    None
                        => String::new(),
                };

                OptionUsage {
                    preferred_name: option.primary_name().to_string(),
                    name_set: option.all_names().map(|name| name.to_string()).collect(),
                    definition: OptionDefinition(option).to_string(),
                    description,
                    required: option.is_required,
                }
            })
            .collect()
    }
}

pub struct OptionUsage {
    pub preferred_name: String,
    pub name_set: Vec<String>,
    pub definition: String,
    pub description: String,
    pub required: bool,
}
