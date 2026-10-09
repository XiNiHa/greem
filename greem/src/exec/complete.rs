//! Completion: turning resolver outputs into columns, child scopes and
//! stream drivers. `Completes` is the sealed, tag-side trait every generated
//! completion impl implements; `Outputs` is its user-facing bridge.

use crate::context::{Context, HintAddr};
use crate::error::{Error, GraphQLError};
use crate::exec::column::{Column, ErrorId, ErrorRecord, Inner, Slot, Storage, Stored, Turn};
use crate::exec::list::ListOutput;
use crate::exec::scope::{
    Batch, DeferredSet, DeferredSetState, FieldFuture, Frame, ObjectMeta, ParentLink, Scope,
    ScopeMeta,
};
use crate::exec::state::{GroupId, GroupKind, Shared};
use crate::exec::stream::{StreamDriver, StreamState};
use crate::plan::{Leaf as LeafPath, PlanHeader, PlanId, PlanTable, Walker};
use crate::resolver::{
    Args, As, Either, Field, List, Nullable, Outputs, Resolver, Shape, Streamed,
};
use crate::tree::{Abort, FieldKind, NodeId};
use crate::value::{ToLeaf, Value};
use futures::Stream;
use futures::StreamExt;
use smallvec::SmallVec;
use std::borrow::Borrow;
use std::marker::PhantomData;
#[cfg(feature = "reference-executor")]
use {
    crate::exec::reference::{RefCompletion, RefValue},
    futures::future::BoxFuture,
};

/// A position being completed: a slot at the current level, the parent
/// object it belongs to, and the list indices below that object.
#[derive(Clone, Debug)]
pub struct Pos {
    pub slot: u32,
    pub object: u32,
    pub indices: SmallVec<[u32; 4]>,
}

/// Everything a scope's field futures share.
pub struct FieldsCx<'a, C> {
    pub shared: &'a Shared,
    pub app: &'a C,
    pub table: &'a PlanTable,
    pub header: &'a PlanHeader,
    pub entry: PlanId,
    pub meta: &'a ScopeMeta<'a>,
    pub contexts: &'a [Context<'a, C>],
    pub groups: Vec<GroupId>,
}

impl<C> Clone for FieldsCx<'_, C> {
    fn clone(&self) -> Self {
        FieldsCx {
            shared: self.shared,
            app: self.app,
            table: self.table,
            header: self.header,
            entry: self.entry,
            meta: self.meta,
            contexts: self.contexts,
            groups: self.groups.clone(),
        }
    }
}

/// Executor-facing completion, implemented by every generated type tag and
/// by greem's own scalar, list, nullable and abstract wrappers.
pub trait Completes<T, C>: Sized {
    const TYPENAME: &'static str = "";

    /// Visits every Plan entry this output type can produce under `node`.
    fn walk<'a>(w: &mut Walker<'_, C>, node: NodeId, leaf: &LeafPath) -> Result<(), Abort>
    where
        T: 'a,
        C: 'a;

    /// Object tags only: the error of a value that failed as a whole.
    fn parent_error(_value: &T) -> Option<&Error> {
        None
    }

    /// Leaf tags only: `value` completes to null. Fine beneath `Nullable`,
    /// an error anywhere else.
    fn null_leaf(_value: &T) -> bool {
        false
    }

    /// The first error completing `value` records at or beneath its own
    /// position before any resolver runs (a failed object, a leaf its scalar
    /// cannot represent) that `wanted` accepts, given the list indices from
    /// `value` to the error; those indices are left on `indices`. A stream
    /// stops pulling at an item with an error that ends it.
    fn first_error(
        value: &T,
        indices: &mut Vec<u32>,
        wanted: &dyn Fn(&[u32]) -> bool,
    ) -> Option<Error> {
        Self::parent_error(value)
            .filter(|_| wanted(indices))
            .cloned()
    }

    /// Completes a set of outputs at their positions.
    fn complete<'a>(values: Vec<T>, positions: Vec<Pos>, cc: &mut Completion<'a, '_, C>)
    where
        T: 'a,
        C: 'a;

    /// The reference executor's depth-first completion of one value.
    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(value: T, rc: &RefCompletion<'s, C>) -> BoxFuture<'v, RefValue>
    where
        T: 'v,
        C: 'v;

    /// Object tags only: the field futures of one delivery set over a parent set.
    fn start_fields<'a>(
        _cx: &FieldsCx<'a, C>,
        _parents: &[&'a T],
        _set: usize,
    ) -> Vec<FieldFuture<'a>>
    where
        T: 'a,
        C: 'a,
    {
        unreachable!("start_fields is only implemented by object type tags")
    }
}

/// The completion context handed to `Completes::complete`: the parts of one
/// turn that completion writes.
pub struct Completion<'a, 'c, C> {
    pub(crate) cx: &'c FieldsCx<'a, C>,
    pub(crate) field: u32,
    pub(crate) shape: Shape,
    /// The outermost list level, one slot per object; only turn 0 writes it.
    pub(crate) level0: &'c mut [Slot],
    pub(crate) levels: &'c mut [Vec<Slot>],
    pub(crate) errors: &'c mut Vec<ErrorRecord>,
    pub(crate) stored: &'c mut Stored<'a>,
    /// Where a streamed output registers its driver; only turn 0 has one.
    pub(crate) stream: Option<&'c mut Option<Box<dyn StreamDriver<'a> + 'a>>>,
    pub(crate) level: usize,
    pub(crate) leaf: LeafPath,
    pub(crate) generation: u32,
}

