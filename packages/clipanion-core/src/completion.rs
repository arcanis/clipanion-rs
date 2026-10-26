use std::collections::BTreeMap;

use crate::{
    builder::{CliMachine, CommandSpec, Component, PositionalSpec, Reducer, Str},
    runner::Runner,
    shared::{ArgKey, ERROR_NODE_ID},
};

/// A single completion candidate
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Completion {
    /// The completion text
    pub text: String,

    /// Description of this completion
    pub description: Option<String>,

    /// Whether this completion represents a path/command keyword
    pub is_path: bool,

    /// Whether this completion represents an option
    pub is_option: bool,
}

impl Completion {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            description: None,
            is_path: false,
            is_option: false,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn as_path(mut self) -> Self {
        self.is_path = true;
        self
    }

    pub fn as_option(mut self) -> Self {
        self.is_option = true;
        self
    }
}

/// Context for completion computation
#[derive(Debug, Clone)]
pub struct CompletionContext<'a> {
    /// Arguments before the current token
    pub args_before: Vec<&'a str>,

    /// The partial token being completed (empty if cursor is between tokens)
    pub current: &'a str,
}

impl<'a> CompletionContext<'a> {
    pub fn new(args_before: Vec<&'a str>, current: &'a str) -> Self {
        Self {
            args_before,
            current,
        }
    }

    /// Create a context from a full command line where the cursor is at the end
    pub fn from_args_at_end(args: Vec<&'a str>) -> Self {
        if args.is_empty() {
            Self::new(vec![], "")
        } else {
            let last_arg
                = args[args.len() - 1];
            Self::new(args[..args.len() - 1].to_vec(), last_arg)
        }
    }
}

/// Result of completion computation
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct CompletionResult {
    pub completions: Vec<Completion>,
}

