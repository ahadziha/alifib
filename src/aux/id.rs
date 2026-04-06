use std::sync::atomic::{AtomicUsize, Ordering};

static GLOBAL_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A globally-unique integer identifier, allocated from a process-wide atomic counter.
/// Used to identify cells and types in the global store across all modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlobalId(pub usize);

impl GlobalId {
    pub fn fresh() -> Self {
        Self(GLOBAL_COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

impl std::fmt::Display for GlobalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A user-visible string name, scoped within a single type or module complex.
pub type LocalId = String;

/// A module identifier (typically the canonical file path of the source file).
pub type ModuleId = String;

/// A tag: the identity of a cell, either as a local name or a global ID.
///
/// `Local` tags appear during type elaboration and are scoped to the enclosing
/// type or module complex.  `Global` tags refer to finalized cells committed to
/// the global store.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Tag {
    /// A local name, scoped to the enclosing type or module complex.
    Local(LocalId),
    /// A globally unique ID, referring to a cell in the global store.
    Global(GlobalId),
}

impl Tag {
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }
}

impl PartialOrd for Tag {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tag {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Local(a), Self::Local(b)) => a.cmp(b),
            (Self::Global(a), Self::Global(b)) => a.cmp(b),
            (Self::Local(_), Self::Global(_)) => std::cmp::Ordering::Less,
            (Self::Global(_), Self::Local(_)) => std::cmp::Ordering::Greater,
        }
    }
}

impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(name) => write!(f, "{}", name),
            Self::Global(id) => write!(f, "{}", id),
        }
    }
}
