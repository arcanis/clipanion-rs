use std::{fmt::Display, iter::once};

use itertools::Itertools;

use crate::{machine, runner::{self, DeriveState, RunnerState, ValidateTransition}, shared::{Arg, ERROR_NODE_ID, INITIAL_NODE_ID, SUCCESS_NODE_ID}, CommandUsageResult, Error, Selector};

#[cfg(test)]
use crate::SelectionResult;

/**
 */
#[derive(Debug, Clone)]
pub enum BuiltinCommand<'cmds> {
    Version,
    Help(Vec<&'cmds CommandSpec>),
}

#[derive(Debug, Clone)]
pub struct Info {
    pub program_name: String,
    pub binary_name: String,
    pub version: String,
    pub about: String,
    pub colorized: bool,
}

#[derive(Debug)]
pub struct Context {
    _command_id: usize,
    _command_spec: CommandSpec,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Ord, PartialOrd)]
pub struct State<'args> {
    pub context_id: usize,
    pub node_id: usize,
    pub keyword_count: usize,
    pub path: Vec<&'args str>,
    pub positional_values: Vec<(usize, Vec<&'args str>)>,
    pub option_values: Vec<(usize, Vec<&'args str>)>,
    pub post_double_slash: bool,
    pub is_help: bool,
}

impl<'args> State<'args> {
    pub fn values(&self) -> Vec<(usize, Vec<&'args str>)> {
        self.positional_values.clone()
            .into_iter()
            .chain(self.option_values.clone().into_iter())
            .sorted_by_key(|(id, _)| *id)
            .collect()
    }

    pub fn values_owned(self) -> Vec<(usize, Vec<String>)> {
        self.values()
            .into_iter()
            .map(|(id, values)| (id, values.into_iter().map(|s| s.to_string()).collect()))
            .collect()
    }
}

impl<'args> RunnerState for State<'args> {
    fn get_context_id(&self) -> usize {
        self.context_id
    }

    fn set_context_id(&mut self, context_id: usize) {
        self.context_id = context_id;
    }

    fn get_node_id(&self) -> usize {
        self.node_id
    }

    fn set_node_id(&mut self, node_id: usize) {
        self.node_id = node_id;
    }

    fn get_keyword_count(&self) -> usize {
        self.keyword_count
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check<'cmds> {
    IsOption(&'cmds str),
    IsOptionLike,
    IsOptionBinding(&'cmds str),
    IsNotOptionLike,
    IsBatch(Vec<char>),
}

impl<'cmds, 'args> ValidateTransition<'args, State<'args>> for Check<'cmds> {
    fn check(&self, state: &State<'args>, arg: &'args str) -> bool {
        match self {
            Check::IsOption(name) => {
                !state.post_double_slash && arg == *name
            },

            Check::IsOptionBinding(name) => {
                !state.post_double_slash && arg.starts_with(name) && arg.chars().nth(name.len()) == Some('=')
            },

            Check::IsOptionLike => {
                !state.post_double_slash && arg.starts_with("-") && arg != "--"
            },

            Check::IsNotOptionLike => {
                state.post_double_slash || !arg.starts_with("-")
            },

            Check::IsBatch(batch) => {
                !state.post_double_slash && arg.starts_with("-") && arg.len() > 2 && arg.chars().skip(1).all(|c| batch.contains(&c))
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attachment {
    Option,
    Positional,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reducer {
    EnableDoubleSlash,
    EnableHelp,
    IncreaseStaticCount,
    StartValue(Attachment, usize),
    PushValue(Attachment),
    BindValue(usize, usize),
    ResolveBatch(Vec<(char, usize)>),
}

impl<'args> DeriveState<'args, State<'args>> for Reducer {
    fn derive(&self, state: &mut State<'args>, _target_id: usize, token: &'args str) -> () {
        match self {
            Reducer::EnableHelp => {
                state.is_help = true;
            },

            Reducer::EnableDoubleSlash => {
                state.post_double_slash = true;
            },

            Reducer::IncreaseStaticCount => {
                state.keyword_count += 1;
            },

            Reducer::StartValue(attachment, attachment_id) => {
                match attachment {
                    Attachment::Option => {
                        state.option_values.push((*attachment_id, vec![]));
                    },

                    Attachment::Positional => {
                        state.positional_values.push((*attachment_id, vec![token]));
                    },
                }
            },

            Reducer::PushValue(attachment) => {
                match attachment {
                    Attachment::Option => {
                        if let Some((_, ref mut values)) = state.option_values.last_mut() {
                            values.push(token);
                        }
                    },

                    Attachment::Positional => {
                        if let Some((_, ref mut values)) = state.positional_values.last_mut() {
                            values.push(token);
                        }
                    },
                }
            },

            Reducer::BindValue(skip_len, option_id) => {
                state.option_values.push((*option_id, vec![&token[*skip_len + 1..]]));
            },

            Reducer::ResolveBatch(batch) => {
                for c in token.chars().skip(1) {
                    let index = batch.iter()
                        .find_map(|(other_c, option_id)| (c == *other_c).then_some(option_id));

                    if let Some(option_id) = index {
                        state.option_values.push((*option_id, vec![]));
                    }
                }
            },
        }
    }
}

type Machine<'cmds>
    = machine::Machine<'cmds, Option<Check<'cmds>>, Option<Reducer>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PositionalSpec {
    Keyword {
        expected: String,
    },

    Dynamic {
        name: String,
        description: String,

        min_len: usize,
        extra_len: Option<usize>,

        is_prefix: bool,
        is_proxy: bool,
    },
}

fn format_range(f: &mut std::fmt::Formatter<'_>, name: &str, min_len: usize, extra_len: Option<usize>) -> std::fmt::Result {
    if min_len > 0 {
        write!(f, "<{}>", name)?;

        for i in 1..min_len {
            write!(f, " <{}{}>", name, i + 1)?;
        }
    }

    let spacing
        = if min_len > 0 {" "} else {""};

    if extra_len != Some(0) {
        if let Some(extra_len) = extra_len {
            write!(f, "{}[…{}{}]", spacing, name, extra_len)?;
        } else {
            write!(f, "{}[…{}N]", spacing, name)?;
        }
    }

    Ok(())
}

fn format_collection<T: Display>(f: &mut std::fmt::Formatter<'_>, components: impl IntoIterator<Item = T>, separator: &str) -> std::fmt::Result {
    let mut first = true;

    for component in components {
        if !first {
            write!(f, "{}", separator)?;
        }

        write!(f, "{}", component)?;
        first = false;
    }

    Ok(())
}

impl std::fmt::Display for PositionalSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PositionalSpec::Keyword {expected} => {
                write!(f, "{}", expected)
            },

            PositionalSpec::Dynamic {name, min_len, extra_len, ..} => {
                format_range(f, name, *min_len, *extra_len)
            },
        }
    }
}

impl PositionalSpec {
    pub fn keyword<T: Into<String>>(value: T) -> Self {
        PositionalSpec::Keyword {
            expected: value.into(),
        }
    }

    pub fn optional() -> Self {
        PositionalSpec::Dynamic {
            name: "".to_string(),
            description: "".to_string(),

            min_len: 0,
            extra_len: Some(1),

            is_prefix: false,
            is_proxy: false,
        }
    }

    pub fn required() -> Self {
        PositionalSpec::Dynamic {
            name: "".to_string(),
            description: "".to_string(),
            
            min_len: 1,
            extra_len: Some(0),

            is_prefix: false,
            is_proxy: false,
        }
    }

    pub fn rest() -> Self {
        PositionalSpec::Dynamic {
            name: "".to_string(),
            description: "".to_string(),

            min_len: 0,
            extra_len: None,

            is_prefix: false,
            is_proxy: false,
        }
    }

    pub fn proxy() -> Self {
        PositionalSpec::Dynamic {
            name: "".to_string(),
            description: "".to_string(),
            
            min_len: 0,
            extra_len: None,

            is_prefix: false,
            is_proxy: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionSpec {
    pub primary_name: String,
    pub aliases: Vec<String>,

    pub description: String,

    pub min_len: usize,
    pub extra_len: Option<usize>,

    pub allow_binding: bool,
    pub allow_boolean: bool,
    pub is_hidden: bool,
    pub is_required: bool,
}

impl OptionSpec {
    fn parse_names(name: &str) -> (String, Vec<String>) {
        let names = name
            .split(',')
            .collect::<Vec<_>>();

        let (primary_name, aliases) = names
            .split_first()
            .unwrap();

        let aliases = aliases.iter()
            .map(|name| name.to_string())
            .collect();

        (primary_name.to_string(), aliases)
    }

    pub fn boolean(name: &str) -> Self {
        let (primary_name, aliases)
            = Self::parse_names(name);

        OptionSpec {
            primary_name,
            aliases,
            description: "".to_string(),
            
            min_len: 0,
            extra_len: Some(0),
            
            allow_binding: false,
            allow_boolean: true,
            is_hidden: false,
            is_required: false,
        }
    }

    pub fn parametrized(name: &str) -> Self {
        let (primary_name, aliases)
            = Self::parse_names(name);

        OptionSpec {
            primary_name,
            aliases,
            description: "".to_string(),
            
            min_len: 1,
            extra_len: Some(0),
            
            allow_binding: false,
            allow_boolean: false,
            is_hidden: false,
            is_required: true,
        }
    }

    pub fn all_names(&self) -> impl Iterator<Item = &str> {
        once(self.primary_name.as_str())
            .chain(self.aliases.iter().map(|alias| alias.as_str()))
    }
}

impl std::fmt::Display for OptionSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_required {
            write!(f, "<{}", self.primary_name)?;
        } else {
            write!(f, "[{}", self.primary_name)?;
        }

        for alias in &self.aliases {
            write!(f, ",{}", alias)?;
        }

        if self.min_len > 0 || self.extra_len != Some(0) {
            write!(f, " ")?;
            format_range(f, "arg", self.min_len, self.extra_len)?;
        }

        if self.is_required {
            write!(f, ">")
        } else {
            write!(f, "]")
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Component {
    Positional(PositionalSpec),
    Option(OptionSpec),
}

impl Component {
    pub fn is_option(&self) -> Option<&OptionSpec> {
        match self {
            Component::Option(spec) => Some(spec),
            _ => None,
        }
    }
}

impl std::fmt::Display for Component {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Component::Positional(spec)
                => write!(f, "{}", spec),

            Component::Option(spec)
                => write!(f, "{}", spec),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandSpec {
    pub category: Option<String>,
    pub description: Option<String>,
    pub details: Option<String>,
    pub paths: Vec<Vec<String>>,
    pub components: Vec<Component>,
    pub required_options: Vec<usize>,
}

impl std::fmt::Display for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let primary_path = self.paths.iter()
            .max_by_key(|path| path.len())
            .unwrap();

        let (prefix_components, suffix_components): (Vec<_>, _)
            = self.components.iter()
                .partition(|component| matches!(component, Component::Positional(PositionalSpec::Dynamic {is_prefix: true, ..})));

        let components
            = prefix_components.into_iter().map(|component| component.to_string())
                .chain(primary_path.iter().map(|segment| segment.to_string()))
                .chain(suffix_components.into_iter().map(|component| component.to_string()));

        format_collection(f, components, " ")?;

        Ok(())
    }
}

impl CommandSpec {
    pub fn is_default(&self) -> bool {
        self.paths.is_empty() || self.paths.iter().all(|path| path.is_empty())
    }

    pub fn longest_path(&self) -> Vec<&str> {
        self.paths.iter()
            .max_by_key(|path| path.len())
            .map(|path| path.iter().map(|segment| segment.as_ref()).collect())
            .unwrap_or_default()
    }

    pub fn usage(&self) -> CommandUsageResult {
        CommandUsageResult::new(self.clone())
    }

    pub fn build(&'_ self, command_id: usize) -> Machine<'_> {
        CommandBuilderContext::new(&self, command_id).build()
    }
}

pub struct CommandBuilderContext<'cmds> {
    machine: Machine<'cmds>,
    spec: &'cmds CommandSpec,
    batch_check: Vec<char>,
    batch_resolve: Vec<(char, usize)>,
    inhibit_options: usize,
    proxy_options: usize,
}

impl<'cmds> CommandBuilderContext<'cmds> {
    fn new(spec: &'cmds CommandSpec, command_id: usize) -> Self {
        // -v,--verbose => vec!["v"]
        let batch_check = spec.components.iter()
            .filter_map(|component| component.is_option())
            .flat_map(|option| option.all_names())
            .filter(|name| name.starts_with("-") && !name.starts_with("--"))
            .flat_map(|name| name.chars().skip(1))
            .collect::<Vec<_>>();

        // -v,--verbose => vec![("v", 0)]
        let batch_resolve = spec.components.iter()
            .enumerate()
            .filter_map(|(component_id, component)| component.is_option().map(|option| (option, component_id)))
            .flat_map(|(option, component_id)| option.all_names().map(move |name| (name, component_id)))
            .filter(|(name, _)| name.starts_with("-") && !name.starts_with("--"))
            .map(|(name, component_id)| (name.chars().skip(1).next().unwrap(), component_id))
            .collect::<Vec<_>>();

        CommandBuilderContext {
            machine: Machine::new(command_id),
            spec,
            batch_check,
            batch_resolve,
            inhibit_options: 0,
            proxy_options: 0,
        }
    }

    fn enter_inhibit_options(&mut self) {
        self.inhibit_options += 1;
    }

    fn exit_inhibit_options(&mut self) {
        self.inhibit_options -= 1;
    }

    fn enter_proxy_options(&mut self) {
        self.proxy_options += 1;
    }

    fn exit_proxy_options(&mut self) {
        self.proxy_options -= 1;
    }

    fn get_positional_check(&self) -> Option<Check<'cmds>> {
        if self.proxy_options > 0 {
            None
        } else {
            Some(Check::IsNotOptionLike)
        }
    }

    fn attach_options(&mut self, pre_options_node_id: usize) -> usize {
        if self.inhibit_options > 0 || self.proxy_options > 0 {
            return pre_options_node_id;
        }

        let post_options_node_id
            = self.machine.create_node();

        self.machine.register_shortcut(
            pre_options_node_id,
            post_options_node_id,
        );

        self.machine.register_dynamic(
            pre_options_node_id,
            Some(Check::IsOption("--")),
            pre_options_node_id,
            Some(Reducer::EnableDoubleSlash),
        );

        self.machine.register_dynamic(
            pre_options_node_id,
            Some(Check::IsBatch(self.batch_check.clone())),
            pre_options_node_id,
            Some(Reducer::ResolveBatch(self.batch_resolve.clone())),
        );

        let options = self.spec.components.iter()
            .enumerate()
            .filter_map(|(i, component)| match component {
                Component::Option(option) => Some((i, option)),
                _ => None,
            });

        for (option_id, option) in options {
            let all_names
                = once(&option.primary_name)
                    .chain(option.aliases.iter());

            for name in all_names {
                let mut post_option_node_id
                    = self.machine.create_node();

                self.machine.register_dynamic(
                    pre_options_node_id,
                    Some(Check::IsOption(name)),
                    post_option_node_id,
                    Some(Reducer::StartValue(Attachment::Option, option_id)),
                );

                let accepts_arguments
                    = option.min_len > 0 || option.extra_len != Some(0);

                if option.allow_boolean && accepts_arguments && option.min_len > 0 {
                    self.machine.register_shortcut(
                        post_option_node_id,
                        pre_options_node_id,
                    );
                }

                if accepts_arguments {
                    self.enter_inhibit_options();

                    post_option_node_id = self.attach_variadic(
                        post_option_node_id,
                        option.min_len,
                        option.extra_len,
                        Reducer::PushValue(Attachment::Option),
                        Reducer::PushValue(Attachment::Option),
                    );

                    self.exit_inhibit_options();

                    if option.min_len + option.extra_len.unwrap_or(0) == 1 {
                        self.machine.register_dynamic(
                            pre_options_node_id,
                            Some(Check::IsOptionBinding(name)),
                            post_option_node_id,
                            Some(Reducer::BindValue(name.len(), option_id)),
                        );
                    }
                }

                self.machine.register_shortcut(
                    post_option_node_id,
                    pre_options_node_id,
                );
            }
        }

        post_options_node_id
    }

    fn attach_required(&mut self, pre_node_id: usize, reducer: Reducer) -> usize {
        let next_node_id
            = self.machine.create_node();

        self.machine.register_dynamic(
            pre_node_id,
            self.get_positional_check(),
            next_node_id,
            Some(reducer),
        );

        self.attach_options(next_node_id)
    }

    fn attach_variadic(&mut self, pre_node_id: usize, min_len: usize, extra_len: Option<usize>, start_action: Reducer, subsequent_actions: Reducer) -> usize {
        let mut current_node_id
            = pre_node_id;

        let mut next_action
            = start_action.clone();

        for _ in 0..min_len {
            current_node_id = self.attach_required(current_node_id, next_action);
            next_action = subsequent_actions.clone();
        }

        match extra_len {
            Some(extra_len) => {
                if extra_len > 0 {
                    let end_node_id
                        = self.machine.create_node();

                    self.machine.register_shortcut(
                        current_node_id,
                        end_node_id,
                    );

                    for _ in 0..extra_len {
                        current_node_id = self.attach_required(current_node_id, next_action);
                        next_action = subsequent_actions.clone();

                        self.machine.register_shortcut(
                            current_node_id,
                            end_node_id,
                        );
                    }

                    current_node_id = end_node_id;
                }
            },

            None => {
                let end_node_id
                    = self.machine.create_node();

                self.machine.register_shortcut(
                    current_node_id,
                    end_node_id,
                );

                if next_action == start_action && next_action != subsequent_actions {
                    current_node_id = self.attach_required(current_node_id, next_action);
                    next_action = subsequent_actions;

                    self.machine.register_shortcut(
                        current_node_id,
                        end_node_id,
                    );
                }

                let post_variadic_node_id
                    = self.attach_required(current_node_id, next_action);

                self.machine.register_shortcut(
                    post_variadic_node_id,
                    current_node_id,
                );

                current_node_id = end_node_id;
            },
        }

        current_node_id
    }

    fn attach_positionals(&mut self, pre_node_id: usize, positionals: &Vec<(usize, &'cmds PositionalSpec)>) -> usize {
        let mut current_node_id
            = pre_node_id;

        for (id, spec) in positionals {
            match spec {
                PositionalSpec::Keyword {expected} => {
                    let next_node_id
                        = self.machine.create_node();

                    self.machine.register_static(
                        current_node_id,
                        Arg::User(expected),
                        next_node_id,
                        Some(Reducer::IncreaseStaticCount),
                    );

                    current_node_id
                        = self.attach_options(next_node_id);
                },

                PositionalSpec::Dynamic {min_len, extra_len, is_proxy, ..} => {
                    if *is_proxy {
                        self.enter_proxy_options();
                    }

                    current_node_id = self.attach_variadic(
                        current_node_id,
                        *min_len,
                        *extra_len,
                        Reducer::StartValue(Attachment::Positional, *id),
                        Reducer::PushValue(Attachment::Positional),
                    );

                    if *is_proxy {
                        self.exit_proxy_options();
                    }
                },
            }
        }

        current_node_id
    }

    fn build(mut self) -> Machine<'cmds> {
        let first_node_id
            = self.machine.create_node();

        self.machine.register_static(
            INITIAL_NODE_ID,
            Arg::StartOfInput,
            first_node_id,
            None,
        );

        let mut current_node_id
            = self.attach_options(first_node_id);

        let positional_components = self.spec.components.iter()
            .enumerate()
            .filter_map(|(i, component)| if let Component::Positional(spec) = component {Some((i, spec))} else {None})
            .collect::<Vec<_>>();

        let (prefix_components, positional_components)
            = positional_components.into_iter()
                .partition(|(_, spec)| matches!(spec, PositionalSpec::Dynamic {is_prefix: true, ..}));

        current_node_id
            = self.attach_positionals(current_node_id, &prefix_components);

        let is_first_positional_a_proxy
            = self.spec.components.iter()
                .find_map(|component| if let Component::Positional(PositionalSpec::Dynamic {is_prefix, is_proxy, ..}) = component {(!is_prefix).then_some(*is_proxy)} else {None})
                .unwrap_or(false);

        let help_node_id = (!is_first_positional_a_proxy).then(|| {
            let consumer_node_id
                = self.machine.create_node();

            self.machine.register_dynamic(
                consumer_node_id,
                None,
                consumer_node_id,
                None,
            );

            self.machine.register_static(
                consumer_node_id,
                Arg::EndOfInput,
                SUCCESS_NODE_ID,
                Some(Reducer::EnableHelp),
            );

            consumer_node_id
        });

        if !self.spec.paths.is_empty() && !self.spec.paths.iter().all(|path| path.is_empty()) {
            let post_paths_node_id
                = self.machine.create_node();

            for path in &self.spec.paths {
                let mut current_path_node_id
                    = current_node_id;

                if !path.is_empty() {
                    for segment in path {
                        let post_segment_node_id
                            = self.machine.create_node();

                        self.machine.register_static(
                            current_path_node_id,
                            Arg::User(segment.as_str()),
                            post_segment_node_id,
                            Some(Reducer::IncreaseStaticCount),
                        );

                        current_path_node_id
                            = self.attach_options(post_segment_node_id);
                    }

                    if let Some(help_node_id) = help_node_id {
                        self.machine.register_static(
                            current_path_node_id,
                            Arg::User("--help"),
                            help_node_id,
                            Some(Reducer::IncreaseStaticCount),
                        );
        
                        self.machine.register_static(
                            current_path_node_id,
                            Arg::User("-h"),
                            help_node_id,
                            Some(Reducer::IncreaseStaticCount),
                        );
                    }
                }

                self.machine.register_shortcut(
                    current_path_node_id,
                    post_paths_node_id,
                );
            }

            current_node_id = post_paths_node_id;
        }

        current_node_id
            = self.attach_positionals(current_node_id, &positional_components);

        self.machine.register_static(
            current_node_id,
            Arg::EndOfInput,
            SUCCESS_NODE_ID,
            None,
        );

        self.machine
    }
}

#[derive(Clone)]
pub struct CliBuilder<'cmds> {
    commands: Vec<&'cmds CommandSpec>,
}

impl<'cmds> CliBuilder<'cmds> {
    pub fn new() -> Self {
        CliBuilder {
            commands: vec![],
        }
    }

    pub fn add_command(&mut self, spec: &'cmds CommandSpec) -> &mut Self {
        self.commands.push(spec);
        self
    }

    pub fn compile(&self) -> Machine<'cmds> {
        let command_machines: Vec<Machine<'cmds>>
            = self.commands.iter()
                .enumerate()
                .map(|(command_id, &command)| command.build(command_id))
                .collect::<Vec<_>>();

        let mut machine
            = Machine::new_any_of(command_machines);

        machine.simplify_machine();
        machine
    }

    pub fn run<'args>(&self, args: &[&'args str]) -> Result<Selector<'cmds, 'args>, Error<'cmds>> {
        fn on_error<'args>(mut state: State<'args>, _: Arg<'args>) -> State<'args> {
            state.set_node_id(ERROR_NODE_ID);
            state
        }

        let machine
            = self.compile();

        let states: Vec<State<'args>>
            = runner::Runner::run(&machine, on_error, args).unwrap();
        
        let selector: Selector<'cmds, 'args>
            = Selector::new(self.commands.clone(), args.to_vec(), states);

        Ok(selector)
    }
}

#[test]
fn it_should_select_the_default_command_when_using_no_arguments() {
    let mut cli_builder
        = CliBuilder::new();

    let spec = CommandSpec {
        ..Default::default()
    };

    cli_builder.add_command(&spec);

    let result
        = cli_builder.run(&[""; 0]);

    let Ok(mut selector) = result else {
        panic!("Expected a selector result");
    };

    let selector_result
        = selector.resolve_state(|_| Ok(())).unwrap();

    let SelectionResult::Command(_, state, _) = selector_result else {
        panic!("Expected a command result");
    };

    assert_eq!(state.context_id, 0);
}

#[test]
fn it_should_select_the_default_command_when_using_mandatory_positional_arguments() {
    let mut cli_builder
        = CliBuilder::new();

    let spec = CommandSpec {
        components: vec![
            Component::Positional(PositionalSpec::required()),
            Component::Positional(PositionalSpec::required()),
        ],
        ..Default::default()
    };

    cli_builder.add_command(&spec);

    let result
        = cli_builder.run(&["foo", "bar"]);

    let Ok(mut selector) = result else {
        panic!("Expected a selector result");
    };

    let selector_result
        = selector.resolve_state(|_| Ok(())).unwrap();

    let SelectionResult::Command(_, state, _) = selector_result else {
        panic!("Expected a command result");
    };

    assert_eq!(state.context_id, 0);
}

#[test]
fn it_should_select_commands_by_their_path() {
    let mut cli_builder
        = CliBuilder::new();

    let spec1 = CommandSpec {
        components: vec![Component::Positional(PositionalSpec::keyword("foo"))],
        ..Default::default()
    };

    let spec2 = CommandSpec {
        components: vec![Component::Positional(PositionalSpec::keyword("bar"))],
        ..Default::default()
    };

    cli_builder.add_command(&spec1);
    cli_builder.add_command(&spec2);

    let result
        = cli_builder.run(&["foo"]);

    let Ok(mut selector) = result else {
        panic!("Expected a selector result");
    };

    let selector_result
        = selector.resolve_state(|_| Ok(())).unwrap();

    let SelectionResult::Command(_, state, _) = selector_result else {
        panic!("Expected a command result");
    };

    assert_eq!(state.context_id, 0);

    let result
        = cli_builder.run(&["bar"]);

    let Ok(mut selector) = result else {
        panic!("Expected a selector result");
    };

    let selector_result
        = selector.resolve_state(|_| Ok(())).unwrap();

    let SelectionResult::Command(_, state, _) = selector_result else {
        panic!("Expected a command result");
    };

    assert_eq!(state.context_id, 1);
}

#[test]
fn it_should_favor_paths_over_mandatory_positional_arguments() {
    let mut cli_builder
        = CliBuilder::new();

    let spec1 = CommandSpec {
        components: vec![Component::Positional(PositionalSpec::required())],
        ..Default::default()
    };

    let spec2 = CommandSpec {
        components: vec![Component::Positional(PositionalSpec::keyword("foo"))],
        ..Default::default()
    };

    cli_builder.add_command(&spec1);
    cli_builder.add_command(&spec2);

    let result
        = cli_builder.run(&["foo"]);

    let Ok(mut selector) = result else {
        panic!("Expected a selector result");
    };

    let selector_result
        = selector.resolve_state(|_| Ok(())).unwrap();

    let SelectionResult::Command(_, state, _) = selector_result else {
        panic!("Expected a command result");
    };

    assert_eq!(state.context_id, 1);
}

#[test]
fn it_should_favor_paths_over_optional_positional_arguments() {
    let mut cli_builder
        = CliBuilder::new();

    let spec1 = CommandSpec {
        components: vec![Component::Positional(PositionalSpec::optional())],
        ..Default::default()
    };

    let spec2 = CommandSpec {
        components: vec![Component::Positional(PositionalSpec::keyword("foo"))],
        ..Default::default()
    };

    cli_builder.add_command(&spec1);
    cli_builder.add_command(&spec2);

    let result
        = cli_builder.run(&["foo"]);

    let Ok(mut selector) = result else {
        panic!("Expected a selector result");
    };

    let selector_result
        = selector.resolve_state(|_| Ok(())).unwrap();

    let SelectionResult::Command(_, state, _) = selector_result else {
        panic!("Expected a command result");
    };

    assert_eq!(state.context_id, 1);
}

#[test]
fn it_should_favor_paths_filling_early_positional_arguments() {
    // Note: This test currently fails because of the logic described in selector.rs:
    //
    //     We're now going to remove all the entries except for the first
    //     one for each different command.
    //
    // Since in this test we have three different commands, the conflict remains. Should we address that, or update the test?

    let mut cli_builder
        = CliBuilder::new();

    let spec1 = CommandSpec {
        components: vec![Component::Positional(PositionalSpec::optional()), Component::Positional(PositionalSpec::rest())],
        ..Default::default()
    };

    let spec2 = CommandSpec {
        components: vec![Component::Positional(PositionalSpec::optional()), Component::Positional(PositionalSpec::optional()), Component::Positional(PositionalSpec::rest())],
        ..Default::default()
    };

    let spec3 = CommandSpec {
        components: vec![Component::Positional(PositionalSpec::rest())],
        ..Default::default()
    };

    cli_builder.add_command(&spec1);
    cli_builder.add_command(&spec2);
    cli_builder.add_command(&spec3);

    let result
        = cli_builder.run(&["foo", "bar", "baz"]);

    let Ok(mut selector) = result else {
        panic!("Expected a selector result");
    };

    let selector_result
        = selector.resolve_state(|_| Ok(())).unwrap();

    let SelectionResult::Command(_, state, _) = selector_result else {
        panic!("Expected a command result");
    };

    assert_eq!(state.context_id, 1);
}

#[test]
fn it_should_aggregate_positional_values() {
    let mut cli_builder
        = CliBuilder::new();

    let spec = CommandSpec {
        components: vec![
            Component::Positional(PositionalSpec::required()),
            Component::Positional(PositionalSpec::required()),
        ],
        ..Default::default()
    };

    cli_builder.add_command(&spec);

    let result
        = cli_builder.run(&["foo", "bar"]);

    let Ok(mut selector) = result else {
        panic!("Expected a selector result");
    };

    let selector_result
        = selector.resolve_state(|_| Ok(())).unwrap();

    let SelectionResult::Command(_, state, _) = selector_result else {
        panic!("Expected a command result");
    };

    assert_eq!(state.values(), vec![
        (0, vec!["foo"]),
        (1, vec!["bar"]),
    ]);
}

#[test]
fn it_should_aggregate_positional_values_with_rest() {
    let mut cli_builder
        = CliBuilder::new();

    let spec = CommandSpec {
        components: vec![
            Component::Positional(PositionalSpec::required()),
            Component::Positional(PositionalSpec::rest()),
        ],
        ..Default::default()
    };

    cli_builder.add_command(&spec);

    let result
        = cli_builder.run(&["foo", "bar", "baz"]);

    let Ok(mut selector) = result else {
        panic!("Expected a selector result");
    };

    let selector_result
        = selector.resolve_state(|_| Ok(())).unwrap();

    let SelectionResult::Command(_, state, _) = selector_result else {
        panic!("Expected a command result");
    };

    assert_eq!(state.values(), vec![
        (0, vec!["foo"]),
        (1, vec!["bar", "baz"]),
    ]);
}

#[test]
fn it_should_parse_batch_options() {
    let mut cli_builder
        = CliBuilder::new();

    let spec = CommandSpec {
        components: vec![
            Component::Option(OptionSpec::boolean("-V,--verbose")),
            Component::Option(OptionSpec::boolean("-Q,--quiet")),
        ],
        ..Default::default()
    };

    cli_builder.add_command(&spec);

    let result
        = cli_builder.run(&["-V"]);

    let Ok(mut selector) = result else {
        panic!("Expected a selector result");
    };

    let selector_result
        = selector.resolve_state(|_| Ok(())).unwrap();

    let SelectionResult::Command(_, state, _) = selector_result else {
        panic!("Expected a command result: {:?}", selector_result);
    };

    assert_eq!(state.values(), vec![
        (0, vec![]),
    ]);
}
