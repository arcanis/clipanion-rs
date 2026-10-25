use std::fmt::Debug;

use crate::{node::Node, shared::{is_terminal_node, ArgKey, CUSTOM_NODE_ID, ERROR_NODE_ID, INITIAL_NODE_ID, SUCCESS_NODE_ID}, transition::{offset_node_id, Transition}};

#[derive(Clone)]
pub struct Machine<'a, TCheck, TReducer> {
    pub contexts: Vec<usize>,
    pub nodes: Vec<Node<'a, TCheck, TReducer>>,
}

impl<'a, TCheck: Debug, TReducer: Debug> Debug for Machine<'a, TCheck, TReducer> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for (id, node) in self.nodes.iter().enumerate() {
            writeln!(f, "Node {} ({}):", id, node.context)?;

            if id == ERROR_NODE_ID {
                writeln!(f, "  [Error]")?;
            } else if id == SUCCESS_NODE_ID {
                writeln!(f, "  [Success]")?;
            }

            if let Some(options) = &node.options {
                writeln!(f, "  Options{}", if options.help {" (with help)"} else {""})?;
            }

            for (check, transition) in node.dynamics.iter() {
                writeln!(f, "  Dynamic: {:?} -> {:?} -> {}", check, transition.reducer, transition.to)?;
            }

            for transition in node.shortcuts.iter() {
                writeln!(f, "  Shortcut -> {}", transition.to)?;
            }

            for (segment, transitions) in node.statics.iter() {
                for transition in transitions.iter() {
                    writeln!(f, "  Static: {:?} -> {:?} -> {}", segment, transition.reducer, transition.to)?;
                }
            }
        }

        Ok(())
    }
}

impl<'a, TCheck, TReducer> Machine<'a, TCheck, TReducer> {
    pub fn new(context: usize) -> Self {
        let mut default = Self {
            contexts: vec![context],
            nodes: vec![],
        };

        for _ in 0..CUSTOM_NODE_ID {
            default.nodes.push(Node::new());
        }

        default
    }

    pub fn create_node(&mut self) -> usize {
        self.inject_node(Node::new())
    }

    pub fn inject_node(&mut self, node: Node<'a, TCheck, TReducer>) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    pub fn register_dynamic(&mut self, from: usize, check: TCheck, to: usize, reducer: TReducer) {
        self.nodes[from].dynamics.push((check, Transition::new(to, reducer)));
    }

    pub fn register_shortcut(&mut self, from: usize, to: usize) where TReducer: Default {
        self.nodes[from].shortcuts.push(Transition::new(to, Default::default()));
    }

    pub fn register_static(&mut self, from: usize, key: ArgKey<'a>, to: usize, reducer: TReducer) {
        self.nodes[from].statics.entry(key).or_default().push(Transition::new(to, reducer));
    }

    pub fn new_any_of<I>(machines: I) -> Self where TReducer: Default, I: IntoIterator<Item = Self> {
        let mut out = Machine {
            contexts: vec![],
            nodes: vec![],
        };

        for _ in 0..CUSTOM_NODE_ID {
            out.nodes.push(Node::new());
        }

        for machine in machines {
            let context_offset
                = out.contexts.len();

            let node_offset
                = out.nodes.len();

            out.contexts.extend(machine.contexts);
            out.register_shortcut(INITIAL_NODE_ID, node_offset);

            // The machines are moved in rather than cloned: building them is most of
            // what a program does before running its command.
            for (id, mut node) in machine.nodes.into_iter().enumerate() {
                if !is_terminal_node(id) {
                    for transition in node.transitions_mut() {
                        transition.to = offset_node_id(transition.to, node_offset);
                    }

                    node.context += context_offset;
                    out.nodes.push(node);
                }
            }
        }

        out
    }

    fn resolve_passthrough(&self, mut id: usize) -> usize {
        loop {
            let node
                = &self.nodes[id];

            if node.shortcuts.len() != 1 || !node.statics.is_empty() || !node.dynamics.is_empty() || node.options.is_some() {
                break;
            }

            id = node.shortcuts[0].to;
        }

        id
    }

    /// Skips the nodes that only lead to another one, and removes the nodes that
    /// can't be reached anymore.
    pub fn simplify_machine(&mut self) {
        let passthroughs
            = (0..self.nodes.len())
                .map(|id| self.resolve_passthrough(id))
                .collect::<Vec<_>>();

        let mut visited
            = vec![false; self.nodes.len()];

        let mut queue
            = vec![INITIAL_NODE_ID];

        while let Some(node_id) = queue.pop() {
            for transition in self.nodes[node_id].transitions_mut() {
                transition.to = passthroughs[transition.to];

                if !visited[transition.to] {
                    visited[transition.to] = true;
                    queue.push(transition.to);
                }
            }
        }

        let mut map_offset
            = vec![usize::MAX; self.nodes.len()];

        let mut out_offset
            = 0;

        for (in_offset, mapped) in map_offset.iter_mut().enumerate() {
            if in_offset < CUSTOM_NODE_ID || visited[in_offset] {
                *mapped = out_offset;
                out_offset += 1;
            }
        }

        let mut in_offset = 0;

        self.nodes.retain(|_| {
            in_offset += 1;
            map_offset[in_offset - 1] != usize::MAX
        });

        for node in self.nodes.iter_mut() {
            for transition in node.transitions_mut() {
                transition.to = map_offset[transition.to];
            }
        }
    }
}
