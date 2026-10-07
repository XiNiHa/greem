//! Payloads: the borrowed views the sink receives at each barrier, and their
//! serialization straight from the columns.

use crate::error::{GraphQLError, PathSegment};
use crate::exec::column::{Column, Inner, Leaf, ObjSlot, Slot};
use crate::exec::scope::Scope;
use crate::tree::FieldKind;
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

/// A step from one scope to a nested one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Child { field: u32, turn: u32, child: u32 },
    Deferred(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadKind {
    /// Request-level failure: `errors` and no `data`.
    RequestError,
    /// The first (or only) response: `data`, optionally `pending` and `hasNext`.
    Initial,
    /// A subsequent incremental result.
    Subsequent,
}

#[derive(Clone, Debug, Serialize)]
pub struct PendingEntry {
    pub id: String,
    pub path: Vec<PathSegment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CompletedEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<GraphQLError>,
}

pub(crate) enum EntrySource {
    Defer {
        path: Vec<Step>,
        object: u32,
    },
    Items {
        path: Vec<Step>,
        field: u32,
        turn: u32,
        start_slot: u32,
        len: u32,
    },
}

pub struct IncrementalEntry {
    pub(crate) id: String,
    /// Length of the entry's full response path; entries in one payload are
    /// ordered by it so a parent's items precede work delivered beneath them.
    pub(crate) depth: usize,
    pub(crate) sub_path: Vec<PathSegment>,
    pub(crate) errors: Vec<GraphQLError>,
    pub(crate) source: EntrySource,
}

pub(crate) enum Data {
    Absent,
    Null,
    Root,
}

/// Object-safe access to the root scope, so a payload needs one lifetime.
pub(crate) trait ErasedRoot {
    fn with_scope(&self, f: &mut dyn for<'s, 'a> FnMut(&'s Scope<'a>));
}

impl ErasedRoot for Scope<'_> {
    fn with_scope(&self, f: &mut dyn for<'s, 'a> FnMut(&'s Scope<'a>)) {
        f(self)
    }
}

fn with_root<R>(root: &dyn ErasedRoot, f: impl for<'s, 'a> FnOnce(&'s Scope<'a>) -> R) -> R {
    let mut f = Some(f);
    let mut result = None;
    root.with_scope(&mut |scope| {
        if let Some(f) = f.take() {
            result = Some(f(scope));
        }
    });
    result.expect("root visited")
}

/// One response payload, borrowing the executor's retained frames. It is only
/// valid inside the sink callback; serialize it there.
pub struct Payload<'p> {
    pub(crate) root: Option<&'p (dyn ErasedRoot + 'p)>,
    pub(crate) kind: PayloadKind,
    pub(crate) data: Data,
    pub(crate) errors: Vec<GraphQLError>,
    pub(crate) pending: Vec<PendingEntry>,
    pub(crate) incremental: Vec<IncrementalEntry>,
    pub(crate) completed: Vec<CompletedEntry>,
    pub(crate) has_next: Option<bool>,
}

impl<'p> Payload<'p> {
    /// The root object failed as a whole: `data: null` and its error at the root position.
    pub(crate) fn failed_root(error: GraphQLError) -> Self {
        Payload {
            root: None,
            kind: PayloadKind::Initial,
            data: Data::Null,
            errors: vec![error],
            pending: Vec::new(),
            incremental: Vec::new(),
            completed: Vec::new(),
            has_next: None,
        }
    }

    pub(crate) fn request_error(errors: Vec<GraphQLError>) -> Self {
        Payload {
            root: None,
            kind: PayloadKind::RequestError,
            data: Data::Absent,
            errors,
            pending: Vec::new(),
            incremental: Vec::new(),
            completed: Vec::new(),
            has_next: None,
        }
    }

    pub fn kind(&self) -> PayloadKind {
        self.kind
    }

    /// Whether more payloads follow (`hasNext`); `None` for a plain response.
    pub fn has_next(&self) -> Option<bool> {
        self.has_next
    }

    pub fn errors(&self) -> &[GraphQLError] {
        &self.errors
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.kind, PayloadKind::Subsequent)
            && self.incremental.is_empty()
            && self.completed.is_empty()
            && self.pending.is_empty()
    }
}

impl Serialize for Payload<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        match self.kind {
            PayloadKind::RequestError => {
                map.serialize_entry("errors", &self.errors)?;
            }
            PayloadKind::Initial => {
                match self.data {
                    Data::Absent => {}
                    Data::Null => map.serialize_entry("data", &())?,
                    Data::Root => with_root(self.root.expect("root"), |scope| {
                        map.serialize_entry("data", &ObjectView(scope, 0))
                    })?,
                }
                if !self.errors.is_empty() {
                    map.serialize_entry("errors", &self.errors)?;
                }
                if !self.pending.is_empty() {
                    map.serialize_entry("pending", &self.pending)?;
                }
                if let Some(has_next) = self.has_next {
                    map.serialize_entry("hasNext", &has_next)?;
                }
            }
            PayloadKind::Subsequent => {
                if !self.pending.is_empty() {
                    map.serialize_entry("pending", &self.pending)?;
                }
                if !self.incremental.is_empty() {
                    map.serialize_entry("incremental", &IncrementalList(self))?;
                }
                if !self.completed.is_empty() {
                    map.serialize_entry("completed", &self.completed)?;
                }
                map.serialize_entry("hasNext", &self.has_next.unwrap_or(false))?;
            }
        }
        map.end()
    }
}

struct IncrementalList<'s, 'p>(&'s Payload<'p>);