impl<'a, 'c, C> Completion<'a, 'c, C> {
    /// Completes into `column`'s turn 0, from its outermost level.
    pub(crate) fn immediate(
        cx: &'c FieldsCx<'a, C>,
        field: u32,
        column: &'c mut Column<'a>,
        leaf: LeafPath,
        generation: u32,
    ) -> Self {
        let Column {
            shape,
            level0,
            turns,
            stream,
            ..
        } = column;
        let turn = &mut turns[0];
        let Storage::Immediate(stored) = &mut turn.stored else {
            unreachable!("turn 0 is never a stream turn")
        };
        Completion {
            cx,
            field,
            shape: *shape,
            level0,
            levels: &mut turn.levels,
            errors: &mut turn.errors,
            stored,
            stream: Some(stream),
            level: 0,
            leaf,
            generation,
        }
    }
}

impl<'a, C> Completion<'a, '_, C> {
    pub(crate) fn depth(&self) -> usize {
        self.shape.levels.len()
    }

    fn set(&mut self, pos: &Pos, slot: Slot) {
        if self.level == 0 {
            self.level0[pos.slot as usize] = slot;
        } else {
            self.levels[self.level - 1][pos.slot as usize] = slot;
        }
    }

    pub fn null(&mut self, pos: &Pos) {
        if self.level == self.depth() {
            self.stored.inner.set_null(pos.slot);
        } else {
            self.set(pos, Slot::Null);
        }
    }

    pub fn error(&mut self, pos: &Pos, error: Error) {
        let group = self.cx.groups[pos.object as usize];
        self.cx.shared.halt(group, || {
            let field = &self.cx.header.fields[self.field as usize];
            let path = self.cx.meta.path_to(pos.object, &field.key, &pos.indices);
            GraphQLError::from_error(&error, field.spans.clone(), path)
        });
        let id = self.errors.len() as ErrorId;
        self.errors.push(ErrorRecord {
            error,
            object: pos.object,
            indices: pos.indices.to_vec(),
            generation: self.generation,
        });
        if self.level == self.depth() {
            self.stored.inner.set_error(pos.slot, id);
        } else {
            self.set(pos, Slot::Error(id));
        }
    }

    pub fn leaf(&mut self, pos: &Pos, value: Result<Value<'a>, Error>) {
        match value {
            Ok(Value::Null) if !self.shape.nullable_at(self.level) => {
                self.error(pos, crate::value::null_at_non_null())
            }
            Ok(value) => match &mut self.stored.inner {
                Inner::Leaves(leaves) => {
                    leaves[pos.slot as usize] = crate::exec::column::Leaf::Value(value)
                }
                Inner::Objects(_) => unreachable!("leaf written to an object column"),
            },
            Err(error) => self.error(pos, error),
        }
    }

    /// Writes a list of `len` items at `pos` and returns the item positions one
    /// level down.
    pub fn list(&mut self, pos: &Pos, len: usize) -> impl Iterator<Item = Pos> + use<C> {
        let depth = self.depth();
        debug_assert!(self.level < depth, "list written at a non-list level");
        let start = if self.level + 1 == depth {
            let start = self.stored.inner.len() as u32;
            for _ in 0..len {
                self.stored.inner.push_pending();
            }
            start
        } else {
            let level = &mut self.levels[self.level];
            let start = level.len() as u32;
            level.extend(std::iter::repeat_n(Slot::Pending, len));
            start
        };
        self.set(
            pos,
            Slot::Items {
                start,
                len: len as u32,
            },
        );
        let (object, base) = (pos.object, pos.indices.clone());
        (0..len as u32).map(move |i| {
            let mut indices = base.clone();
            indices.push(i);
            Pos {
                slot: start + i,
                object,
                indices,
            }
        })
    }

    pub fn descend<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.level += 1;
        let result = f(self);
        self.level -= 1;
        result
    }

    pub fn with_leaf<R>(&mut self, step: u8, f: impl FnOnce(&mut Self) -> R) -> R {
        let next = self.leaf.push(step);
        let saved = std::mem::replace(&mut self.leaf, next);
        let result = f(self);
        self.leaf = saved;
        result
    }

    pub(crate) fn streamed(&self) -> bool {
        self.cx.header.fields[self.field as usize].stream.is_some()
    }

    pub(crate) fn stream_inner_unsupported(&mut self, positions: &[Pos]) {
        for pos in positions {
            self.error(
                pos,
                Error::framework(
                    "streamed lists are only supported at the outermost list level",
                    "STREAM_NESTED",
                ),
            );
        }
    }
}

