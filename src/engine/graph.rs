use std::collections::HashMap
use idalib::func::FunctionId;

pub type RETask<In, Out> = fn( In ) -> Out
	where Out: std::future::Future;

pub struct RETaskNode {
	task: RETask<(), ()>,
	dependencies: HashSet<FunctionID>,
}

impl RETaskNode {
	pub async fn iter_dependencies( &self ) -> <HashSet<FunctionID> as IntoIterator>::IntoIter {
		self.dependencies.into_iter()
	}
}

pub struct RETaskGraph<R>
where R: std::future::Future,
{
	entry: FunctionId,
	functions: HashMap<FunctionId, RENode>
}

impl RETaskGraph {
	pub fn new( entry: FunctionId ) -> Self {
		Self {
			entry,
			functions: HashMap::new(),
		}
	}
}