impl Serialize for IncrementalList<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.incremental.len()))?;
        for entry in &self.0.incremental {
            with_root(self.0.root.expect("root"), |scope| {
                seq.serialize_element(&EntryView(scope, entry))
            })?;
        }
        seq.end()
    }
}

struct EntryView<'s, 'p>(&'s Scope<'p>, &'s IncrementalEntry);

impl Serialize for EntryView<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entry = self.1;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &entry.id)?;
        if !entry.sub_path.is_empty() {
            map.serialize_entry("subPath", &entry.sub_path)?;
        }
        match &entry.source {
            EntrySource::Defer { path, object } => {
                let object = *object;
                with_scope_at(self.0, path, &mut |scope| {
                    map.serialize_entry("data", &ObjectView(scope, object))
                })?;
            }
            EntrySource::Items {
                path,
                field,
                turn,
                start_slot,
                len,
            } => {
                let (field, turn, start, len) = (*field, *turn, *start_slot, *len);
                with_scope_at(self.0, path, &mut |scope| {
                    let column = scope.column(field).expect("streamed column");
                    map.serialize_entry(
                        "items",
                        &RangeView {
                            column,
                            turn: turn as usize,
                            level: 1,
                            start,
                            len,
                        },
                    )
                })?;
            }
        }
        if !entry.errors.is_empty() {
            map.serialize_entry("errors", &entry.errors)?;
        }
        map.end()
    }
}

pub(crate) fn with_scope_at<R>(
    scope: &Scope<'_>,
    path: &[Step],
    f: &mut dyn for<'s, 'a> FnMut(&'s Scope<'a>) -> R,
) -> R {
    match path.split_first() {
        None => f(scope),
        Some((Step::Child { field, turn, child }, rest)) => {
            let column = scope.column(*field).expect("column on path");
            column.turns[*turn as usize].children[*child as usize]
                .with_dependent(|_, inner| with_scope_at(inner, rest, f))
        }
        Some((Step::Deferred(index), rest)) => {
            let inner = scope.deferred[*index as usize]
                .scope()
                .expect("running deferred set on path");
            with_scope_at(inner, rest, f)
        }
    }
}

/// One object of a scope, serialized with the scope's field set.
pub(crate) struct ObjectView<'s, 'a>(pub &'s Scope<'a>, pub u32);

impl Serialize for ObjectView<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let scope = self.0;
        let header = scope.shared.table.header(scope.meta.entry);
        let fields = &header.sets[scope.set].1;
        let mut map = serializer.serialize_map(Some(fields.len()))?;
        for &field in fields {
            let key = &header.fields[field as usize].key;
            match scope.column(field) {
                Some(column) => map.serialize_entry(key, &FieldView(scope, column, self.1))?,
                None => map.serialize_entry(key, &())?,
            }
        }
        map.end()
    }
}

struct FieldView<'s, 'a>(&'s Scope<'a>, &'s Column<'a>, u32);

impl Serialize for FieldView<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (scope, column, object) = (self.0, self.1, self.2);
        match column.kind {
            FieldKind::Typename => {
                return serializer
                    .serialize_str(scope.shared.table.header(scope.meta.entry).typename);
            }
            FieldKind::Introspection => {
                return match &column.introspection {
                    Some(value) => value.serialize(serializer),
                    None => serializer.serialize_unit(),
                };
            }
            FieldKind::Normal => {}
        }
        if column.depth() == 0 {
            return InnerView {
                column,
                turn: 0,
                slot: object,
            }
            .serialize(serializer);
        }
        match column.level0[object as usize] {
            Slot::Items { start, len } => RangeView {
                column,
                turn: 0,
                level: 1,
                start,
                len,
            }
            .serialize(serializer),
            _ => serializer.serialize_unit(),
        }
    }
}

struct RangeView<'s, 'a> {
    column: &'s Column<'a>,
    turn: usize,
    level: usize,
    start: u32,
    len: u32,
}

impl Serialize for RangeView<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.len as usize))?;
        for slot in self.start..self.start + self.len {
            seq.serialize_element(&LevelView {
                column: self.column,
                turn: self.turn,
                level: self.level,
                slot,
            })?;
        }
        seq.end()
    }
}

struct LevelView<'s, 'a> {
    column: &'s Column<'a>,
    turn: usize,
    level: usize,
    slot: u32,
}

impl Serialize for LevelView<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.level == self.column.depth() {
            return InnerView {
                column: self.column,
                turn: self.turn,
                slot: self.slot,
            }
            .serialize(serializer);
        }
        match self.column.turns[self.turn].levels[self.level - 1][self.slot as usize] {
            Slot::Items { start, len } => RangeView {
                column: self.column,
                turn: self.turn,
                level: self.level + 1,
                start,
                len,
            }
            .serialize(serializer),
            _ => serializer.serialize_unit(),
        }
    }
}

struct InnerView<'s, 'a> {
    column: &'s Column<'a>,
    turn: usize,
    slot: u32,
}

impl Serialize for InnerView<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let turn = &self.column.turns[self.turn];
        match &turn.inner {
            Inner::Leaves(leaves) => match &leaves[self.slot as usize] {
                Leaf::Value(value) => value.serialize(serializer),
                _ => serializer.serialize_unit(),
            },
            Inner::Objects(objects) => match objects[self.slot as usize] {
                ObjSlot::Object { child, index } => turn.children[child as usize]
                    .with_dependent(|_, scope| ObjectView(scope, index).serialize(serializer)),
                _ => serializer.serialize_unit(),
            },
        }
    }
}
