use crate::error::{Error, GraphQLError, PathSegment};
use crate::exec::column::{
    Column, ErrorRecord, Slot, Storage, Stored, StreamCell, Turn, TurnBatch, TurnRange,
};
use crate::exec::complete::{Completion, FieldsCx, Pos};
use crate::exec::state::{ErrorBehavior, GroupId, Groups};
use crate::plan::Leaf as LeafPath;
use crate::resolver::{Outputs, Shape};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use std::cell::Cell;
use std::marker::PhantomData;
use std::ops::ControlFlow;
use std::task::{Context as TaskContext, Poll};

/// The type-erased driver of one streamed (or lazily drained) list column.
pub trait StreamDriver<'a>: Send {
    /// Pulls and completes the initial items inside the immediate scope.
    fn initial<'s>(&'s mut self, column: &'s mut Column<'a>) -> BoxFuture<'s, ()>;
    /// Polls live sources into the bounded buffer; true when a turn can be
    /// made or a source ended.
    fn pump(&mut self, cx: &mut TaskContext<'_>) -> bool;
    /// Completes the buffered items as one new turn on the column; true when
    /// a turn was made (a reused slot counts as progress too).
    fn make_turn(&mut self, column: &mut Column<'a>) -> bool;
    /// All sources ended and nothing is buffered.
    fn is_done(&self) -> bool;
    /// Visits the groups whose completion this driver still holds back:
    /// each live parent's stream group, and the deferred groups it carries
    /// whose field sets its items can still produce. A group may come twice.
    fn live_groups(
        &self,
        groups: &Groups,
        f: &mut dyn FnMut(GroupId) -> ControlFlow<()>,
    ) -> ControlFlow<()>;
    fn groups(&self) -> &[GroupId];
    /// Whether `groups` are this driver's own stream groups. A lazily drained
    /// list holds its parents' groups instead, which are not its to announce,
    /// ship or complete.
    fn owns_groups(&self) -> bool;
    fn parent_object(&self, parent: usize) -> u32;
    fn source_ended(&self, parent: usize) -> bool;
    fn release(&mut self);
    fn is_released(&self) -> bool;
    fn drop_group(&mut self, group: GroupId);
}

pub struct StreamState<'a, T, Ty, C> {
    sources: Vec<Option<BoxStream<'a, Result<T, Error>>>>,
    positions: Vec<Pos>,
    groups: Vec<GroupId>,
    initial_count: Option<u32>,
    /// Whether an item error nulls the item (source continues) or fails the stream.
    item_nullable: bool,
    shape: Shape,
    next_index: Vec<u32>,
    buffer: Vec<(usize, Result<T, Error>)>,
    released: bool,
    cx: FieldsCx<'a, C>,
    field: u32,
    leaf: LeafPath,
    generation: u32,
    capacity: usize,
    tag: PhantomData<fn() -> Ty>,
}

impl<'a, T, Ty, C> StreamState<'a, T, Ty, C> {
    /// An item error ends the source only when it cannot be absorbed: a
    /// non-null item type under a behavior that propagates or halts.
    fn terminates_on_error(&self) -> bool {
        !self.item_nullable && self.cx.shared.behavior != ErrorBehavior::Null
    }

