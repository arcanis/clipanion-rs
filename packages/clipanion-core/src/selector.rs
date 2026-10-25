use std::collections::BTreeSet;

use itertools::Itertools;

use crate::{shared::{is_debug_enabled, ERROR_NODE_ID, SUCCESS_NODE_ID}, BuiltinCommand, CommandError, CommandSpec, Component, Error, State};

#[derive(Debug)]
pub enum SelectionResult<'cmds, 'args, T> {
    Builtin(BuiltinCommand<'cmds, 'args>),
    Command(&'cmds CommandSpec, State<'args>, T),
}

#[derive(Debug, Clone)]
pub struct Selector<'cmds, 'args> {
    pub commands: Vec<&'cmds CommandSpec>,
    pub args: Vec<&'args str>,
    pub states: Vec<State<'args>>,
    pub candidates: Vec<usize>,
}

impl<'cmds, 'args> Selector<'cmds, 'args> {
    pub fn new(commands: Vec<&'cmds CommandSpec>, args: Vec<&'args str>, states: Vec<State<'args>>) -> Self {
        let candidates = (0..states.len()).collect();

        Self {
            commands,
            args,
            states,
            candidates,
        }
    }

    fn prune_unsuccessful_nodes<'a>(&mut self) -> Result<(), Error<'cmds>> {
        let owned_candidates
            = std::mem::take(&mut self.candidates);

        let successful_candidates
            = owned_candidates.into_iter()
                .filter(|id| self.states[*id].node_id == SUCCESS_NODE_ID)
                .collect::<Vec<_>>();

        self.candidates = successful_candidates;
        Ok(())
    }

    fn fail_missing_required_options<'a>(&mut self) -> Result<(), Error<'cmds>> {
        let mut has_valid_states
            = false;
        let mut has_otherwise_valid_states
            = None;

        for &id in self.candidates.iter() {
            let state
                = &mut self.states[id];

            if state.node_id != SUCCESS_NODE_ID {
                continue;
            }

            let command
                = self.commands[state.context_id];

            let missing_required_options
                = command.required_options.iter()
                    .filter(|&option_id| !state.option_values.iter().any(|(id, _)| id == option_id))
                    .map(|option_id| self.commands[state.context_id].components[*option_id].is_option().unwrap().primary_name())
                    .collect::<Vec<_>>();

            if missing_required_options.len() == 0 {
                has_valid_states = true;
            } else {
                state.node_id = ERROR_NODE_ID;
                if has_otherwise_valid_states.is_none() {
                    has_otherwise_valid_states = Some(Some((command, missing_required_options)));
                } else if has_otherwise_valid_states.is_some() {
                    has_otherwise_valid_states = Some(None);
                }
            }
        }

        if !has_valid_states {
            if let Some(Some((command, missing_required_options))) = has_otherwise_valid_states {
                return Err(Error::CommandError(command, CommandError::MissingOptionArguments(missing_required_options.into_iter().map(|option| option.to_string()).collect::<Vec<_>>())));
            }
        }

        Ok(())
    }

    fn prune_by_hydration_results(&mut self, mut hydration_errors: Vec<(usize, CommandError)>) -> Result<(), Error<'cmds>> {
        let mut failed_hydrations
            = vec![false; self.states.len()];

        for (id, _) in hydration_errors.iter() {
            failed_hydrations[*id] = true;
        }

        self.candidates.retain(|id| {
            !failed_hydrations[*id]
        });

        if self.candidates.len() == 0 {
            if hydration_errors.len() == 1 {
                let (id, error)
                    = hydration_errors.remove(0);

                let state
                    = &self.states[id];

                let command_spec
                    = self.commands[state.context_id];

                return Err(Error::CommandError(command_spec, error));
            } else {
                let suggested_commands = hydration_errors.into_iter()
                    .map(|(id, _)| self.commands[id])
                    .collect::<Vec<_>>();

                return Err(Error::NotFound(suggested_commands));
            }
        }