impl<'a, C: Send + Sync + 'a> Completion<'a, '_, C> {
    /// Creates the child scope for a set of object outputs at their positions.
    pub fn object_scope<T: Outputs<Ty, C> + Send + Sync + 'a, Ty: 'static>(
        &mut self,
        values: Vec<T>,
        positions: Vec<Pos>,
    ) {
        if values.is_empty() {
            return;
        }
        let field = &self.cx.header.fields[self.field as usize];
        let node = field.child.expect("composite field has a child node");
        let entry = self
            .cx
            .table
            .lookup(node, &self.leaf)
            .expect("plan entry exists for every reachable (node, leaf)");
        let child_header = self.cx.table.header(entry);
        let mut objects = Vec::with_capacity(values.len());
        {
            let mut groups = self.cx.shared.groups();
            for pos in &positions {
                let parent = &self.cx.meta.objects[pos.object as usize];
                let mut pending = parent.pending.clone();
                let group = self.cx.groups[pos.object as usize];
                for &usage in &child_header.introduced {
                    let usage_def = &self.cx.table.usages[usage as usize];
                    // A nested fragment ships after whichever is deeper: the
                    // group delivering this object, or its parent fragment's group.
                    // The group delivering this object creates the position; the
                    // enclosing fragment's group must ship first too. Whichever is
                    // deeper is the parent; unrelated ones become an `after` dependency.
                    let outer = usage_def
                        .parent
                        .and_then(|p| pending.iter().find(|(u, _)| *u == p).map(|(_, g)| *g));
                    let (parent_group, after) = match outer {
                        Some(outer) if groups.is_ancestor(outer, group) => (group, None),
                        Some(outer) if groups.is_ancestor(group, outer) => (outer, None),
                        Some(outer) => (group, Some(outer)),
                        None => (group, None),
                    };
                    let g = groups.alloc(
                        GroupKind::Defer {
                            usage,
                            label: usage_def.label.clone(),
                            path: self.cx.meta.path_to(pos.object, &field.key, &pos.indices),
                            after,
                        },
                        parent_group,
                    );
                    pending.push((usage, g));
                }
                groups.retain(group);
                for &(_, g) in &pending {
                    groups.retain(g);
                }
                let shared = groups.alloc_shared(&child_header.sets, &pending, group);
                objects.push(ObjectMeta {
                    group,
                    parent: pos.object,
                    indices: pos.indices.clone(),
                    pending,
                    shared,
                });
            }
        }
        let contexts = (0..child_header.fields.len())
            .map(|i| {
                Context::new(
                    self.cx.app,
                    Some(HintAddr {
                        table: self.cx.table,
                        entry,
                        field: i as u32,
                    }),
                )
            })
            .collect();
        let batch: ObjectBatch<'a, T, Ty, C> = ObjectBatch {
            values,
            contexts,
            meta: ScopeMeta {
                entry,
                generation: self.generation + 1,
                objects,
                serial: false,
                parent: Some(ParentLink {
                    meta: self.cx.meta,
                    key: &field.key,
                }),
            },
            shared: self.cx.shared,
            app: self.cx.app,
            table: self.cx.table,
            header: self.cx.table.header(entry),
            entry,
            tag: PhantomData,
        };
        let child = self.stored.children.len() as u32;
        self.stored
            .children
            .push(Frame::from_batch(Box::new(batch)));
        match &mut self.stored.inner {
            Inner::Objects(slots) => {
                for (index, pos) in positions.iter().enumerate() {
                    slots[pos.slot as usize] = crate::exec::column::ObjSlot::Object {
                        child,
                        index: index as u32,
                    };
                }
            }
            Inner::Leaves(_) => unreachable!("object written to a leaf column"),
        }
    }

    /// Registers the stream driver for a streamed (or lazily drained) list field.
    pub(crate) fn stream<T, Ty, S>(
        &mut self,
        sources: Vec<Streamed<S, Result<T, Error>>>,
        positions: Vec<Pos>,
    ) where
        S: Stream<Item = Result<T, Error>> + Send + 'a,
        T: Outputs<Ty, C> + Send + 'a,
        Ty: 'static,
    {
        debug_assert_eq!(self.level, 0);
        let field = &self.cx.header.fields[self.field as usize];
        let streamed = field.stream.clone();
        let groups: Vec<GroupId> = {
            let mut table = self.cx.shared.groups();
            positions
                .iter()
                .map(|pos| match &streamed {
                    Some(info) => table.alloc(
                        GroupKind::Stream {
                            node: field.child.unwrap_or(0),
                            label: info.label.clone(),
                            path: self.cx.meta.path_to(pos.object, &field.key, &[]),
                        },
                        self.cx.groups[pos.object as usize],
                    ),
                    None => self.cx.groups[pos.object as usize],
                })
                .collect()
        };
        let state: StreamState<'a, T, Ty, C> = StreamState::new(
            sources.into_iter().map(|s| s.0.boxed()).collect(),
            positions,
            groups,
            streamed.map(|s| s.initial_count),
            self.cx.clone(),
            self.field,
            self.leaf.clone(),
            self.generation,
            self.cx.shared.capacity,
            self.shape,
        );
        self.cx
            .shared
            .has_streams
            .store(true, std::sync::atomic::Ordering::Relaxed);
        **self
            .stream
            .as_mut()
            .expect("only turn 0 registers a stream") = Some(Box::new(state));
    }
}

/// The owner of an object scope: the completed outputs plus the per-field
/// context views its resolvers borrow.
pub struct ObjectBatch<'a, T, Ty, C> {
    pub values: Vec<T>,
    pub contexts: Vec<Context<'a, C>>,
    pub meta: ScopeMeta<'a>,
    pub shared: &'a Shared,
    pub app: &'a C,
    pub table: &'a PlanTable,
    pub header: &'a PlanHeader,
    pub entry: PlanId,
    pub tag: PhantomData<fn() -> Ty>,
}

impl<T, Ty, C> Drop for ObjectBatch<'_, T, Ty, C> {
    fn drop(&mut self) {
        if let Some(mut groups) = self.shared.groups_for_drop() {
            for object in &self.meta.objects {
                groups.release_ref(object.group);
                for &(_, g) in &object.pending {
                    groups.release_ref(g);
                }
                for &(_, g) in &object.shared {
                    groups.release_ref(g);
                }
            }
        }
    }
}

impl<'a, T, Ty, C> Batch for ObjectBatch<'a, T, Ty, C>
where
    T: Outputs<Ty, C> + Send + Sync + 'a,
    Ty: 'static,
    C: Send + Sync + 'a,
{
    fn start<'this>(&'this self) -> Scope<'this> {
        let parents: Vec<&'this T> = self.values.iter().collect();
        let base_groups: Vec<GroupId> = self.meta.objects.iter().map(|o| o.group).collect();
        let cx = FieldsCx {
            shared: self.shared,
            app: self.app,
            table: self.table,
            header: self.header,
            entry: self.entry,
            meta: &self.meta,
            contexts: &self.contexts,
            groups: base_groups.clone(),
        };
        let futures = T::__start_fields(&cx, &parents, 0);
        let deferred = (1..self.header.sets.len())
            .map(|set| {
                let usages = &self.header.sets[set].0;
                let attributed = usages
                    .iter()
                    .copied()
                    .max()
                    .expect("deferred set has usages");
                // A set several fragments select runs under its own group;
                // any other under its one fragment's.
                let groups: Vec<GroupId> = self
                    .meta
                    .objects
                    .iter()
                    .map(|o| {
                        let shared = o.shared.iter().find(|(s, _)| *s == set).map(|(_, g)| *g);
                        let own = o
                            .pending
                            .iter()
                            .find(|(u, _)| *u == attributed)
                            .map(|(_, g)| *g);
                        shared.or(own).unwrap_or(o.group)
                    })
                    .collect();
                let cx = FieldsCx {
                    groups: groups.clone(),
                    ..cx.clone()
                };
                let parents = parents.clone();
                DeferredSet {
                    set,
                    groups,
                    state: DeferredSetState::Waiting(Box::new(move || {
                        T::__start_fields(&cx, &parents, set)
                    })),
                }
            })
            .collect();
        Scope::new(&self.meta, self.shared, 0, base_groups, futures, deferred)
    }
}

