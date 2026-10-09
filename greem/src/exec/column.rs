use crate::error::Error;
use crate::exec::scope::{Frame, Scope};
use crate::exec::stream::StreamDriver;
use crate::resolver::Shape;
use crate::tree::FieldKind;
use crate::value::Value;

pub type ErrorId = u32;

/// One list-level entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Items {
        start: u32,
        len: u32,
    },
    Null,
    Error(ErrorId),
    /// Nulled by propagation; keeps the range so errors beneath stay reachable.
    Propagated {
        start: u32,
        len: u32,
    },
    Pending,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Leaf<'a> {
    Value(Value<'a>),
    Null,
    Error(ErrorId),
    Propagated,
    Pending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjSlot {
    Object {
        child: u32,
        index: u32,
    },
    Null,
    Error(ErrorId),
    /// Nulled by propagation; keeps the link so errors beneath stay reachable.
    Propagated {
        child: u32,
        index: u32,
    },
    Pending,
}

pub enum Inner<'a> {
    Leaves(Vec<Leaf<'a>>),
    Objects(Vec<ObjSlot>),
}

impl<'a> Inner<'a> {
    pub fn len(&self) -> usize {
        match self {
            Inner::Leaves(v) => v.len(),
            Inner::Objects(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn push_pending(&mut self) -> u32 {
        let index = self.len() as u32;
        match self {
            Inner::Leaves(v) => v.push(Leaf::Pending),
            Inner::Objects(v) => v.push(ObjSlot::Pending),
        }
        index
    }

    pub fn set_null(&mut self, slot: u32) {
        match self {
            Inner::Leaves(v) => v[slot as usize] = Leaf::Null,
            Inner::Objects(v) => v[slot as usize] = ObjSlot::Null,
        }
    }

    pub fn set_error(&mut self, slot: u32, id: ErrorId) {
        match self {
            Inner::Leaves(v) => v[slot as usize] = Leaf::Error(id),
            Inner::Objects(v) => v[slot as usize] = ObjSlot::Error(id),
        }
    }

    pub fn set_propagated(&mut self, slot: u32) {
        match self {
            Inner::Leaves(v) => v[slot as usize] = Leaf::Propagated,
            Inner::Objects(v) => {
                if let ObjSlot::Object { child, index } = v[slot as usize] {
                    v[slot as usize] = ObjSlot::Propagated { child, index };
                }
            }
        }
    }

    pub fn is_null_like(&self, slot: u32) -> bool {
        match self {
            Inner::Leaves(v) => !matches!(v[slot as usize], Leaf::Value(_)),
            Inner::Objects(v) => !matches!(v[slot as usize], ObjSlot::Object { .. }),
        }
    }
}

/// The range of one parent object's items inside a stream turn.
#[derive(Clone, Copy, Debug)]
pub struct TurnRange {
    pub object: u32,
    pub start_index: u32,
    pub start_slot: u32,
    pub len: u32,
    /// This parent's items of the turn have shipped in an incremental entry.
    pub shipped: bool,
}

/// What a turn's values and child frames borrow besides the frame: for a
/// stream turn, the items it completed and the context it completed them in.
pub trait TurnBatch: Send {
    /// Completes the turn's items, writing list levels and errors to the turn.
    fn complete<'this>(
        &'this self,
        levels: &mut [Vec<Slot>],
        errors: &mut Vec<ErrorRecord>,
    ) -> Stored<'this>;
}

/// The innermost slots and child frames of one turn: everything in it that
/// borrows objects or outputs.
pub struct Stored<'a> {
    pub inner: Inner<'a>,
    pub children: Vec<Frame<'a>>,
}

impl Stored<'_> {
    pub fn new(objects: bool) -> Self {
        Self {
            inner: if objects {
                Inner::Objects(Vec::new())
            } else {
                Inner::Leaves(Vec::new())
            },
            children: Vec::new(),
        }
    }
}

self_cell::self_cell!(
    pub struct StreamCell<'a> {
        owner: Box<dyn TurnBatch + 'a>,
        #[not_covariant]
        dependent: Stored,
    }
);

