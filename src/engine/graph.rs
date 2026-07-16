// Experimental task graph — remote TUI code, needs migration.
// This module is behind the `tui` feature and is not part of the M1 plan.

use std::collections::{HashMap, HashSet};

pub type RETask<In, Out> = fn(In) -> Out
where
	Out: std::future::Future;

pub struct RETaskNode {
	_task: RETask<(), ()>,
	_dependencies: HashSet<u64>,
}

pub struct RETaskGraph {
	_entry: Option<u64>,
	_functions: HashMap<u64, RETaskNode>,
}

impl RETaskGraph {
	pub fn new() -> Self {
		Self {
			_entry: None,
			_functions: HashMap::new(),
		}
	}
}