pub(crate) fn new_column<'a>(
    field: u32,
    shape: Shape,
    kind: FieldKind,
    n: usize,
    objects: bool,
) -> Column<'a> {
    let depth = shape.levels.len();
    let mut turn = Turn::new(depth, objects);
    if depth == 0 {
        turn.with_stored_mut(|stored| {
            for _ in 0..n {
                stored.inner.push_pending();
            }
        });
    }
    Column {
        field,
        kind,
        shape,
        level0: if depth >= 1 {
            vec![Slot::Pending; n]
        } else {
            Vec::new()
        },
        turns: vec![turn],
        stream: None,
        introspection: None,
    }
}

/// Whether the innermost position of `Ty` is an object (child scopes) or a leaf.
pub trait InnerKind {
    const OBJECTS: bool;
}

impl<Ty: InnerKind> InnerKind for List<Ty> {
    const OBJECTS: bool = Ty::OBJECTS;
}
impl<Ty: InnerKind> InnerKind for Nullable<Ty> {
    const OBJECTS: bool = Ty::OBJECTS;
}
impl InnerKind for crate::value::scalars::Int {
    const OBJECTS: bool = false;
}
impl InnerKind for crate::value::scalars::Float {
    const OBJECTS: bool = false;
}
impl InnerKind for crate::value::scalars::String {
    const OBJECTS: bool = false;
}
impl InnerKind for crate::value::scalars::Boolean {
    const OBJECTS: bool = false;
}
impl InnerKind for crate::value::scalars::ID {
    const OBJECTS: bool = false;
}

/// One field's future: resolve over the live parents, then complete.
pub fn field<'a, T, F, C>(
    cx: &FieldsCx<'a, C>,
    index: u32,
    parents: &[&'a T],
    args: &'a Result<Args<F>, Error>,
) -> FieldFuture<'a>
where
    T: Resolver<F, C> + 'a,
    F: Field,
    F::Type: InnerKind,
    C: Send + Sync + 'a,
{
    let cx = cx.clone();
    let parents: Vec<&'a T> = parents.to_vec();
    Box::pin(async move {
        let n = parents.len();
        let generation = cx.meta.generation;
        let mut column = new_column(index, F::SHAPE, FieldKind::Normal, n, F::Type::OBJECTS);
        let live: Vec<u32> = {
            let groups = cx.shared.groups();
            (0..n as u32)
                .filter(|&i| !groups.is_dead(cx.groups[i as usize]))
                .collect()
        };
        if live.is_empty() {
            return column;
        }
        let base = |i: u32| Pos {
            slot: i,
            object: i,
            indices: SmallVec::new(),
        };
        let mut cc =
            Completion::immediate(&cx, index, &mut column, LeafPath::default(), generation);
        let args = match args {
            Ok(args) => args,
            Err(error) => {
                for &i in &live {
                    cc.error(&base(i), error.clone());
                }
                return column;
            }
        };
        let ctx: &'a Context<'a, C> = &cx.contexts[index as usize];
        let live_parents: Vec<&'a T> = live.iter().map(|&i| parents[i as usize]).collect();
        match T::resolve(&live_parents, args, ctx).await {
            Err(error) => {
                for &i in &live {
                    cc.error(&base(i), error.clone());
                }
            }
            Ok(values) if values.len() != live.len() => {
                let error = Error::framework(
                    format!(
                        "resolver for field `{}` returned {} outputs for {} parents",
                        F::NAME,
                        values.len(),
                        live.len()
                    ),
                    "CARDINALITY",
                );
                for &i in &live {
                    cc.error(&base(i), error.clone());
                }
            }
            Ok(values) => {
                let positions = live.iter().map(|&i| base(i)).collect();
                <T::Output<'a> as Outputs<F::Type, C>>::__complete(values, positions, &mut cc);
            }
        }
        if let Some(mut driver) = column.stream.take() {
            driver.initial(&mut column).await;
            column.stream = Some(driver);
        }
        column
    })
}

pub fn typename_field<'a, C: Send + Sync + 'a>(
    cx: &FieldsCx<'a, C>,
    index: u32,
) -> FieldFuture<'a> {
    let n = cx.meta.objects.len();
    Box::pin(
        async move { new_column(index, Shape::new(&[], false), FieldKind::Typename, n, false) },
    )
}

pub fn introspection_field<'a, C: Send + Sync + 'a>(
    cx: &FieldsCx<'a, C>,
    index: u32,
) -> FieldFuture<'a> {
    let n = cx.meta.objects.len();
    let key = &cx.header.fields[index as usize].key;
    let value = cx
        .shared
        .introspection
        .as_ref()
        .and_then(|v| v.get(key))
        .map(Value::from_json)
        .unwrap_or(Value::Null);
    Box::pin(async move {
        let mut column = new_column(
            index,
            Shape::new(&[], true),
            FieldKind::Introspection,
            n,
            false,
        );
        column.introspection = Some(value);
        column
    })
}

// ---- built-in completions -------------------------------------------------

