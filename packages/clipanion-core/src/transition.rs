use crate::shared::{is_terminal_node, CUSTOM_NODE_ID};

#[derive(Debug, Default, Clone)]
pub struct Transition<TReducer> {
    pub to: usize,
    pub reducer: TReducer,
}

impl<TReducer> Transition<TReducer> {
    pub fn new(to: usize, reducer: TReducer) -> Transition<TReducer> {
        Transition {
            to,
            reducer,
        }
    }
}

/// Where a node of a machine lands once the machine is merged into another one
/// at `offset` (see `Machine::new_any_of`): the terminal nodes are shared, the
/// other ones follow each other.
pub fn offset_node_id(id: usize, offset: usize) -> usize {
    if is_terminal_node(id) {
        id
    } else if id >= CUSTOM_NODE_ID {
        id + offset - CUSTOM_NODE_ID + 1
    } else {
        id + offset
    }
}
