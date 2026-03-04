use std::sync::atomic::{AtomicUsize, Ordering};

static GLOBAL_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A globally-unique integer ID
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

/// A local string-based identifier
pub type LocalId = String;

/// A module identifier (canonical file path)
pub type ModuleId = String;

/// A tag: either local (named) or global (numbered)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Tag {
    Local(LocalId),
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