/// The first item error of a list, leaving its index path on `indices`.
fn first_item_error<I>(
    items: impl Iterator<Item = I>,
    indices: &mut Vec<u32>,
    first_error: impl Fn(&I, &mut Vec<u32>) -> Option<Error>,
) -> Option<Error> {
    for (i, item) in items.enumerate() {
        indices.push(i as u32);
        if let Some(error) = first_error(&item, indices) {
            return Some(error);
        }
        indices.pop();
    }
    None
}

macro_rules! scalar_tag {
    ($tag:ty) => {
        impl<T, C> Completes<T, C> for $tag
        where
            T: ToLeaf<$tag> + Send,
        {
            fn walk<'a>(_: &mut Walker<'_, C>, _: NodeId, _: &LeafPath) -> Result<(), Abort>
            where
                T: 'a,
                C: 'a,
            {
                Ok(())
            }

            fn first_error(
                value: &T,
                indices: &mut Vec<u32>,
                wanted: &dyn Fn(&[u32]) -> bool,
            ) -> Option<Error> {
                // Reached directly the position is non-null; `Nullable`
                // asks `null_leaf` first and never gets here with a null.
                value
                    .leaf_error()
                    .or_else(|| value.leaf_is_null().then(crate::value::null_at_non_null))
                    .filter(|_| wanted(indices))
            }

            fn null_leaf(value: &T) -> bool {
                value.leaf_is_null()
            }

            fn complete<'a>(values: Vec<T>, positions: Vec<Pos>, cc: &mut Completion<'a, '_, C>)
            where
                T: 'a,
                C: 'a,
            {
                for (value, pos) in values.into_iter().zip(&positions) {
                    cc.leaf(pos, value.to_leaf());
                }
            }

            #[cfg(feature = "reference-executor")]
            fn reference<'v, 's: 'v>(value: T, rc: &RefCompletion<'s, C>) -> BoxFuture<'v, RefValue>
            where
                T: 'v,
                C: 'v,
            {
                rc.leaf(value.to_leaf())
            }
        }
    };
}
scalar_tag!(crate::value::scalars::Int);
scalar_tag!(crate::value::scalars::Float);
scalar_tag!(crate::value::scalars::String);
scalar_tag!(crate::value::scalars::Boolean);
scalar_tag!(crate::value::scalars::ID);

impl<T, Ty, C> Completes<Option<T>, C> for Nullable<Ty>
where
    T: Outputs<Ty, C> + Send,
    C: Send + Sync,
{
    fn walk<'a>(w: &mut Walker<'_, C>, node: NodeId, leaf: &LeafPath) -> Result<(), Abort>
    where
        Option<T>: 'a,
        C: 'a,
    {
        T::__walk(w, node, leaf)
    }

    fn first_error(
        value: &Option<T>,
        indices: &mut Vec<u32>,
        wanted: &dyn Fn(&[u32]) -> bool,
    ) -> Option<Error> {
        value
            .as_ref()
            .filter(|v| !<T as Outputs<Ty, C>>::__null_leaf(v))
            .and_then(|v| v.__first_error(indices, wanted))
    }

    fn complete<'a>(values: Vec<Option<T>>, positions: Vec<Pos>, cc: &mut Completion<'a, '_, C>)
    where
        Option<T>: 'a,
        C: 'a,
    {
        let mut some = Vec::new();
        let mut some_pos = Vec::new();
        for (value, pos) in values.into_iter().zip(positions) {
            match value {
                Some(value) => {
                    some.push(value);
                    some_pos.push(pos);
                }
                None => cc.null(&pos),
            }
        }
        if !some.is_empty() {
            T::__complete(some, some_pos, cc);
        }
    }

    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(value: Option<T>, rc: &RefCompletion<'s, C>) -> BoxFuture<'v, RefValue>
    where
        Option<T>: 'v,
        C: 'v,
    {
        match value {
            Some(value) => {
                let inner = T::__reference(value, rc);
                Box::pin(async move { crate::exec::reference::nullable(inner.await) })
            }
            None => Box::pin(async { Ok(serde_json::Value::Null) }),
        }
    }
}

impl<T, Ty, C> Completes<Result<Option<T>, Error>, C> for Nullable<Ty>
where
    T: Outputs<Ty, C> + Send,
    C: Send + Sync,
{
    fn walk<'a>(w: &mut Walker<'_, C>, node: NodeId, leaf: &LeafPath) -> Result<(), Abort>
    where
        Result<Option<T>, Error>: 'a,
        C: 'a,
    {
        T::__walk(w, node, leaf)
    }

    fn first_error(
        value: &Result<Option<T>, Error>,
        indices: &mut Vec<u32>,
        wanted: &dyn Fn(&[u32]) -> bool,
    ) -> Option<Error> {
        match value {
            Ok(value) => value
                .as_ref()
                .filter(|v| !<T as Outputs<Ty, C>>::__null_leaf(v))
                .and_then(|v| v.__first_error(indices, wanted)),
            Err(error) => wanted(indices).then(|| error.clone()),
        }
    }

    fn complete<'a>(
        values: Vec<Result<Option<T>, Error>>,
        positions: Vec<Pos>,
        cc: &mut Completion<'a, '_, C>,
    ) where
        Result<Option<T>, Error>: 'a,
        C: 'a,
    {
        let mut ok = Vec::new();
        let mut ok_pos = Vec::new();
        for (value, pos) in values.into_iter().zip(positions) {
            match value {
                Ok(value) => {
                    ok.push(value);
                    ok_pos.push(pos);
                }
                Err(error) => cc.error(&pos, error),
            }
        }
        <Nullable<Ty> as Completes<Option<T>, C>>::complete(ok, ok_pos, cc);
    }

    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(
        value: Result<Option<T>, Error>,
        rc: &RefCompletion<'s, C>,
    ) -> BoxFuture<'v, RefValue>
    where
        Result<Option<T>, Error>: 'v,
        C: 'v,
    {
        match value {
            Ok(value) => <Nullable<Ty> as Completes<Option<T>, C>>::reference(value, rc),
            Err(error) => rc.error(error),
        }
    }
}