        Ok(())
    }

    /**
     * This function is used to favour options that are more greedy than others
     * from the same command.
     * 
     * For example, if we have "foo bar baz" on a command "[arg] [...rest]", we
     * are going to have the two following options:
     * 
     * - arg = Some(foo), rest = vec!["bar", "baz"]
     * - arg = None, rest = vec!["foo", "bar", "baz"]
     * 
     * The first option is more greedy, so we remove the second one.
     */
    fn prune_by_greediness<'a>(&mut self) {
        let owned_candidates
            = std::mem::take(&mut self.candidates);

        // First we convert the positional values into a list of sort
        // criteria: first by positional index, then by number of values
        // provided to the positional argument, and finally by the argument
        // indexes.
        //
        // Let's say we have the following command:
        //
        // ```
        // my-cmd [...rest1] [...rest2]
        // ```
        // 
        // We want `my-cmd foo bar baz` to be parsed as:
        //
        // ```
        // rest1 = vec!["foo", "bar", "baz"]
        // rest2 = vec![]
        // ```
        //
        // And not (which we could have if we didn't favour states with more positionals of low indexes):
        //
        // ```
        // rest1 = vec![]
        // rest2 = vec!["foo", "bar", "baz"]
        // ```
        //
        // Nor (which we could have if we didn't favour states where positionals of low indexes have more values):
        //
        // ```
        // rest1 = vec!["foo"]
        // rest2 = vec!["bar", "baz"]
        // ```
        //
        // And, for the following command:
        //
        // ```
        // my-cmd [--opt [...args]] [positional]
        // ```
        //
        // We want `my-cmd --opt foo --opt bar baz` to be parsed as:
        //
        // ```
        // opt = vec![]
        // positional = "foo" 
        // opt = vec!["bar", "baz"]
        // ```
        //
        // And not (which we could have if we didn't favour lowest argument indexes, despite the precedent rules):
        //
        // ```
        // opt = vec!["foo"]
        // opt = vec!["bar"]
        // positional = "baz" 
        // ```
        //
        // We have a small problem: we want to first favour the lowest
        // positional indexes (since they signal higher greediness), but
        // we want to then favour the HIGHEST number of values provided,
        // for the same reason.
        //
        // To do this, we apply `-x-1` on the positional index which, since
        // we're dealing with unsigned integers, ensures that the lowest
        // indexes are now the highest values, and allowing us to use the
        // default `max_by` function to sort on both indexes and value count.
        //
        // Note: it's -x-1 (and not -x) because otherwise 0 would remain 0.
        //
        #[derive(Debug, PartialEq, Eq)]
        struct Flip(usize);

        impl PartialOrd for Flip {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(other.0.cmp(&self.0))
            }
        }

        impl Ord for Flip {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                other.0.cmp(&self.0)
            }
        }

        let mut states_with_positional_tracks = owned_candidates.into_iter().map(|id| {
            let positional_track = self.states[id].positional_values.iter().map(|(idx, values)| {
                (Flip(*idx), values.len(), values.iter().map(|value| Flip(value.index)).collect::<Vec<_>>())
            }).collect::<Vec<_>>();

            (id, positional_track)
        }).collect::<Vec<_>>();

        states_with_positional_tracks.sort_by(|a, b| {
            b.1.cmp(&a.1)
        });

        // We're now going to remove all the entries except for the first
        // one for each different command.
        let mut seen
            = vec![false; self.commands.len()];

        states_with_positional_tracks.retain(|(id, _)| {
            let context_id
                = self.states[*id].context_id;

            if seen[context_id] {
                false
            } else {
                seen[context_id] = true;
                true
            }
        });

        self.candidates = states_with_positional_tracks.into_iter()
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
    }

    fn prune_by_unused_positionals<'a>(&mut self) {
        let mut states_with_unused_positional_count
            = self.candidates.iter()
                .map(|&candidate_id| {
                    let state
                        = &self.states[candidate_id];

                    let command
                        = self.commands[state.context_id];

                    let mut seen
                        = vec![false; command.components.len()];

                    for (idx, _) in state.positional_values.iter() {
                        seen[*idx] = true;
                    }

                    let unused_positionals_count = command.components.iter()
                        .enumerate()
                        .filter(|(_, component)| matches!(component, Component::Positional(_)))
                        .filter(|(idx, _)| !seen[*idx])
                        .count();

                    (candidate_id, unused_positionals_count)
                })
                .collect::<Vec<_>>();

        states_with_unused_positional_count.sort_by_key(|(_, unused_positionals_count)| {
            *unused_positionals_count
        });

        let (_, min_unused_positional_count)
            = states_with_unused_positional_count.first()
                .expect("Expected at least one state")
                .clone();

        states_with_unused_positional_count.retain(|(_, unused_positionals_count)| {
            *unused_positionals_count == min_unused_positional_count
        });

        self.candidates = states_with_unused_positional_count.into_iter()
            .map(|(id, _)| id)
            .collect();
    }

    fn check_clipanion_builtins(&self) -> Option<BuiltinCommand<'cmds, 'args>> {
        if self.args.is_empty() {
            return None;
        }

        if self.args[0] == "--clipanion-commands" && self.args.len() == 1 {
            return Some(BuiltinCommand::Describe);
        }

        if self.args[0].starts_with("--clipanion-tokens") {
            return Some(BuiltinCommand::Tokenize(self.args[1..].to_vec()));
        }

        // Handle --clipanion-completion-script [--shell <shell>] [--command <command>]
        if self.args[0] == "--clipanion-completion-script" {
            let mut shell
                = None;
            let mut command
                = None;
            let mut i
                = 1;

            while i < self.args.len() {
                match self.args[i] {
                    "--shell" if i + 1 < self.args.len() => {
                        shell = Some(self.args[i + 1].to_string());
                        i += 2;
                    }
                    "--command" if i + 1 < self.args.len() => {
                        command = Some(self.args[i + 1].to_string());
                        i += 2;
                    }
                    _ => i += 1,
                }
            }

            return Some(BuiltinCommand::CompletionScript { shell, command });
        }

        // Handle --clipanion-complete <index> -- <args...>
        if self.args[0] == "--clipanion-complete" {
            let separator_pos
                = self.args.iter().position(|&arg| arg == "--");

            if let Some(sep_pos) = separator_pos {
                let index
                    = if sep_pos > 1 {
                        self.args[1].parse::<usize>().unwrap_or(0)
                    } else {
                        0
                    };

                let args
                    = self.args[sep_pos + 1..].to_vec();

                return Some(BuiltinCommand::Complete { index, args });
            } else {
                return Some(BuiltinCommand::Complete { index: 0, args: vec![] });
            }
        }

        None
    }

    fn handle_everything_is_an_error<T>(&mut self) -> Result<SelectionResult<'cmds, 'args, T>, Error<'cmds>> {
        if self.args.len() == 1 && matches!(self.args[0], "--version" | "-v") {
            return Ok(SelectionResult::Builtin(BuiltinCommand::Version));
        }

        if self.args.len() == 1 && matches!(self.args[0], "--help" | "-h") {
            return Ok(SelectionResult::Builtin(BuiltinCommand::Help(vec![])));
        }

        self.candidates = (0..self.states.len()).collect();

        self.prune_by_greediness();

        let owned_candidates
            = std::mem::take(&mut self.candidates);

        let context_ids = owned_candidates.into_iter()
            .map(|id| self.states[id].context_id)
            .collect::<BTreeSet<_>>();

        let commands = context_ids.into_iter()
            .map(|id| self.commands[id])
            .collect::<Vec<_>>();

        Err(Error::NotFound(commands))
    }

    pub fn resolve_state<F: Fn(&State<'args>) -> Result<T, CommandError>, T>(&mut self, f: F) -> Result<SelectionResult<'cmds, 'args, T>, Error<'cmds>> {
        // Framework-internal flags (--clipanion-*) must be intercepted unconditionally,
        // before any candidate filtering. These are never user-facing — they're only
        // invoked by scripts the framework itself generates. Without this early check,
        // a root-level proxy command would swallow them as proxy arguments.
        if let Some(builtin) = self.check_clipanion_builtins() {
            return Ok(SelectionResult::Builtin(builtin));
        }

        if is_debug_enabled() {
            println!("========== Pre-selection states ==========");

            for state in &self.states {
                println!("- {:?}", state);
            }
        }

        let help_contexts = self.states.iter()
            .filter(|state| state.is_help)
            .map(|state| state.context_id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|id| self.commands[id])
            .collect::<Vec<_>>();

        if help_contexts.len() > 0 && !self.states.iter().any(|state| state.node_id == SUCCESS_NODE_ID) {
            return Ok(SelectionResult::Builtin(BuiltinCommand::Help(help_contexts)));
        }

        self.fail_missing_required_options()?;

        if is_debug_enabled() {
            println!("========== After failing missing required options ==========");

            for state in &self.states {
                println!("- {:?}", state);
            }
        }

        if is_debug_enabled() {
            println!("========== Candidate filtering ==========");
            println!("initial candidates: {:?}", self.candidates);
        }

        self.prune_unsuccessful_nodes()?;

        if is_debug_enabled() {
            println!("after prune_unsuccessful_nodes: {:?}", self.candidates);
        }

        if self.candidates.len() == 0 {
            return self.handle_everything_is_an_error();
        }

        let hydration_results = self.candidates.iter()
            .map(|id| match f(&self.states[*id]) {
                Ok(result) => Ok((*id, result)),
                Err(err) => Err((*id, err)),
            })
            .collect::<Vec<_>>();

        let (successful_hydrations, unsuccessful_hydrations): (Vec<_>, Vec<_>)
            = hydration_results.into_iter()
                .partition_result();

        self.prune_by_hydration_results(unsuccessful_hydrations)?;

        if is_debug_enabled() {
            println!("after prune_by_hydration_results: {:?}", self.candidates);
        }

        self.prune_by_greediness();

        if is_debug_enabled() {
            println!("after prune_by_greediness: {:?}", self.candidates);
        }

        self.prune_by_unused_positionals();

        if is_debug_enabled() {
            println!("after prune_by_empty_positional_values: {:?}", self.candidates);
        }

        let owned_candidates
            = std::mem::take(&mut self.candidates);

        if owned_candidates.len() > 1 {
            let context_ids = owned_candidates.into_iter()
                .map(|id| self.states[id].context_id)
                .collect::<BTreeSet<_>>();

            let commands = context_ids.into_iter()
                .map(|id| self.commands[id])
                .collect::<Vec<_>>();

            return Err(Error::AmbiguousSyntax(commands));
        }

        let index
            = owned_candidates.first().unwrap();

        if is_debug_enabled() {
            println!("========== Selected state ==========");
            println!("{:?}", self.states[*index]);
        }
    
        let state
            = self.states.swap_remove(*index);
        let command_spec
            = self.commands[state.context_id];

        let (_, hydration_result)
            = successful_hydrations.into_iter()
                .find(|(id, _)| *id == *index)
                .unwrap();

        let res
            = SelectionResult::Command(command_spec, state, hydration_result);

        Ok(res)
    }
}
