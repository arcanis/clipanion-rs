use crate::{builder::{CliMachine, CommandSpec, Component, OptionSpec, Reducer, State}, node::NodeOptions, options::{self, OptionWord, PendingValues}, shared::{is_debug_enabled, Arg, UserArg, ERROR_NODE_ID, INITIAL_NODE_ID}, transition::Transition};

/// Runs the machine of a program over its arguments. The machine says where
/// keywords and positionals go; the options are read from the commands' specs,
/// on the nodes the machine marks (see `options.rs`).
pub struct Runner<'machine, 'cmds, 'args> {
    machine: &'machine CliMachine<'cmds>,
    commands: &'machine [&'cmds CommandSpec],

    states: Vec<State<'args>>,
    next_states: Vec<State<'args>>,

    // Colors are used to avoid infinite loops.
    node_colors: Vec<usize>,
    current_color: usize,

    debug: bool,
}

impl<'machine, 'cmds, 'args> Runner<'machine, 'cmds, 'args> {
    /**
     * Run the state machine with the given arguments.
     */
    pub fn run(machine: &'machine CliMachine<'cmds>, commands: &'machine [&'cmds CommandSpec], args: &[&'args str]) -> Vec<State<'args>> {
        let mut runner
            = Runner::new(machine, commands);

        runner.send(args);

        runner.update(Arg::EndOfInput);
        runner.digest()
    }

    /**
     * Run the state machine with the given arguments. Unlike `run`, this method will mark
     * all states that are not in an error state as successful. This can be useful when you
     * want to obtain information about a partial command line (for example in a documentation
     * you often want to reference a command but not provide all the arguments).
     */
    pub fn run_partial(machine: &'machine CliMachine<'cmds>, commands: &'machine [&'cmds CommandSpec], args: &[&'args str]) -> Vec<State<'args>> {
        let mut runner
            = Runner::new(machine, commands);

        runner.send(args);

        runner.states.retain(|state| {
            state.node_id != ERROR_NODE_ID
        });

        runner.digest()
    }

    pub fn new(machine: &'machine CliMachine<'cmds>, commands: &'machine [&'cmds CommandSpec]) -> Self {
        let mut runner = Runner {
            machine,
            commands,
            states: vec![],
            next_states: vec![],
            node_colors: vec![0; machine.nodes.len()],
            current_color: 0,
            debug: is_debug_enabled(),
        };

        let initial_state
            = State::default();

        runner.next_states.push(initial_state.clone());

        for shortcut in &machine.nodes[INITIAL_NODE_ID].shortcuts {
            runner.transition_to(&initial_state, shortcut, Arg::StartOfInput);
        }

        runner.states = std::mem::take(&mut runner.next_states);
        runner
    }

    fn send(&mut self, args: &[&'args str]) {
        if self.debug {
            println!("========== Parsing ==========");
        }

        self.update(Arg::StartOfInput);

        let machine
            = self.machine;

        for state in self.states.iter_mut() {
            state.context_id = machine.nodes[state.node_id].context;
        }

        for (index, value) in args.iter().enumerate() {
            self.update(Arg::User(UserArg {value, index}));
        }
    }

    fn transition_to(&mut self, from_state: &State<'args>, transition: &Transition<Option<Reducer>>, token: Arg<'args>) {
        let mut next_state
            = from_state.clone();

        if let Some(reducer) = &transition.reducer {
            reducer.apply(&mut next_state, token);
        }

        if self.debug {
            println!("  [{}] {} -> {} (reducer: {:?})", from_state.context_id, from_state.node_id, transition.to, transition.reducer);
        }

        self.enter(next_state, transition.to);
    }

    /// Puts `state` on `node_id`, and on the nodes its shortcuts lead to.
    fn enter(&mut self, state: State<'args>, node_id: usize) {
        self.current_color = self.current_color.wrapping_add(1);
        self.enter_with_color(state, node_id, self.current_color);
    }

    fn enter_with_color(&mut self, mut state: State<'args>, node_id: usize, color: usize) {
        state.node_id = node_id;
        self.node_colors[node_id] = color;

        let machine
            = self.machine;

        for shortcut in &machine.nodes[node_id].shortcuts {
            if self.node_colors[shortcut.to] != color {
                if self.debug {
                    println!("  [{}] {} -> {} (shortcut)", state.context_id, node_id, shortcut.to);
                }

                self.enter_with_color(state.clone(), shortcut.to, color);
            }
        }

        self.next_states.push(state);
    }

    fn fail(&mut self, mut state: State<'args>) {
        state.node_id = ERROR_NODE_ID;
        self.next_states.push(state);
    }

    pub fn update(&mut self, token: Arg<'args>) {
        if self.debug {
            println!("{:?}", token);
        }

        let states
            = std::mem::take(&mut self.states);

        for state in states {
            if state.node_id == ERROR_NODE_ID {
                self.next_states.push(state);
                continue;
            }

            match (token, state.pending_values) {
                (Arg::User(arg), Some(pending)) => {
                    self.feed_value(state, pending, arg);
                },

                // The input ended while an option was waiting for values.
                (_, Some(_)) => {
                    self.fail(state);
                },

                (_, None) => {
                    self.feed(state, token);
                },
            }
        }

        self.states
            = std::mem::take(&mut self.next_states);

        self.trim_shortest_branches();
    }

    fn feed(&mut self, state: State<'args>, token: Arg<'args>) {
        let machine
            = self.machine;

        let node
            = &machine.nodes[state.node_id];

        let mut transitioned
            = false;

        if let Some(transitions) = node.statics.get(&token.into()) {
            for transition in transitions {
                self.transition_to(&state, transition, token);
                transitioned = true;
            }
        }

        if let Arg::User(arg) = token {
            let spec
                = self.commands[state.context_id];

            let option_word = match node.options {
                Some(NodeOptions {help}) if !state.post_double_dash => options::parse_option_word(spec, arg.value, help),
                _ => None,
            };

            if let Some(option_word) = option_word {
                self.apply_option_word(&state, spec, option_word, arg);
                transitioned = true;
            }

            for (check, transition) in &node.dynamics {
                if check.as_ref().is_none_or(|check| check.check(spec, &state, arg.value)) {
                    self.transition_to(&state, transition, token);
                    transitioned = true;
                }
            }
        }

        if !transitioned {
            self.fail(state);
        }
    }

    /// Applies an option word read on the state's node, where the state stays.
    fn apply_option_word(&mut self, state: &State<'args>, spec: &CommandSpec, option_word: OptionWord, arg: UserArg<'args>) {
        let mut next_state
            = state.clone();

        match option_word {
            OptionWord::EndOfOptions => {
                next_state.post_double_dash = true;
                next_state.push_syntax(arg);

                self.enter(next_state, state.node_id);
            },

            OptionWord::Help => {
                next_state.is_help = true;
                next_state.push_syntax(arg);

                self.fail(next_state);
            },

            OptionWord::Options(option_uses) => {
                let mut can_go_without_values
                    = false;

                for option_use in &option_uses {
                    next_state.start_option(arg, option_use);

                    if option_use.value.is_none() {
                        if let Some((pending, optional)) = options::values_after_name(option_spec(spec, option_use.option_id)) {
                            next_state.pending_values = Some(pending);
                            can_go_without_values = optional;
                        }
                    }
                }

                if can_go_without_values {
                    let mut without_values
                        = next_state.clone();

                    without_values.pending_values = None;
                    self.enter(without_values, state.node_id);
                }

                self.settle(next_state, state.node_id);
            },
        }
    }

    /// Gives a value to the option the state waits for.
    fn feed_value(&mut self, mut state: State<'args>, mut pending: PendingValues, arg: UserArg<'args>) {
        if !options::accepts_value(arg.value) {
            return self.fail(state);
        }

        state.push_option_value(arg);

        if pending.required > 0 {
            pending.required -= 1;
        } else {
            pending.extra = pending.extra.map(|extra| extra - 1);
        }

        state.pending_values = Some(pending);

        let node_id
            = state.node_id;

        self.settle(state, node_id);
    }

    /// Puts back on its node a state that read an option, or one of its values:
    /// the option is done, or waits for more values, or both when the values it
    /// can still take are optional.
    fn settle(&mut self, mut state: State<'args>, node_id: usize) {
        match state.pending_values {
            Some(pending) if pending.required > 0 => {
                // Only its values can follow: the node's other transitions wait.
                state.node_id = node_id;
                self.next_states.push(state);
            },

            Some(pending) if pending.extra != Some(0) => {
                let mut done
                    = state.clone();

                done.pending_values = None;
                self.enter(done, node_id);

                state.node_id = node_id;
                self.next_states.push(state);
            },

            _ => {
                state.pending_values = None;
                self.enter(state, node_id);
            },
        }
    }

    fn trim_shortest_branches(&mut self) {
        let max_keyword_count
            = self.states.iter()
                .map(|state| state.keyword_count)
                .max();

        if let Some(max_keyword_count) = max_keyword_count {
            self.states.retain(|state| state.keyword_count == max_keyword_count);
        }
    }

    pub fn digest(self) -> Vec<State<'args>> {
        self.states
    }
}

fn option_spec(spec: &CommandSpec, option_id: usize) -> &OptionSpec {
    match &spec.components[option_id] {
        Component::Option(option) => option,
        _ => unreachable!("Expected an option"),
    }
}