impl<'x, T, Ty, C> Completes<&'x Option<T>, C> for Nullable<Ty>
where
    &'x T: Outputs<Ty, C> + Send,
    T: Sync,
    C: Send + Sync,
{
    fn walk<'a>(w: &mut Walker<'_, C>, node: NodeId, leaf: &LeafPath) -> Result<(), Abort>
    where
        &'x Option<T>: 'a,
        C: 'a,
    {
        <&'x T as Outputs<Ty, C>>::__walk(w, node, leaf)
    }

    fn first_error(
        value: &&'x Option<T>,
        indices: &mut Vec<u32>,
        wanted: &dyn Fn(&[u32]) -> bool,
    ) -> Option<Error> {
        value
            .as_ref()
            .filter(|v| !<&'x T as Outputs<Ty, C>>::__null_leaf(v))
            .and_then(|v| <&'x T as Outputs<Ty, C>>::__first_error(&v, indices, wanted))
    }

    fn complete<'a>(values: Vec<&'x Option<T>>, positions: Vec<Pos>, cc: &mut Completion<'a, '_, C>)
    where
        &'x Option<T>: 'a,
        C: 'a,
    {
        let values: Vec<Option<&'x T>> = values.into_iter().map(Option::as_ref).collect();
        <Nullable<Ty> as Completes<Option<&'x T>, C>>::complete(values, positions, cc);
    }

    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(
        value: &'x Option<T>,
        rc: &RefCompletion<'s, C>,
    ) -> BoxFuture<'v, RefValue>
    where
        &'x Option<T>: 'v,
        C: 'v,
    {
        <Nullable<Ty> as Completes<Option<&'x T>, C>>::reference(value.as_ref(), rc)
    }
}

impl<'x, T, Ty, C> Completes<&'x Result<Option<T>, Error>, C> for Nullable<Ty>
where
    &'x T: Outputs<Ty, C> + Send,
    T: Sync,
    C: Send + Sync,
{
    fn walk<'a>(w: &mut Walker<'_, C>, node: NodeId, leaf: &LeafPath) -> Result<(), Abort>
    where
        &'x Result<Option<T>, Error>: 'a,
        C: 'a,
    {
        <&'x T as Outputs<Ty, C>>::__walk(w, node, leaf)
    }

    fn first_error(
        value: &&'x Result<Option<T>, Error>,
        indices: &mut Vec<u32>,
        wanted: &dyn Fn(&[u32]) -> bool,
    ) -> Option<Error> {
        <Nullable<Ty> as Completes<Result<Option<&'x T>, Error>, C>>::first_error(
            &value.as_ref().map(Option::as_ref).map_err(Clone::clone),
            indices,
            wanted,
        )
    }

    fn complete<'a>(
        values: Vec<&'x Result<Option<T>, Error>>,
        positions: Vec<Pos>,
        cc: &mut Completion<'a, '_, C>,
    ) where
        &'x Result<Option<T>, Error>: 'a,
        C: 'a,
    {
        let values = values
            .into_iter()
            .map(|value| value.as_ref().map(Option::as_ref).map_err(Clone::clone))
            .collect();
        <Nullable<Ty> as Completes<Result<Option<&'x T>, Error>, C>>::complete(
            values, positions, cc,
        );
    }

    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(
        value: &'x Result<Option<T>, Error>,
        rc: &RefCompletion<'s, C>,
    ) -> BoxFuture<'v, RefValue>
    where
        &'x Result<Option<T>, Error>: 'v,
        C: 'v,
    {
        <Nullable<Ty> as Completes<Result<Option<&'x T>, Error>, C>>::reference(
            value.as_ref().map(Option::as_ref).map_err(Clone::clone),
            rc,
        )
    }
}

impl<L, Ty, C> Completes<L, C> for List<Ty>
where
    L: ListOutput,
    L::Item: Outputs<Ty, C> + Send,
    Ty: 'static,
    C: Send + Sync,
{
    fn walk<'a>(w: &mut Walker<'_, C>, node: NodeId, leaf: &LeafPath) -> Result<(), Abort>
    where
        L: 'a,
        C: 'a,
    {
        <L::Item as Outputs<Ty, C>>::__walk(w, node, leaf)
    }

    fn first_error(
        value: &L,
        indices: &mut Vec<u32>,
        wanted: &dyn Fn(&[u32]) -> bool,
    ) -> Option<Error> {
        first_item_error(value.items(), indices, |item, indices| {
            <L::Item as Outputs<Ty, C>>::__first_error(item.borrow(), indices, wanted)
        })
    }

    fn complete<'a>(values: Vec<L>, positions: Vec<Pos>, cc: &mut Completion<'a, '_, C>)
    where
        L: 'a,
        C: 'a,
    {
        if cc.level == 0 && cc.streamed() {
            let sources = values
                .into_iter()
                .map(|list| {
                    let items: Vec<L::Item> = list.into_items().collect();
                    Streamed::new(futures::stream::iter(items.into_iter().map(Ok)))
                })
                .collect();
            cc.stream::<L::Item, Ty, _>(sources, positions);
            return;
        }
        let mut items = Vec::new();
        let mut item_pos = Vec::new();
        for (list, pos) in values.into_iter().zip(&positions) {
            let start = items.len();
            items.extend(list.into_items());
            item_pos.extend(cc.list(pos, items.len() - start));
        }
        if !items.is_empty() {
            cc.descend(|cc| <L::Item as Outputs<Ty, C>>::__complete(items, item_pos, cc));
        }
    }

    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(value: L, rc: &RefCompletion<'s, C>) -> BoxFuture<'v, RefValue>
    where
        L: 'v,
        C: 'v,
    {
        let nullable = rc.item_nullable();
        let mut rc = rc.clone();
        let items: Vec<_> = value
            .into_items()
            .map(|item| <L::Item as Outputs<Ty, C>>::__reference(item, &rc.item()))
            .collect();
        Box::pin(async move {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(item.await);
            }
            crate::exec::reference::list(out, nullable)
        })
    }
}

