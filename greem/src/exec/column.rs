use crate::error::Error;
use crate::exec::scope::Frame;
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

/// One batch of completed items: the immediate completion (turn 0) or a
/// stream turn, with its own inner slots and child scopes.
pub struct Turn<'a> {
    /// List levels below the outermost one (`levels[0]` is list depth 1).
    pub levels: Vec<Vec<Slot>>,
    pub inner: Inner<'a>,
    pub children: Vec<Frame<'a>>,
    pub ranges: Vec<TurnRange>,
    /// Errors raised at slots of this turn; freed with it when it retires.
    pub errors: Vec<ErrorRecord>,
    pub shipped: bool,
    pub retired: bool,
}

impl<'a> Turn<'a> {
    /// Frees everything a shipped turn held: memory scales with in-flight
    /// work, not stream length.
    pub fn retire(&mut self) {
        self.children = Vec::new();
        self.levels = Vec::new();
        self.inner = match self.inner {
            Inner::Leaves(_) => Inner::Leaves(Vec::new()),
            Inner::Objects(_) => Inner::Objects(Vec::new()),
        };
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
            inner: if objects {
                Inner::Objects(Vec::new())
            } else {
                Inner::Leaves(Vec::new())
            },
            children: Vec::new(),
            ranges: Vec::new(),
            errors: Vec::new(),
            shipped: false,
            retired: false,
        }
    }
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

impl<'a> Column<'a> {
    pub fn depth(&self) -> usize {
        self.shape.levels.len()
    }

    pub fn record_error(
        &mut self,
        turn: usize,
        error: Error,
        object: u32,
        indices: Vec<u32>,
        generation: u32,
    ) -> ErrorId {
        let errors = &mut self.turns[turn].errors;
        let id = errors.len() as ErrorId;
        errors.push(ErrorRecord {
            error,
            object,
            indices,
            generation,
        });
        id
    }
}
