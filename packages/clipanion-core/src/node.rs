use std::collections::HashMap;

use crate::{shared::ArgKey, transition::Transition};

/// Marks a node where the command's options can be given. Options aren't nodes
/// of the machine: the runner reads them from the command's spec, and a state
/// that reads one stays on the node it was on (see `options.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeOptions {
    /// Whether `-h` and `--help` ask for the command's help here.
    pub help: bool,
}

#[derive(Debug, Clone)]
pub struct Node<'a, TCheck, TReducer> {
    pub context: usize,
    pub options: Option<NodeOptions>,
    pub dynamics: Vec<(TCheck, Transition<TReducer>)>,
    pub shortcuts: Vec<Transition<TReducer>>,
    pub statics: HashMap<ArgKey<'a>, Vec<Transition<TReducer>>>,
}

impl<'a, TCheck, TReducer> Node<'a, TCheck, TReducer> {
    pub fn new() -> Self {
        Self {
            context: 0,
            options: None,
            dynamics: vec![],
            shortcuts: vec![],
            statics: HashMap::new(),
        }
    }

    pub fn transitions_mut(&mut self) -> impl Iterator<Item = &mut Transition<TReducer>> {
        self.dynamics.iter_mut().map(|(_, transition)| transition)
            .chain(self.shortcuts.iter_mut())
            .chain(self.statics.values_mut().flatten())
    }

    pub fn transitions(&self) -> impl Iterator<Item = &Transition<TReducer>> {
        self.dynamics.iter().map(|(_, transition)| transition)
            .chain(self.shortcuts.iter())
            .chain(self.statics.values().flatten())
    }
}

impl<'a, TCheck, TReducer> Default for Node<'a, TCheck, TReducer> {
    fn default() -> Self {
        Self::new()
    }
}