impl<X, Ty, C> Completes<Result<X, Error>, C> for List<Ty>
where
    List<Ty>: Completes<X, C>,
    C: Send + Sync,
{
    fn walk<'a>(w: &mut Walker<'_, C>, node: NodeId, leaf: &LeafPath) -> Result<(), Abort>
    where
        Result<X, Error>: 'a,
        C: 'a,
    {
        <List<Ty> as Completes<X, C>>::walk(w, node, leaf)
    }

    fn first_error(
        value: &Result<X, Error>,
        indices: &mut Vec<u32>,
        wanted: &dyn Fn(&[u32]) -> bool,
    ) -> Option<Error> {
        match value {
            Ok(value) => <List<Ty> as Completes<X, C>>::first_error(value, indices, wanted),
            Err(error) => wanted(indices).then(|| error.clone()),
        }
    }

    fn complete<'a>(
        values: Vec<Result<X, Error>>,
        positions: Vec<Pos>,
        cc: &mut Completion<'a, '_, C>,
    ) where
        Result<X, Error>: 'a,
        C: 'a,
    {
        let mut ok = Vec::new();
        let mut ok_pos = Vec::new();
        for (value, pos) in values.into_iter().zip(positions) {
            match value {
                Ok(value) => {
                    ok.push(value);
                    ok_pos.push(pos);
                }
                Err(error) => cc.error(&pos, error),
            }
        }
        if !ok.is_empty() {
            <List<Ty> as Completes<X, C>>::complete(ok, ok_pos, cc);
        }
    }

    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(
        value: Result<X, Error>,
        rc: &RefCompletion<'s, C>,
    ) -> BoxFuture<'v, RefValue>
    where
        Result<X, Error>: 'v,
        C: 'v,
    {
        match value {
            Ok(value) => <List<Ty> as Completes<X, C>>::reference(value, rc),
            Err(error) => rc.error(error),
        }
    }
}

impl<'x, X, Ty, C> Completes<&'x Result<X, Error>, C> for List<Ty>
where
    List<Ty>: Completes<&'x X, C>,
    C: Send + Sync,
{
    fn walk<'a>(w: &mut Walker<'_, C>, node: NodeId, leaf: &LeafPath) -> Result<(), Abort>
    where
        &'x Result<X, Error>: 'a,
        C: 'a,
    {
        <List<Ty> as Completes<&'x X, C>>::walk(w, node, leaf)
    }

    fn first_error(
        value: &&'x Result<X, Error>,
        indices: &mut Vec<u32>,
        wanted: &dyn Fn(&[u32]) -> bool,
    ) -> Option<Error> {
        <List<Ty> as Completes<Result<&'x X, Error>, C>>::first_error(
            &value.as_ref().map_err(Clone::clone),
            indices,
            wanted,
        )
    }

    fn complete<'a>(
        values: Vec<&'x Result<X, Error>>,
        positions: Vec<Pos>,
        cc: &mut Completion<'a, '_, C>,
    ) where
        &'x Result<X, Error>: 'a,
        C: 'a,
    {
        let values = values
            .into_iter()
            .map(|value| value.as_ref().map_err(Clone::clone))
            .collect();
        <List<Ty> as Completes<Result<&'x X, Error>, C>>::complete(values, positions, cc);
    }

    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(
        value: &'x Result<X, Error>,
        rc: &RefCompletion<'s, C>,
    ) -> BoxFuture<'v, RefValue>
    where
        &'x Result<X, Error>: 'v,
        C: 'v,
    {
        <List<Ty> as Completes<Result<&'x X, Error>, C>>::reference(
            value.as_ref().map_err(Clone::clone),
            rc,
        )
    }
}

impl<S, T, Ty, C> Completes<Streamed<S, Result<T, Error>>, C> for List<Ty>
where
    S: Stream<Item = Result<T, Error>> + Send,
    T: Outputs<Ty, C> + Send,
    Ty: 'static,
    C: Send + Sync,
{
    fn walk<'a>(w: &mut Walker<'_, C>, node: NodeId, leaf: &LeafPath) -> Result<(), Abort>
    where
        Streamed<S, Result<T, Error>>: 'a,
        C: 'a,
    {
        T::__walk(w, node, leaf)
    }

    fn complete<'a>(
        values: Vec<Streamed<S, Result<T, Error>>>,
        positions: Vec<Pos>,
        cc: &mut Completion<'a, '_, C>,
    ) where
        Streamed<S, Result<T, Error>>: 'a,
        C: 'a,
    {
        if cc.level != 0 {
            // Only the outermost list level streams; drain inner streams later.
            cc.stream_inner_unsupported(&positions);
            return;
        }
        cc.stream::<T, Ty, S>(values, positions);
    }

    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(
        value: Streamed<S, Result<T, Error>>,
        rc: &RefCompletion<'s, C>,
    ) -> BoxFuture<'v, RefValue>
    where
        Streamed<S, Result<T, Error>>: 'v,
        C: 'v,
    {
        let nullable = rc.item_nullable();
        let mut rc = rc.clone();
        Box::pin(async move {
            let mut source = value.0.boxed();
            let mut items = Vec::new();
            while let Some(item) = source.next().await {
                let item_rc = rc.item();
                let value = match item {
                    Ok(value) => T::__reference(value, &item_rc).await,
                    Err(error) => item_rc.error(error).await,
                };
                items.push(value);
            }
            crate::exec::reference::list(items, nullable)
        })
    }
}