    /// Under `Halt`, hands the error item at `index` of parent `p` to `group`
    /// (the parent's own group during the initial pull, its stream group
    /// afterwards) before anything else is pulled. `beneath` leads from the
    /// item to the failing position inside it.
    fn halt_at(&self, p: usize, index: u32, beneath: &[u32], error: &Error, group: GroupId) {
        let object = self.positions[p].object;
        let field = &self.cx.header.fields[self.field as usize];
        self.cx.shared.halt(group, || {
            let mut path = self
                .cx
                .meta
                .path_to(object, &field.key, &self.positions[p].indices);
            path.extend(
                std::iter::once(&index)
                    .chain(beneath)
                    .map(|&i| PathSegment::Index(i as usize)),
            );
            GraphQLError::from_error(error, field.spans.clone(), path)
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        sources: Vec<BoxStream<'a, Result<T, Error>>>,
        positions: Vec<Pos>,
        groups: Vec<GroupId>,
        initial_count: Option<u32>,
        cx: FieldsCx<'a, C>,
        field: u32,
        leaf: LeafPath,
        generation: u32,
        capacity: usize,
        shape: Shape,
    ) -> Self {
        let n = sources.len();
        {
            let mut table = cx.shared.groups();
            for &g in &groups {
                table.retain(g);
            }
        }
        Self {
            sources: sources.into_iter().map(Some).collect(),
            positions,
            groups,
            initial_count,
            item_nullable: shape.nullable_at(1),
            shape,
            next_index: vec![0; n],
            buffer: Vec::new(),
            released: false,
            cx,
            field,
            leaf,
            generation,
            capacity: capacity.max(1),
            tag: PhantomData,
        }
    }
}

impl<T: Outputs<Ty, C>, Ty, C> StreamState<'_, T, Ty, C> {
    /// The error that ends this parent's source at `item`, with the indices
    /// from the item to its position: under `Halt` any error, otherwise one
    /// the non-null item cannot absorb. It may come from the source or from
    /// completing the item, which would record it before any resolver runs.
    fn terminal_error(&self, item: &Result<T, Error>) -> Option<(Vec<u32>, Error)> {
        let halt = self.cx.shared.behavior == ErrorBehavior::Halt;
        if !halt && !self.terminates_on_error() {
            return None;
        }
        match item {
            Ok(value) => {
                let shape = self.shape;
                // An error beneath the item reaches it only through non-null positions.
                let reaches_item = |beneath: &[u32]| {
                    halt || (1..=beneath.len() + 1).all(|level| !shape.nullable_at(level))
                };
                let mut beneath = Vec::new();
                value
                    .__first_error(&mut beneath, &reaches_item)
                    .map(|error| (beneath, error))
            }
            Err(error) => Some((Vec::new(), error.clone())),
        }
    }
}

impl<T, Ty, C> Drop for StreamState<'_, T, Ty, C> {
    fn drop(&mut self) {
        if let Some(mut table) = self.cx.shared.groups_for_drop() {
            for &g in &self.groups {
                table.release_ref(g);
            }
        }
    }
}

/// What a stream turn's values borrow: the items until they complete, and
/// the field context they complete in.
struct TurnItems<'a, T, Ty, C> {
    cx: FieldsCx<'a, C>,
    field: u32,
    shape: Shape,
    leaf: LeafPath,
    generation: u32,
    objects: bool,
    /// Innermost slots the turn opens: one per item at a depth-1 list.
    inner_len: u32,
    items: Cell<Option<TurnInput<T>>>,
    tag: PhantomData<fn() -> Ty>,
}

/// The items a turn completes and the errors its sources yielded instead,
/// each with its position.
type TurnInput<T> = (Vec<T>, Vec<Pos>, Vec<(Pos, Error)>);

impl<'a, T, Ty, C> TurnBatch for TurnItems<'a, T, Ty, C>
where
    T: Outputs<Ty, C> + Send + 'a,
    Ty: 'static,
    C: Send + Sync + 'a,
{
    fn complete<'this>(
        &'this self,
        levels: &mut [Vec<Slot>],
        errors: &mut Vec<ErrorRecord>,
    ) -> Stored<'this> {
        let (values, positions, failed) = self.items.take().expect("a turn completes once");
        let mut stored = Stored::new(self.objects);
        for _ in 0..self.inner_len {
            stored.inner.push_pending();
        }
        let mut cc = Completion {
            cx: &self.cx,
            field: self.field,
            shape: self.shape,
            level0: &mut [],
            levels,
            errors,
            stored: &mut stored,
            stream: None,
            level: 1,
            leaf: self.leaf.clone(),
            generation: self.generation,
        };
        for (pos, error) in failed {
            cc.error(&pos, error);
        }
        if !values.is_empty() {
            T::__complete(values, positions, &mut cc);
        }
        stored
    }
}