/// Compute completions for a CLI given the current context
pub fn compute_completions<'cmds, 'args>(
    commands: &[&'cmds CommandSpec],
    machine: &CliMachine<'cmds>,
    context: &CompletionContext<'args>,
) -> CompletionResult {
    // Run the machine with the arguments before the current token to get the current states
    let states
        = Runner::run_partial(machine, commands, &context.args_before);

    // Now analyze each state to find valid completions
    // Use a BTreeMap keyed by text so that entries with descriptions
    // take precedence over bare entries for the same keyword.
    let mut completions: BTreeMap<String, Completion>
        = BTreeMap::new();

    for state in &states {
        if state.node_id == ERROR_NODE_ID {
            continue;
        }

        let node
            = &machine.nodes[state.node_id];
        let command
            = commands.get(state.context_id);

        // Build a set of already-used option component IDs
        let used_option_ids: std::collections::HashSet<usize>
            = state.option_values.iter()
                .map(|(id, _)| *id)
                .collect();

        // Collect static transitions (command paths, keywords)
        for (key, _transitions) in &node.statics {
            if let ArgKey::User(keyword) = key {
                // Check if the current partial matches
                if keyword.starts_with(context.current) {
                    let mut completion
                        = Completion::new(*keyword).as_path();

                    // If this keyword is the last segment of the command's path,
                    // attach the command's description.
                    if let Some(cmd) = command {
                        let idx
                            = state.keyword_count;

                        let is_last_segment = |path: &[Str]| {
                            idx + 1 == path.len()
                                && path.get(idx).map_or(false, |s| s == keyword)
                        };

                        if is_last_segment(&cmd.primary_path)
                            || cmd.aliases.iter().any(|a| is_last_segment(a))
                        {
                            if let Some(doc) = &cmd.documentation {
                                completion = completion.with_description(doc.description.clone());
                            }
                        }
                    }

                    insert_completion(&mut completions, completion);
                }
            }
        }

        // Collect dynamic transitions (options and positionals)
        // Only suggest if no keyword transition is reachable from the current
        // node (including through shortcuts), meaning the command path has been
        // fully consumed. This works for aliases of any length.
        let state_has_complete_path
            = command.is_some()
                && !has_reachable_keyword(machine, state.node_id);

        if state_has_complete_path {
            // Collect matching option completions for this state first, then
            // only include undescribed options when no described option matches.
            let mut option_candidates: Vec<Completion>
                = Vec::new();

            // Options can be given on this node, unless an option waits for its
            // values, or `--` ended them.
            let accepts_options
                = node.options.is_some()
                    && state.pending_values.is_none()
                    && !state.post_double_dash;

            let options = command.into_iter()
                .filter(|_| accepts_options)
                .flat_map(|cmd| cmd.components.iter().enumerate())
                .filter_map(|(component_id, component)| component.is_option().map(|option| (component_id, option)));

            for (component_id, opt) in options {
                for name in opt.all_names() {
                    // Only suggest long options (--foo), not short options (-f)
                    if !name.starts_with("--") {
                        continue;
                    }

                    // Check if the current partial matches
                    if name.starts_with(context.current) {
                        let is_single_use
                            = opt.extra_len.is_some();

                        if is_single_use && used_option_ids.contains(&component_id) {
                            continue;
                        }

                        let mut completion
                            = Completion::new(name).as_option();

                        if let Some(doc) = &opt.documentation {
                            completion = completion.with_description(doc.description.clone());
                        }

                        option_candidates.push(completion);
                    }
                }
            }

            let has_any_described
                = option_candidates.iter().any(|c| c.description.is_some());

            for c in option_candidates {
                if !has_any_described || c.description.is_some() {
                    insert_completion(&mut completions, c);
                }
            }

            for (check, transition) in &node.dynamics {
                match &transition.reducer {
                    Some(Reducer::StartPositional(_) | Reducer::PushPositional) if state.pending_values.is_none() => {
                        // Positional argument — invoke completer if available
                        if let Some(cmd) = command {
                            let component_id
                                = match &transition.reducer {
                                    Some(Reducer::StartPositional(id)) => Some(*id),
                                    Some(Reducer::PushPositional) => {
                                        state.positional_values.last().map(|(id, _)| *id)
                                    }
                                    _ => None,
                                };

                            if let Some(id) = component_id {
                                // A `proxy_program` proxy doesn't start at an option-like
                                // word: what's being typed can only be one of the options.
                                let starts_proxy_program
                                    = matches!(transition.reducer, Some(Reducer::StartPositional(_)))
                                        && matches!(cmd.components.get(id), Some(Component::Positional(PositionalSpec::Dynamic {is_proxy: true, proxy_program: true, ..})));

                                if starts_proxy_program && check.as_ref().is_some_and(|check| !check.check(cmd, state, context.current)) {
                                    continue;
                                }

                                if let Some(Component::Positional(PositionalSpec::Dynamic { completer: Some(f), .. })) = cmd.components.get(id) {
                                    // Build a context local to this positional: only include
                                    // values already consumed by this specific component.
                                    let positional_args: Vec<&str>
                                        = state.positional_values.iter()
                                            .filter(|(cid, _)| *cid == id)
                                            .flat_map(|(_, values)| values.iter().map(|v| v.value))
                                            .collect();

                                    let local_context
                                        = CompletionContext::new(positional_args, context.current);

                                    for c in f(&local_context) {
                                        insert_completion(&mut completions, c);
                                    }
                                }
                            }
                        }
                    }

                    _ => {}
                }
            }
        }

    }

    CompletionResult {
        completions: completions.into_values().collect(),
    }
}

/// Insert a completion into the map, preferring entries that carry a description.
fn insert_completion(map: &mut BTreeMap<String, Completion>, completion: Completion) {
    match map.entry(completion.text.clone()) {
        std::collections::btree_map::Entry::Vacant(e) => {
            e.insert(completion);
        }
        std::collections::btree_map::Entry::Occupied(mut e) => {
            if completion.description.is_some() && e.get().description.is_none() {
                e.insert(completion);
            }
        }
    }
}

/// Check whether a keyword transition is reachable from the given node,
/// either directly or through shortcuts. If a keyword is reachable, it
/// means the command path hasn't been fully consumed yet.
fn has_reachable_keyword(machine: &CliMachine, start_node_id: usize) -> bool {
    let mut visited
        = std::collections::HashSet::new();

    let mut stack
        = vec![start_node_id];

    while let Some(node_id) = stack.pop() {
        if !visited.insert(node_id) {
            continue;
        }

        let node
            = &machine.nodes[node_id];

        if node.statics.keys().any(|k| matches!(k, ArgKey::User(_))) {
            return true;
        }

        for shortcut in &node.shortcuts {
            stack.push(shortcut.to);
        }
    }

    false
}

/// Generate shell completion script
pub fn generate_completion_script(shell: Shell, command: &str) -> String {
    match shell {
        Shell::Bash => generate_bash_script(command),
        Shell::Zsh => generate_zsh_script(command),
        Shell::Fish => generate_fish_script(command),
    }
}

/// Supported shells for completion scripts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            _ => None,
        }
    }

    pub fn detect() -> Option<Self> {
        std::env::var("SHELL")
            .ok()
            .and_then(|s| {
                let shell_name
                    = s.rsplit('/').next()?;
                Self::from_str(shell_name)
            })
    }
}