/// Completion for a member wrapper at an abstract position: delegates to the
/// member tag under an extra partition-leaf step. Generated abstract impls call this.
pub fn complete_as<'a, Tag, T, C>(
    step: u8,
    values: Vec<As<Tag, T>>,
    positions: Vec<Pos>,
    cc: &mut Completion<'a, '_, C>,
) where
    T: Outputs<Tag, C> + 'a,
    C: Send + Sync + 'a,
{
    let values: Vec<T> = values.into_iter().map(As::into_inner).collect();
    cc.with_leaf(step, |cc| T::__complete(values, positions, cc));
}

pub fn walk_as<'a, Tag, T, C>(
    step: u8,
    w: &mut Walker<'_, C>,
    node: NodeId,
    leaf: &LeafPath,
) -> Result<(), Abort>
where
    T: Outputs<Tag, C> + 'a,
    C: 'a,
{
    T::__walk(w, node, &leaf.push(step))
}

/// Completion for `Either` at an abstract position: partitions into both arms.
pub fn complete_either<'a, Ty, A, B, C>(
    values: Vec<Either<A, B>>,
    positions: Vec<Pos>,
    cc: &mut Completion<'a, '_, C>,
) where
    Ty: Completes<A, C> + Completes<B, C>,
    A: 'a,
    B: 'a,
    C: Send + Sync + 'a,
{
    let mut a = Vec::new();
    let mut a_pos = Vec::new();
    let mut b = Vec::new();
    let mut b_pos = Vec::new();
    for (value, pos) in values.into_iter().zip(positions) {
        match value {
            Either::A(value) => {
                a.push(value);
                a_pos.push(pos);
            }
            Either::B(value) => {
                b.push(value);
                b_pos.push(pos);
            }
        }
    }
    if !a.is_empty() {
        cc.with_leaf(0, |cc| <Ty as Completes<A, C>>::complete(a, a_pos, cc));
    }
    if !b.is_empty() {
        cc.with_leaf(1, |cc| <Ty as Completes<B, C>>::complete(b, b_pos, cc));
    }
}

/// A borrowed `Either` as an `Either` of references.
pub fn either_ref<A, B>(value: &Either<A, B>) -> Either<&A, &B> {
    match value {
        Either::A(a) => Either::A(a),
        Either::B(b) => Either::B(b),
    }
}

pub fn walk_either<'a, Ty, A, B, C>(
    w: &mut Walker<'_, C>,
    node: NodeId,
    leaf: &LeafPath,
) -> Result<(), Abort>
where
    Ty: Completes<A, C> + Completes<B, C>,
    A: 'a,
    B: 'a,
    C: 'a,
{
    <Ty as Completes<A, C>>::walk(w, node, &leaf.push(0))?;
    <Ty as Completes<B, C>>::walk(w, node, &leaf.push(1))
}

impl<C> Completes<crate::resolver::NoMutation, C> for crate::resolver::NoMutationType {
    fn walk<'a>(_: &mut Walker<'_, C>, _: NodeId, _: &LeafPath) -> Result<(), Abort>
    where
        crate::resolver::NoMutation: 'a,
        C: 'a,
    {
        Err(Abort::one(
            "the schema declares no mutation type",
            Vec::new(),
        ))
    }

    fn complete<'a>(_: Vec<crate::resolver::NoMutation>, _: Vec<Pos>, _: &mut Completion<'a, '_, C>)
    where
        crate::resolver::NoMutation: 'a,
        C: 'a,
    {
    }

    #[cfg(feature = "reference-executor")]
    fn reference<'v, 's: 'v>(
        _: crate::resolver::NoMutation,
        _: &RefCompletion<'s, C>,
    ) -> BoxFuture<'v, RefValue>
    where
        crate::resolver::NoMutation: 'v,
        C: 'v,
    {
        Box::pin(async { Err(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::scalars::Float;

    #[test]
    fn first_error_finds_the_first_wanted_position() {
        let value = vec![vec![1.0, f64::NAN], vec![f64::NAN]];
        let search = |wanted: &dyn Fn(&[u32]) -> bool| {
            let mut indices = Vec::new();
            let error = <List<List<Float>> as Completes<Vec<Vec<f64>>, ()>>::first_error(
                &value,
                &mut indices,
                wanted,
            );
            (error.is_some(), indices)
        };
        assert_eq!(search(&|_| true), (true, vec![0, 1]));
        // An error the caller does not want is skipped, not returned.
        assert_eq!(search(&|at| at[0] == 1), (true, vec![1, 0]));
        assert_eq!(search(&|_| false), (false, vec![]));
        // Borrowed rows of a borrowed list find the same position.
        let rows: Vec<&Vec<f64>> = value.iter().collect();
        let mut indices = Vec::new();
        let error = <List<List<Float>> as Completes<&Vec<&Vec<f64>>, ()>>::first_error(
            &&rows,
            &mut indices,
            &|_| true,
        );
        assert_eq!((error.is_some(), indices), (true, vec![0, 1]));
    }

    #[test]
    fn first_error_counts_a_null_leaf_only_at_a_non_null_position() {
        struct Json;
        impl ToLeaf<Json> for serde_json::Value {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> {
                Ok(Value::from_json(&self))
            }
            fn leaf_is_null(&self) -> bool {
                self.is_null()
            }
        }
        scalar_tag!(Json);
        let null = serde_json::Value::Null;
        let first = |strict: bool| {
            let mut indices = Vec::new();
            if strict {
                <List<Json> as Completes<Vec<serde_json::Value>, ()>>::first_error(
                    &vec![serde_json::json!(1), null.clone()],
                    &mut indices,
                    &|_| true,
                )
                .map(|_| indices)
            } else {
                <List<Nullable<Json>> as Completes<Vec<Option<serde_json::Value>>, ()>>::first_error(
                    &vec![Some(serde_json::json!(1)), Some(null.clone()), None],
                    &mut indices,
                    &|_| true,
                )
                .map(|_| indices)
            }
        };
        assert_eq!(first(true), Some(vec![1]));
        assert_eq!(first(false), None);
    }
}