pub enum Storage<'a> {
    /// Turn 0 lives as long as its scope, so it borrows from the frame. A
    /// retired turn keeps empty storage of this kind.
    Immediate(Stored<'a>),
    /// A stream turn owns what it borrows and drops it when it retires.
    Stream(StreamCell<'a>),
}

/// One batch of completed items: the immediate completion (turn 0) or a
/// stream turn, with its own slots and child scopes.
pub struct Turn<'a> {
    /// List levels below the outermost one (`levels[0]` is list depth 1).
    pub levels: Vec<Vec<Slot>>,
    pub stored: Storage<'a>,
    pub ranges: Vec<TurnRange>,
    /// Errors raised at slots of this turn; freed with it when it retires.
    pub errors: Vec<ErrorRecord>,
    pub shipped: bool,
    pub retired: bool,
}

impl<'a> Turn<'a> {
    #[inline(always)]
    pub fn with_stored<R>(&self, f: impl for<'q> FnOnce(&Stored<'q>) -> R) -> R {
        match &self.stored {
            Storage::Immediate(stored) => f(stored),
            Storage::Stream(cell) => stream_stored(cell, f),
        }
    }

    #[inline(always)]
    pub fn with_stored_mut<R>(&mut self, f: impl for<'q> FnOnce(&mut Stored<'q>) -> R) -> R {
        match &mut self.stored {
            Storage::Immediate(stored) => f(stored),
            Storage::Stream(cell) => stream_stored_mut(cell, f),
        }
    }

    pub fn with_child<R>(&self, child: u32, f: impl FnOnce(&Scope<'_>) -> R) -> R {
        self.with_stored(|stored| {
            stored.children[child as usize].with_dependent(|_, scope| f(scope))
        })
    }

    pub fn with_child_mut<R>(&mut self, child: u32, f: impl FnOnce(&mut Scope<'_>) -> R) -> R {
        self.with_stored_mut(|stored| {
            stored.children[child as usize].with_dependent_mut(|_, scope| f(scope))
        })
    }

    /// Whether `f` holds for any of this turn's child scopes.
    pub fn any_child(&self, mut f: impl FnMut(&Scope<'_>) -> bool) -> bool {
        self.with_stored(|stored| {
            stored
                .children
                .iter()
                .any(|child| child.with_dependent(|_, scope| f(scope)))
        })
    }

    pub fn each_child_mut(&mut self, mut f: impl FnMut(&mut Scope<'_>)) {
        self.with_stored_mut(|stored| {
            for child in &mut stored.children {
                child.with_dependent_mut(|_, scope| f(scope));
            }
        });
    }

    pub fn objects(&self) -> bool {
        self.with_stored(|stored| matches!(stored.inner, Inner::Objects(_)))
    }

    /// Frees everything a shipped turn held: memory scales with in-flight
    /// work, not stream length.
    pub fn retire(&mut self) {
        self.stored = Storage::Immediate(Stored::new(self.objects()));
        self.levels = Vec::new();
        self.ranges = Vec::new();
        self.errors = Vec::new();
        self.retired = true;
    }

    /// Reuses a retired turn's slot for a new batch, so the turn list is
    /// bounded by the number of turns in flight rather than the stream length.
    pub fn reset(&mut self, depth: usize, objects: bool) {
        debug_assert!(self.retired);
        *self = Turn::new(depth, objects);
    }

    pub fn new(depth: usize, objects: bool) -> Self {
        Self {
            levels: vec![Vec::new(); depth.saturating_sub(1)],
            stored: Storage::Immediate(Stored::new(objects)),
            ranges: Vec::new(),
            errors: Vec::new(),
            shipped: false,
            retired: false,
        }
    }
}

// Out of line, so readers inline only turn 0's direct access.
#[inline(never)]
fn stream_stored<R>(cell: &StreamCell<'_>, f: impl for<'q> FnOnce(&Stored<'q>) -> R) -> R {
    cell.with_dependent(|_, stored| f(stored))
}

#[inline(never)]
fn stream_stored_mut<R>(
    cell: &mut StreamCell<'_>,
    f: impl for<'q> FnOnce(&mut Stored<'q>) -> R,
) -> R {
    cell.with_dependent_mut(|_, stored| f(stored))
}

pub struct ErrorRecord {
    pub error: Error,
    pub object: u32,
    pub indices: Vec<u32>,
    pub generation: u32,
}

pub struct Column<'a> {
    pub field: u32,
    pub kind: FieldKind,
    pub shape: Shape,
    /// The outermost list level, one entry per object; unused at depth 0.
    pub level0: Vec<Slot>,
    pub turns: Vec<Turn<'a>>,
    pub stream: Option<Box<dyn StreamDriver<'a> + 'a>>,
    pub introspection: Option<Value<'static>>,
}

impl Column<'_> {
    pub fn depth(&self) -> usize {
        self.shape.levels.len()
    }
}