/// Returns the environment variable name used to signal that completions are enabled.
/// Derived from the binary name (e.g. `my-cli` → `_MY_CLI_COMPLETIONS`).
pub fn completion_env_var(binary_name: &str) -> String {
    let sanitized: String
        = binary_name.chars()
            .map(|c| if c.is_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
            .collect();

    format!("_{}_COMPLETIONS", sanitized)
}

/// Returns `true` if the completion script has been sourced in the current shell session.
pub fn is_completion_enabled(binary_name: &str) -> bool {
    std::env::var(completion_env_var(binary_name)).is_ok()
}

fn generate_bash_script(command: &str) -> String {
    // Extract the binary name from the command (last path component, no arguments)
    let binary_name
        = command
            .split_whitespace()
            .next()
            .unwrap_or(command)
            .rsplit('/')
            .next()
            .unwrap_or(command);

    let env_var
        = completion_env_var(binary_name);

    format!(
        r#"# Bash completion script for {binary_name}
# Generated by clipanion-rs
# Add this to your ~/.bashrc or source it directly

export {env_var}=1

_{binary_name}_completions() {{
    local cur prev words cword
    _init_completion || return

    local IFS=$'\n'
    local completions
    # cword is 1-based and includes the command name, so subtract 1 for 0-based index without command
    local index=$((cword - 1))
    completions=$({command} --clipanion-complete "$index" -- "${{words[@]:1}}" 2>/dev/null)

    if [[ -n "$completions" ]]; then
        # Strip descriptions (tab-separated) since bash doesn't support them
        local words_only
        words_only=$(echo "$completions" | cut -f1)
        COMPREPLY=($(compgen -W "$words_only" -- "$cur"))
    fi
}}

complete -F _{binary_name}_completions {binary_name}
"#,
        binary_name = binary_name,
        env_var = env_var,
        command = command,
    )
}

fn generate_zsh_script(command: &str) -> String {
    let binary_name
        = command
            .split_whitespace()
            .next()
            .unwrap_or(command)
            .rsplit('/')
            .next()
            .unwrap_or(command);

    // Replace hyphens with underscores for valid zsh function name
    let func_name
        = binary_name.replace('-', "_");
    let env_var
        = completion_env_var(binary_name);

    format!(
        r#"# Zsh completion script for {binary_name}
# Generated by clipanion-rs
# Add this to your ~/.zshrc or place in a file in your $fpath

export {env_var}=1

_{func_name}() {{
    local -a completions
    local line

    # CURRENT is 1-based and includes command name, convert to 0-based index without command
    local index=$((CURRENT - 2))

    # Get completions from the CLI (format: text or text\tdescription)
    while IFS= read -r line; do
        if [[ "$line" == *$'\t'* ]]; then
            local text="${{line%%$'\t'*}}"
            local desc="${{line#*$'\t'}}"
            completions+=("${{text}}:${{desc}}")
        else
            completions+=("$line")
        fi
    done < <({command} --clipanion-complete "$index" -- "${{words[@]:1}}" 2>/dev/null)

    # Add completions with descriptions
    if (( $#completions )); then
        _describe 'completion' completions
    fi
}}

compdef _{func_name} {binary_name}
"#,
        binary_name = binary_name,
        env_var = env_var,
        func_name = func_name,
        command = command,
    )
}

fn generate_fish_script(command: &str) -> String {
    let binary_name
        = command
            .split_whitespace()
            .next()
            .unwrap_or(command)
            .rsplit('/')
            .next()
            .unwrap_or(command);

    let env_var
        = completion_env_var(binary_name);

    format!(
        r#"# Fish completion script for {binary_name}
# Generated by clipanion-rs
# Save this file to ~/.config/fish/completions/{binary_name}.fish

set -gx {env_var} 1

function __fish_{binary_name}_completions
    set -l tokens (commandline -opc)
    set -l current (commandline -ct)

    # Remove the command name itself (first token)
    set -e tokens[1]

    # commandline -opc already includes the partial token at cursor,
    # so don't append commandline -ct again.
    # When current is empty, cursor is after a space (new argument position).
    # When current is non-empty, it's the last element of tokens.
    set -l index (count $tokens)
    if test -n "$current"
        set index (math $index - 1)
    end

    {command} --clipanion-complete "$index" -- $tokens 2>/dev/null
end

complete -c {binary_name} -f -a '(__fish_{binary_name}_completions)'
"#,
        binary_name = binary_name,
        env_var = env_var,
        command = command,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{builder::{CliBuilder, Documentation, OptionSpec}, PositionalSpec};

    fn create_simple_cli() -> Vec<CommandSpec> {
        vec![
            CommandSpec {
                primary_path: vec!["add".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("-v,--verbose")),
                    Component::Option(OptionSpec::parametrized("-m,--message")),
                    Component::Positional(PositionalSpec::rest()),
                ].into(),
                ..Default::default()
            },
            CommandSpec {
                primary_path: vec!["commit".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("-a,--all")),
                    Component::Option(OptionSpec::parametrized("-m,--message")),
                ].into(),
                ..Default::default()
            },
            CommandSpec {
                primary_path: vec!["checkout".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("-b")),
                    Component::Positional(PositionalSpec::optional()),
                ].into(),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn test_complete_empty_input() {
        let specs
            = create_simple_cli();

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();
        let context
            = CompletionContext::new(vec![], "");

        let result
            = compute_completions(&command_refs, &machine, &context);

        // Should suggest command paths
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"add"));
        assert!(texts.contains(&"commit"));
        assert!(texts.contains(&"checkout"));
    }

    #[test]
    fn test_complete_partial_command() {
        let specs
            = create_simple_cli();

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();
        let context
            = CompletionContext::new(vec![], "co");

        let result
            = compute_completions(&command_refs, &machine, &context);

        // Should only suggest commands starting with "co" (commit, not checkout which starts with "ch")
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"commit"));
        assert!(!texts.contains(&"checkout")); // checkout starts with "ch", not "co"
        assert!(!texts.contains(&"add"));
    }

    #[test]
    fn test_complete_options_after_command() {
        let specs
            = create_simple_cli();

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();
        let context
            = CompletionContext::new(vec!["add"], "-");

        let result
            = compute_completions(&command_refs, &machine, &context);

        // Should suggest long options for 'add' command (short options are filtered out)
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(!texts.contains(&"-v")); // Short options filtered out
        assert!(texts.contains(&"--verbose"));
        assert!(!texts.contains(&"-m")); // Short options filtered out
        assert!(texts.contains(&"--message"));
    }

    #[test]
    fn test_complete_long_option_prefix() {
        let specs
            = create_simple_cli();

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();
        let context
            = CompletionContext::new(vec!["add"], "--v");

        let result
            = compute_completions(&command_refs, &machine, &context);

        // Should only suggest --verbose
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"--verbose"));
        assert!(!texts.contains(&"--message"));
    }

    #[test]
    fn test_complete_from_args_at_end() {
        let specs
            = create_simple_cli();

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();
        let context
            = CompletionContext::from_args_at_end(vec!["add", "--"]);

        let result
            = compute_completions(&command_refs, &machine, &context);

        // Should suggest long options
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"--verbose"));
        assert!(texts.contains(&"--message"));
    }

    #[test]
    fn test_filter_already_used_options() {
        let specs
            = create_simple_cli();

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // After using --verbose, it should not be suggested again
        let context
            = CompletionContext::new(vec!["add", "--verbose"], "-");

        let result
            = compute_completions(&command_refs, &machine, &context);

        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(!texts.contains(&"--verbose")); // Should be filtered out (already used)
        assert!(!texts.contains(&"-v")); // Short options are filtered out
        assert!(!texts.contains(&"-m")); // Short options are filtered out
        assert!(texts.contains(&"--message")); // Other long options should still be available
    }

    #[test]
    fn test_no_options_when_path_incomplete() {
        let specs
            = create_simple_cli();

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // When no command has been typed yet, options should not be suggested
        // even if the current token looks like an option
        let context
            = CompletionContext::new(vec![], "-");

        let result
            = compute_completions(&command_refs, &machine, &context);

        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        // Should only have command paths, no options
        assert!(!texts.contains(&"-v"));
        assert!(!texts.contains(&"--verbose"));
        assert!(!texts.contains(&"-m"));
        assert!(!texts.contains(&"--message"));
    }

    #[test]
    fn test_keyword_completions_include_command_description() {
        let specs = vec![
            CommandSpec {
                primary_path: vec!["add".into()].into(),
                documentation: Some(Documentation::new("Add files to staging", None)),
                components: vec![].into(),
                ..Default::default()
            },
            CommandSpec {
                primary_path: vec!["commit".into()].into(),
                documentation: Some(Documentation::new("Record changes", None)),
                components: vec![].into(),
                ..Default::default()
            },
            CommandSpec {
                primary_path: vec!["workspace".into(), "run".into()].into(),
                documentation: Some(Documentation::new("Run a workspace script", None)),
                components: vec![].into(),
                ..Default::default()
            },
            CommandSpec {
                primary_path: vec!["workspace".into(), "list".into()].into(),
                documentation: Some(Documentation::new("List workspaces", None)),
                components: vec![].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // Top-level: single-segment commands get their description
        let context
            = CompletionContext::new(vec![], "");

        let result
            = compute_completions(&command_refs, &machine, &context);

        let add
            = result.completions.iter().find(|c| c.text == "add").unwrap();
        assert_eq!(add.description.as_deref(), Some("Add files to staging"));

        let commit
            = result.completions.iter().find(|c| c.text == "commit").unwrap();
        assert_eq!(commit.description.as_deref(), Some("Record changes"));

        // "workspace" is not a final segment, so no description
        let workspace
            = result.completions.iter().find(|c| c.text == "workspace").unwrap();
        assert_eq!(workspace.description, None);

        // After "workspace": "run" and "list" are final segments and get descriptions
        let context
            = CompletionContext::new(vec!["workspace"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);

        let run
            = result.completions.iter().find(|c| c.text == "run").unwrap();
        assert_eq!(run.description.as_deref(), Some("Run a workspace script"));

        let list
            = result.completions.iter().find(|c| c.text == "list").unwrap();
        assert_eq!(list.description.as_deref(), Some("List workspaces"));
    }

    #[test]
    fn test_no_options_from_longer_path_when_shorter_path_complete() {
        // Command A: path = ["workspace"] with --json
        // Command B: path = ["workspace", "run"] with --verbose
        // After typing "workspace", only A's path is complete.
        // B's --verbose should NOT be suggested.
        let specs = vec![
            CommandSpec {
                primary_path: vec!["workspace".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--json")),
                ].into(),
                ..Default::default()
            },
            CommandSpec {
                primary_path: vec!["workspace".into(), "run".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--verbose")),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // `workspace --<TAB>` — only "workspace" command's path is complete
        let context
            = CompletionContext::new(vec!["workspace"], "--");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();

        assert!(texts.contains(&"--json"), "complete command's options should appear, got: {:?}", texts);
        assert!(!texts.contains(&"--verbose"), "incomplete path command's options should NOT appear, got: {:?}", texts);

        // `workspace run --<TAB>` — now both paths are complete
        let context
            = CompletionContext::new(vec!["workspace", "run"], "--");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();

        assert!(texts.contains(&"--verbose"), "after full path, options should appear, got: {:?}", texts);
    }

    #[test]
    fn test_options_with_shorter_alias() {
        // A command with a long primary path and a shorter alias should
        // show options when the alias is fully typed, not only when the
        // primary path is fully typed.
        let specs = vec![
            CommandSpec {
                primary_path: vec!["workspace".into(), "list".into()].into(),
                aliases: vec![vec!["workspaces".into()].into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--json")),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // `workspaces --<TAB>` — alias path is complete
        let context
            = CompletionContext::new(vec!["workspaces"], "--");

        let result
            = compute_completions(&command_refs, &machine, &context);

        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();

        assert!(texts.contains(&"--json"), "options should appear after completing the alias path, got: {:?}", texts);

        // `workspace list --<TAB>` — primary path is also complete
        let context
            = CompletionContext::new(vec!["workspace", "list"], "--");

        let result
            = compute_completions(&command_refs, &machine, &context);

        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();

        assert!(texts.contains(&"--json"), "options should appear after completing the primary path, got: {:?}", texts);
    }

    #[test]
    fn test_shell_detection() {
        assert_eq!(Shell::from_str("bash"), Some(Shell::Bash));
        assert_eq!(Shell::from_str("BASH"), Some(Shell::Bash));
        assert_eq!(Shell::from_str("zsh"), Some(Shell::Zsh));
        assert_eq!(Shell::from_str("fish"), Some(Shell::Fish));
        assert_eq!(Shell::from_str("unknown"), None);
    }

    #[test]
    fn test_bash_script_generation() {
        let script
            = generate_completion_script(Shell::Bash, "my-cli");
        assert!(script.contains("complete -F _my-cli_completions my-cli"));
        assert!(script.contains("--clipanion-complete"));
        assert!(script.contains("-- \"${words[@]:1}\"")); // Uses -- separator
    }

    #[test]
    fn test_zsh_script_generation() {
        let script
            = generate_completion_script(Shell::Zsh, "my-cli");
        assert!(script.contains("compdef _my_cli my-cli")); // Function name has underscores
        assert!(script.contains("--clipanion-complete"));
        assert!(script.contains("-- \"${words[@]:1}\"")); // Uses -- separator
        assert!(script.contains("_describe 'completion' completions")); // Uses _describe for descriptions
    }

    #[test]
    fn test_fish_script_generation() {
        let script
            = generate_completion_script(Shell::Fish, "my-cli");
        assert!(script.contains("complete -c my-cli"));
        assert!(script.contains("--clipanion-complete"));
        assert!(script.contains("-- $tokens")); // Uses -- separator
    }

    #[test]
    fn test_script_with_path_command() {
        let script
            = generate_completion_script(Shell::Bash, "/usr/local/bin/my-cli");
        // Should extract binary name from path
        assert!(script.contains("complete -F _my-cli_completions my-cli"));
    }

    fn branch_completer(ctx: &CompletionContext) -> Vec<Completion> {
        let branches
            = vec!["main", "develop", "feature/login", "feature/search"];

        branches.into_iter()
            .filter(|b| b.starts_with(ctx.current))
            .map(|b| Completion::new(b))
            .collect()
    }

    #[test]
    fn test_positional_completer() {
        let specs = vec![
            CommandSpec {
                primary_path: vec!["checkout".into()].into(),
                components: vec![
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "BRANCH".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: Some(0),
                        is_prefix: false,
                        is_proxy: false,
                        proxy_program: false,
                        completer: Some(branch_completer),
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // Complete with empty input after "checkout"
        let context
            = CompletionContext::new(vec!["checkout"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"main"));
        assert!(texts.contains(&"develop"));
        assert!(texts.contains(&"feature/login"));
        assert!(texts.contains(&"feature/search"));

        // Complete with partial input
        let context
            = CompletionContext::new(vec!["checkout"], "fe");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(!texts.contains(&"main"));
        assert!(!texts.contains(&"develop"));
        assert!(texts.contains(&"feature/login"));
        assert!(texts.contains(&"feature/search"));
    }

    #[test]
    fn test_positional_without_completer() {
        // Positionals without a completer should produce no positional completions
        let specs = vec![
            CommandSpec {
                primary_path: vec!["checkout".into()].into(),
                components: vec![
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "BRANCH".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: Some(0),
                        is_prefix: false,
                        is_proxy: false,
                        proxy_program: false,
                        completer: None,
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        let context
            = CompletionContext::new(vec!["checkout"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);

        assert!(result.completions.is_empty());
    }

    fn script_completer(ctx: &CompletionContext) -> Vec<Completion> {
        let scripts
            = vec!["build", "test", "lint", "start"];

        scripts.into_iter()
            .filter(|s| s.starts_with(ctx.current))
            .map(|s| Completion::new(s))
            .collect()
    }

    #[test]
    fn test_proxy_positional_completer() {
        // A proxy positional captures all remaining args (including option-like ones).
        // Its completer should still be invoked.
        let specs = vec![
            CommandSpec {
                primary_path: vec!["run".into()].into(),
                components: vec![
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "SCRIPT".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: None,
                        is_prefix: false,
                        is_proxy: true,
                        proxy_program: false,
                        completer: Some(script_completer),
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // Complete first proxy arg
        let context
            = CompletionContext::new(vec!["run"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"build"));
        assert!(texts.contains(&"test"));
        assert!(texts.contains(&"lint"));
        assert!(texts.contains(&"start"));

        // Complete with partial input
        let context
            = CompletionContext::new(vec!["run"], "t");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"test"));
        assert!(!texts.contains(&"build"));

        // Complete subsequent proxy arg (should still invoke completer via PushValue)
        let context
            = CompletionContext::new(vec!["run", "test"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"build"));
        assert!(texts.contains(&"test"));
    }

    /// Completer that echoes back the args_before it receives, so we can verify
    /// that the engine slices the context correctly for proxy positionals.
    fn echo_context_completer(ctx: &CompletionContext) -> Vec<Completion> {
        // Return each prior arg as a completion, plus "current:<current>" to verify
        let mut result: Vec<Completion>
            = ctx.args_before.iter()
                .map(|a| Completion::new(format!("before:{}", a)))
                .collect();

        result.push(Completion::new(format!("current:{}", ctx.current)));

        result
    }

    #[test]
    fn test_proxy_completer_receives_local_context() {
        let specs = vec![
            CommandSpec {
                primary_path: vec!["run".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--verbose")),
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "ARGS".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: None,
                        is_prefix: false,
                        is_proxy: true,
                        proxy_program: false,
                        completer: Some(echo_context_completer),
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // First proxy arg: completer should see no prior args
        // Full command: `run <TAB>` → args_before for completer = []
        let context
            = CompletionContext::new(vec!["run"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"current:"));
        assert!(!texts.iter().any(|t| t.starts_with("before:")));

        // Second proxy arg: completer should see only the first proxy arg
        // Full command: `run my-script <TAB>` → args_before for completer = ["my-script"]
        let context
            = CompletionContext::new(vec!["run", "my-script"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"before:my-script"));
        assert!(texts.contains(&"current:"));

        // Third proxy arg with partial: completer should see two prior proxy args
        // Full command: `run my-script --foo --b` → args_before for completer = ["my-script", "--foo"]
        let context
            = CompletionContext::new(vec!["run", "my-script", "--foo"], "--b");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"before:my-script"));
        assert!(texts.contains(&"before:--foo"));
        assert!(texts.contains(&"current:--b"));

        // With an option before the proxy, the machine has two valid parses:
        // 1. --verbose consumed as option → proxy sees ["my-script"]
        // 2. --verbose consumed as proxy arg → proxy sees ["--verbose", "my-script"]
        // Both parses contribute completions, so we see results from both.
        let context
            = CompletionContext::new(vec!["run", "--verbose", "my-script"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"before:my-script"));
        assert!(texts.contains(&"current:"));
    }

    #[test]
    fn test_proxy_with_keyword_before() {
        // Command: `run <keyword> <proxy...>`
        // The proxy completer should only see its own args, not the keyword.
        let specs = vec![
            CommandSpec {
                primary_path: vec!["run".into()].into(),
                components: vec![
                    Component::Positional(PositionalSpec::Keyword {
                        expected: "scripts".into(),
                    }),
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "ARGS".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: None,
                        is_prefix: false,
                        is_proxy: true,
                        proxy_program: false,
                        completer: Some(echo_context_completer),
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // `run scripts <TAB>` — first proxy arg, completer should see no prior args
        let context
            = CompletionContext::new(vec!["run", "scripts"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"current:"));
        assert!(!texts.iter().any(|t| t.starts_with("before:")),
            "keyword 'scripts' should not appear in proxy completer context, got: {:?}", texts);

        // `run scripts hello world <TAB>` — third proxy arg
        let context
            = CompletionContext::new(vec!["run", "scripts", "hello", "world"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"before:hello"));
        assert!(texts.contains(&"before:world"));
        assert!(!texts.iter().any(|t| *t == "before:scripts"),
            "keyword should not leak into proxy context");
    }

    #[test]
    fn test_proxy_without_completer() {
        // A proxy without a completer should produce no completions at all.
        // In particular, options from the command should not leak through.
        let specs = vec![
            CommandSpec {
                primary_path: vec!["run".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--verbose")),
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "ARGS".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: None,
                        is_prefix: false,
                        is_proxy: true,
                        proxy_program: false,
                        completer: None,
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // `run foo <TAB>` — proxy has started, no completer set
        let context
            = CompletionContext::new(vec!["run", "foo"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(!texts.contains(&"--verbose"),
            "options should not leak through proxy without completer");
        // The only completions might come from the non-proxy parse path (where
        // --verbose hasn't been consumed yet), but no positional completions.
    }

    #[test]
    fn test_proxy_multiple_commands_no_bleed() {
        // Two commands: `run <proxy...>` and `build <positional>`.
        // Completing after `run foo` should not suggest `build`'s options.
        fn run_completer(ctx: &CompletionContext) -> Vec<Completion> {
            vec![Completion::new(format!("run-completion:{}", ctx.current))]
        }

        let specs = vec![
            CommandSpec {
                primary_path: vec!["run".into()].into(),
                components: vec![
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "ARGS".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: None,
                        is_prefix: false,
                        is_proxy: true,
                        proxy_program: false,
                        completer: Some(run_completer),
                    }),
                ].into(),
                ..Default::default()
            },
            CommandSpec {
                primary_path: vec!["build".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--release")),
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "TARGET".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: Some(0),
                        is_prefix: false,
                        is_proxy: false,
                        proxy_program: false,
                        completer: Some(branch_completer),
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // `run foo <TAB>` — should only get run's proxy completions
        let context
            = CompletionContext::new(vec!["run", "foo"], "");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"run-completion:"));
        assert!(!texts.contains(&"--release"),
            "build's options should not appear in run's proxy completions");
        assert!(!texts.contains(&"main"),
            "build's positional completions should not bleed into run");
    }

    #[test]
    fn test_options_complete_before_proxy_starts() {
        // `run --<TAB>` should still suggest --verbose since no proxy arg consumed yet
        let specs = vec![
            CommandSpec {
                primary_path: vec!["run".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--verbose")),
                    Component::Option(OptionSpec::parametrized("--output")),
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "ARGS".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: None,
                        is_prefix: false,
                        is_proxy: true,
                        proxy_program: false,
                        completer: Some(script_completer),
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // `run --<TAB>` — no proxy arg yet, options should be suggested
        let context
            = CompletionContext::new(vec!["run"], "--");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"--verbose"));
        assert!(texts.contains(&"--output"));
    }

    #[test]
    fn test_proxy_program_completes_options_until_it_starts() {
        let specs = vec![
            CommandSpec {
                primary_path: vec!["run".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--verbose")),
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "ARGS".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: None,
                        is_prefix: false,
                        is_proxy: true,
                        proxy_program: true,
                        completer: Some(echo_context_completer),
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        let texts = |args_before: Vec<&str>, current: &str| -> Vec<String> {
            let context
                = CompletionContext::new(args_before, current);

            compute_completions(&command_refs, &machine, &context).completions.into_iter()
                .map(|completion| completion.text)
                .collect()
        };

        // `run --<TAB>`: the proxy can't start with an option-like word, only the options.
        let completions = texts(vec!["run"], "--");
        assert!(completions.contains(&"--verbose".to_string()), "got: {:?}", completions);
        assert!(!completions.iter().any(|text| text.starts_with("current:")), "got: {:?}", completions);

        // `run <TAB>`: the options, and the proxy's first word.
        let completions = texts(vec!["run"], "");
        assert!(completions.contains(&"--verbose".to_string()), "got: {:?}", completions);
        assert!(completions.contains(&"current:".to_string()), "got: {:?}", completions);

        // After `--`, the proxy can start with a dash.
        let completions = texts(vec!["run", "--"], "-");
        assert!(completions.contains(&"current:-".to_string()), "got: {:?}", completions);
        assert!(!completions.contains(&"--verbose".to_string()), "got: {:?}", completions);

        // Once it started, the proxy takes option-like words too.
        let completions = texts(vec!["run", "--verbose", "test"], "--");
        assert!(completions.contains(&"before:test".to_string()), "got: {:?}", completions);
        assert!(completions.contains(&"current:--".to_string()), "got: {:?}", completions);
        assert!(!completions.contains(&"--verbose".to_string()), "got: {:?}", completions);
    }

    #[test]
    fn test_proxy_swallows_option_like_args() {
        // Once proxy has consumed at least one arg, option-like args should go
        // to the proxy completer rather than being treated as CLI options.
        let specs = vec![
            CommandSpec {
                primary_path: vec!["run".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--verbose")),
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "ARGS".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: None,
                        is_prefix: false,
                        is_proxy: true,
                        proxy_program: false,
                        completer: Some(echo_context_completer),
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let command_refs: Vec<&CommandSpec>
            = specs.iter().collect();

        // `run test --some-flag --<TAB>` — proxy has started, option-like input
        // should be passed to the proxy completer, not matched as CLI options.
        let context
            = CompletionContext::new(vec!["run", "test", "--some-flag"], "--");

        let result
            = compute_completions(&command_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        // The echo completer should receive the option-like args as proxy context
        assert!(texts.contains(&"before:test"));
        assert!(texts.contains(&"before:--some-flag"));
        assert!(texts.contains(&"current:--"));
    }

    #[test]
    fn test_cross_binary_proxy_delegation() {
        // Simulates binary A having a proxy that delegates completion to binary B.
        //
        // Binary B is a CLI with commands: `install`, `publish`, `test`
        // Binary A has: `workspace run <proxy...>` where the proxy completer
        // constructs binary B's CLI and calls compute_completions on it.

        // This is Binary B's "CLI definition"
        fn inner_cli_specs() -> Vec<CommandSpec> {
            vec![
                CommandSpec {
                    primary_path: vec!["install".into()].into(),
                    components: vec![
                        Component::Option(OptionSpec::boolean("--frozen")),
                    ].into(),
                    ..Default::default()
                },
                CommandSpec {
                    primary_path: vec!["publish".into()].into(),
                    components: vec![
                        Component::Option(OptionSpec::boolean("--dry-run")),
                        Component::Option(OptionSpec::parametrized("--tag")),
                    ].into(),
                    ..Default::default()
                },
                CommandSpec {
                    primary_path: vec!["test".into()].into(),
                    components: vec![
                        Component::Option(OptionSpec::boolean("--watch")),
                    ].into(),
                    ..Default::default()
                },
            ]
        }

        // Binary A's proxy completer: delegates to binary B's completion engine
        fn delegate_completer(ctx: &CompletionContext) -> Vec<Completion> {
            let specs
                = inner_cli_specs();

            let mut builder
                = CliBuilder::new();

            for spec in &specs {
                builder.add_command(spec);
            }

            let machine
                = builder.compile();

            let command_refs: Vec<&CommandSpec>
                = specs.iter().collect();

            // Forward the positional-local context directly to binary B's engine
            let result
                = compute_completions(&command_refs, &machine, ctx);

            result.completions
        }

        // Binary A's CLI:
        //   - `workspace run <proxy...>` (delegates to binary B)
        //   - `workspace list` (a non-proxy sibling command)
        //   - `deploy <target>` (a top-level non-proxy command)
        let outer_specs = vec![
            CommandSpec {
                primary_path: vec!["workspace".into(), "run".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--verbose")),
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "ARGS".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: None,
                        is_prefix: false,
                        is_proxy: true,
                        proxy_program: false,
                        completer: Some(delegate_completer),
                    }),
                ].into(),
                ..Default::default()
            },
            CommandSpec {
                primary_path: vec!["workspace".into(), "list".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--json")),
                ].into(),
                ..Default::default()
            },
            CommandSpec {
                primary_path: vec!["deploy".into()].into(),
                components: vec![
                    Component::Option(OptionSpec::boolean("--force")),
                    Component::Positional(PositionalSpec::Dynamic {
                        name: "TARGET".into(),
                        documentation: None,
                        min_len: 1,
                        extra_len: Some(0),
                        is_prefix: false,
                        is_proxy: false,
                        proxy_program: false,
                        completer: Some(branch_completer),
                    }),
                ].into(),
                ..Default::default()
            },
        ];

        let mut builder
            = CliBuilder::new();

        for spec in &outer_specs {
            builder.add_command(spec);
        }

        let machine
            = builder.compile();

        let outer_refs: Vec<&CommandSpec>
            = outer_specs.iter().collect();

        // `<TAB>` — top-level: should see all path keywords from binary A
        let context
            = CompletionContext::new(vec![], "");

        let result
            = compute_completions(&outer_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"workspace"), "got: {:?}", texts);
        assert!(texts.contains(&"deploy"), "got: {:?}", texts);
        // No inner CLI commands should leak to the top level
        assert!(!texts.contains(&"install"));
        assert!(!texts.contains(&"publish"));

        // `workspace <TAB>` — should see both `run` and `list` subcommands
        let context
            = CompletionContext::new(vec!["workspace"], "");

        let result
            = compute_completions(&outer_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"run"), "got: {:?}", texts);
        assert!(texts.contains(&"list"), "got: {:?}", texts);
        assert!(!texts.contains(&"deploy"), "sibling top-level command should not appear here");

        // `workspace run <TAB>` — should see binary B's commands merged with
        // binary A's --verbose (proxy hasn't started yet)
        let context
            = CompletionContext::new(vec!["workspace", "run"], "");

        let result
            = compute_completions(&outer_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"install"), "inner CLI commands should appear, got: {:?}", texts);
        assert!(texts.contains(&"publish"));
        assert!(texts.contains(&"test"));
        assert!(texts.contains(&"--verbose"), "outer option still valid before proxy starts");
        // Sibling command options should NOT appear
        assert!(!texts.contains(&"--json"), "workspace list's options should not appear here");
        assert!(!texts.contains(&"--force"), "deploy's options should not appear here");
        // Sibling completions from branch_completer should not appear
        assert!(!texts.contains(&"main"));

        // `workspace run t<TAB>` — should filter to inner commands starting with "t"
        let context
            = CompletionContext::new(vec!["workspace", "run"], "t");

        let result
            = compute_completions(&outer_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"test"));
        assert!(!texts.contains(&"install"));
        assert!(!texts.contains(&"publish"));

        // `workspace run publish --<TAB>` — should see binary B's publish options
        let context
            = CompletionContext::new(vec!["workspace", "run", "publish"], "--");

        let result
            = compute_completions(&outer_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"--dry-run"), "should see inner command's options, got: {:?}", texts);
        assert!(texts.contains(&"--tag"));
        // publish's options, not install's or test's
        assert!(!texts.contains(&"--frozen"));
        assert!(!texts.contains(&"--watch"));
        // Outer CLI options should not appear (proxy has started)
        assert!(!texts.contains(&"--json"));
        assert!(!texts.contains(&"--force"));

        // `workspace run install --frozen <TAB>` — after using an inner option
        let context
            = CompletionContext::new(vec!["workspace", "run", "install", "--frozen"], "");

        let result
            = compute_completions(&outer_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        // --frozen already used and is single-use, should not be suggested again
        assert!(!texts.contains(&"--frozen"),
            "already-used inner option should be filtered, got: {:?}", texts);

        // `workspace list --<TAB>` — sibling command should work independently
        let context
            = CompletionContext::new(vec!["workspace", "list"], "--");

        let result
            = compute_completions(&outer_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"--json"), "got: {:?}", texts);
        // No proxy/inner CLI options should leak
        assert!(!texts.contains(&"--verbose"));
        assert!(!texts.contains(&"--frozen"));
        assert!(!texts.contains(&"--dry-run"));

        // `deploy <TAB>` — should see branch completions, not inner CLI commands
        let context
            = CompletionContext::new(vec!["deploy"], "");

        let result
            = compute_completions(&outer_refs, &machine, &context);
        let texts: Vec<&str>
            = result.completions.iter().map(|c| c.text.as_str()).collect();
        assert!(texts.contains(&"main"), "deploy should see branch completions, got: {:?}", texts);
        assert!(texts.contains(&"develop"));
        assert!(!texts.contains(&"install"), "inner CLI should not leak into deploy");
        assert!(!texts.contains(&"publish"));
    }
}