impl<'a, T, Ty, C> StreamDriver<'a> for StreamState<'a, T, Ty, C>
where
    T: Outputs<Ty, C> + Send + 'a,
    Ty: 'static,
    C: Send + Sync + 'a,
{
    fn initial<'s>(&'s mut self, column: &'s mut Column<'a>) -> BoxFuture<'s, ()> {
        Box::pin(async move {
            use futures::StreamExt;
            let limit = self.initial_count;
            // A terminal item error must not wait for items that may never
            // come; under Halt the first failing item ends the pull outright.
            let terminates = self.initial_count.is_some() && self.terminates_on_error();
            let halt = self.cx.shared.behavior == ErrorBehavior::Halt;
            let mut pulled: Vec<Vec<Result<T, Error>>> =
                (0..self.sources.len()).map(|_| Vec::new()).collect();
            for (p, items) in pulled.iter_mut().enumerate() {
                // A parent whose group died (halted or dropped meanwhile) is
                // not pulled at all: its items could never be delivered.
                let group = self.cx.groups[self.positions[p].object as usize];
                if self.cx.shared.is_dead(group) {
                    self.sources[p] = None;
                    continue;
                }
                loop {
                    if limit.is_some_and(|k| items.len() as u32 >= k) {
                        break;
                    }
                    let Some(stream) = self.sources[p].as_mut() else {
                        break;
                    };
                    match stream.next().await {
                        Some(item) => {
                            let terminal = self.terminal_error(&item);
                            items.push(item);
                            if let Some((beneath, error)) = &terminal {
                                self.halt_at(p, items.len() as u32 - 1, beneath, error, group);
                            }
                            // A lazily drained list keeps pulling so every
                            // item error is reported, as depth-first would.
                            if terminal.is_some() && (halt || self.initial_count.is_some()) {
                                self.sources[p] = None;
                                break;
                            }
                        }
                        None => {
                            self.sources[p] = None;
                            break;
                        }
                    }
                }
            }
            let mut cc = Completion::immediate(
                &self.cx,
                self.field,
                column,
                self.leaf.clone(),
                self.generation,
            );
            let mut values = Vec::new();
            let mut value_pos = Vec::new();
            for (p, items) in pulled.into_iter().enumerate() {
                let positions = cc.list(&self.positions[p], items.len());
                self.next_index[p] = items.len() as u32;
                for (item, pos) in items.into_iter().zip(positions) {
                    match item {
                        Ok(value) => {
                            values.push(value);
                            value_pos.push(pos);
                        }
                        Err(error) => {
                            if terminates || halt {
                                self.sources[p] = None;
                            }
                            cc.descend(|cc| cc.error(&pos, error));
                        }
                    }
                }
            }
            if !values.is_empty() {
                cc.descend(|cc| T::__complete(values, value_pos, cc));
            }
            if self.initial_count.is_none() {
                self.sources.iter_mut().for_each(|s| *s = None);
            }
        })
    }

    fn pump(&mut self, cx: &mut TaskContext<'_>) -> bool {
        if !self.released {
            return false;
        }
        // A source reaching its end is progress too: the barrier must run so
        // the stream's group completes without waiting on unrelated streams.
        let mut ended = false;
        let count = self.sources.len();
        for p in 0..count {
            if self.sources[p].is_some() && self.cx.shared.is_dead(self.groups[p]) {
                self.sources[p] = None;
                self.buffer.retain(|(q, _)| *q != p);
                ended = true;
                continue;
            }
            while self.buffer.len() < self.capacity {
                let Some(source) = self.sources[p].as_mut() else {
                    break;
                };
                match source.as_mut().poll_next(cx) {
                    Poll::Pending => break,
                    Poll::Ready(None) => {
                        self.sources[p] = None;
                        ended = true;
                    }
                    Poll::Ready(Some(item)) => {
                        let terminal = self.terminal_error(&item);
                        if let Some((beneath, error)) = &terminal {
                            let buffered = self.buffer.iter().filter(|(q, _)| *q == p).count();
                            self.halt_at(
                                p,
                                self.next_index[p] + buffered as u32,
                                beneath,
                                error,
                                self.groups[p],
                            );
                        }
                        self.buffer.push((p, item));
                        if terminal.is_some() {
                            self.sources[p] = None;
                            ended = true;
                        }
                    }
                }
            }
        }
        ended || !self.buffer.is_empty()
    }

    fn make_turn(&mut self, column: &mut Column<'a>) -> bool {
        if self.buffer.is_empty() {
            return false;
        }
        let depth = column.depth();
        let objects = column.turns[0].objects();
        let turn_index = match column.turns.iter().position(|t| t.retired) {
            Some(index) => {
                column.turns[index].reset(depth, objects);
                index
            }
            None => {
                column.turns.push(Turn::new(depth, objects));
                column.turns.len() - 1
            }
        };
        crate::__private::MAX_LIVE_TURNS
            .fetch_max(column.turns.len(), std::sync::atomic::Ordering::Relaxed);
        let buffer = std::mem::take(&mut self.buffer);
        let mut values = Vec::new();
        let mut value_pos = Vec::new();
        let mut failed = Vec::new();
        let mut inner_len = 0;
        let turn = &mut column.turns[turn_index];
        let mut per_parent: Vec<Vec<Result<T, Error>>> =
            (0..self.sources.len()).map(|_| Vec::new()).collect();
        for (p, item) in buffer {
            per_parent[p].push(item);
        }
        for (p, items) in per_parent.into_iter().enumerate() {
            if items.is_empty() {
                continue;
            }
            let len = items.len() as u32;
            let start_slot = if depth == 1 {
                inner_len += len;
                inner_len - len
            } else {
                let start = turn.levels[0].len() as u32;
                turn.levels[0].extend(std::iter::repeat_n(Slot::Pending, len as usize));
                start
            };
            let object = self.positions[p].object;
            turn.ranges.push(TurnRange {
                object,
                start_index: self.next_index[p],
                start_slot,
                len,
                shipped: false,
            });
            for (j, item) in items.into_iter().enumerate() {
                let pos = Pos {
                    slot: start_slot + j as u32,
                    object,
                    indices: smallvec::smallvec![self.next_index[p] + j as u32],
                };
                match item {
                    Ok(value) => {
                        values.push(value);
                        value_pos.push(pos);
                    }
                    Err(error) => failed.push((pos, error)),
                }
            }
            self.next_index[p] += len;
        }
        // Turn items belong to their parent's stream group, not the scope's group.
        let mut cx = self.cx.clone();
        for (p, pos) in self.positions.iter().enumerate() {
            cx.groups[pos.object as usize] = self.groups[p];
        }
        let batch: Box<dyn TurnBatch + 'a> = Box::new(TurnItems::<T, Ty, C> {
            cx,
            field: self.field,
            shape: self.shape,
            leaf: self.leaf.clone(),
            generation: self.generation + 1,
            objects,
            inner_len,
            items: Cell::new(Some((values, value_pos, failed))),
            tag: PhantomData,
        });
        let Turn {
            levels,
            errors,
            stored,
            ..
        } = turn;
        *stored = Storage::Stream(StreamCell::new(batch, |batch| {
            batch.complete(levels, errors)
        }));
        true
    }

    fn is_done(&self) -> bool {
        self.sources.iter().all(Option::is_none) && self.buffer.is_empty()
    }

    fn live_groups(
        &self,
        groups: &Groups,
        f: &mut dyn FnMut(GroupId) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        // A live source may still produce items whose deferred field sets
        // belong to a group the parent object carries; that group cannot
        // complete before the stream does.
        // A stream delivered under a carried group (a descendant) cannot hold
        // it back: it is only announced once that group ships.
        for (p, &g) in self.groups.iter().enumerate() {
            if self.sources[p].is_none() && !self.buffer.iter().any(|(q, _)| *q == p) {
                continue;
            }
            f(g)?;
            let parent = &self.cx.meta.objects[self.positions[p].object as usize];
            for &(_, carried) in &parent.pending {
                if groups.is_ancestor(carried, g) {
                    continue;
                }
                if let crate::exec::state::GroupKind::Defer { usage, .. } =
                    &groups.get(carried).kind
                    && self.cx.header.fields[self.field as usize]
                        .beneath
                        .contains(usage)
                {
                    f(carried)?;
                }
            }
        }
        ControlFlow::Continue(())
    }

    fn groups(&self) -> &[GroupId] {
        &self.groups
    }

    fn parent_object(&self, parent: usize) -> u32 {
        self.positions[parent].object
    }

    fn source_ended(&self, parent: usize) -> bool {
        self.sources[parent].is_none() && !self.buffer.iter().any(|(q, _)| *q == parent)
    }

    fn release(&mut self) {
        self.released = true;
    }

    fn owns_groups(&self) -> bool {
        self.initial_count.is_some()
    }

    fn is_released(&self) -> bool {
        self.released
    }

    fn drop_group(&mut self, group: GroupId) {
        for (p, &g) in self.groups.iter().enumerate() {
            if g == group {
                self.sources[p] = None;
                self.buffer.retain(|(q, _)| *q != p);
            }
        }
    }
}
