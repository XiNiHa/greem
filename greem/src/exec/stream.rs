use crate::error::{Error, GraphQLError, PathSegment};
use crate::exec::column::{
    Column, ErrorRecord, Slot, Storage, Stored, StreamCell, Turn, TurnBatch, TurnRange,
};
use crate::exec::complete::{Completion, FieldsCx, KeptOutputs, Pos};
use crate::exec::scope::STREAM;
use crate::exec::state::{ErrorBehavior, GroupId, GroupKind, Groups};
use crate::plan::Leaf as LeafPath;
use crate::resolver::{Outputs, Shape};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use smallvec::SmallVec;
use std::cell::Cell;
use std::marker::PhantomData;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::task::{Context as TaskContext, Poll};

/// The type-erased driver of one streamed (or lazily drained) list column.
pub trait StreamDriver<'a>: Send {
    /// Pulls and completes the initial items inside the immediate scope.
    fn initial<'s>(&'s mut self, column: &'s mut Column<'a>) -> BoxFuture<'s, ()>;
    /// Polls live sources into the bounded buffer; true when a turn can be
    /// made or a source ended.
    fn pump(&mut self, cx: &mut TaskContext<'_>) -> bool;
    /// Completes the buffered items as one new turn on the column and
    /// returns its index (a reused slot counts as progress too).
    fn make_turn(&mut self, column: &mut Column<'a>) -> Option<usize>;
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
    fn parent_count(&self) -> usize;
    fn group(&self, parent: usize) -> GroupId;
    /// Whether the parents' groups are this driver's own stream groups. A
    /// lazily drained list holds its parents' groups instead, which are not
    /// its to announce, ship or complete.
    fn owns_groups(&self) -> bool;
    fn parent_object(&self, parent: usize) -> u32;
    /// The parent index of `object`, if the object streams this list.
    fn parent_of(&self, object: u32) -> Option<usize>;
    fn source_ended(&self, parent: usize) -> bool;
    /// One of `parent`'s item ranges shipped (or was discarded as dead).
    fn range_shipped(&mut self, parent: usize);
    /// The parents whose stream may complete now, in index order: their
    /// source ended and every item they produced has shipped. Each comes
    /// once per event that could have made it so.
    fn take_completable(&mut self) -> Vec<usize>;
    /// Examines `parent` for completion at the next barrier. The driver
    /// queues its own events; the barrier queues a parent whose stream
    /// could not complete yet for a reason the driver cannot see.
    fn queue_completion(&mut self, parent: usize);
    fn release(&mut self);
    fn is_released(&self) -> bool;
    /// Ends `parent`'s source and discards its buffered items: its group
    /// failed or was dropped.
    fn drop_parent(&mut self, parent: usize, table: &mut Groups);
    /// Drops every hold this driver has on groups: its scope went quiescent.
    fn release_holds(&mut self, table: &mut Groups);
}

impl<'a> dyn StreamDriver<'a> + '_ {
    /// Every parent's group, in parent order.
    pub fn groups(&self) -> impl Iterator<Item = GroupId> + '_ {
        (0..self.parent_count()).map(move |p| self.group(p))
    }
}

/// One parent object's share of a streamed column.
struct Parent<'a, T> {
    /// `None` once it ended: drained, failed, or its group died.
    source: Option<BoxStream<'a, Result<T, Error>>>,
    position: Pos,
    group: GroupId,
    /// Items of this parent in the buffer.
    buffered: u32,
    /// Holds `group` and `carried`: the source may still yield, or items
    /// wait in the buffer.
    live: bool,
    /// The deferred groups the parent object carries whose field sets its
    /// items can still produce; they cannot complete before the stream does.
    carried: SmallVec<[GroupId; 2]>,
    /// List index of the next item it yields.
    next_index: u32,
    /// Item ranges made into turns but not shipped yet.
    unshipped: u32,
    /// Listed in `completable`.
    queued: bool,
}

pub struct StreamState<'a, T, Ty, C> {
    parents: Vec<Parent<'a, T>>,
    /// Per object of the scope: its parent index, or `u32::MAX`.
    by_object: Vec<u32>,
    /// `cx.groups` with each parent's object under its stream group: turn
    /// items belong to it, not to the scope's group.
    turn_groups: Arc<[GroupId]>,
    initial_count: Option<u32>,
    /// Whether an item error nulls the item (source continues) or fails the stream.
    item_nullable: bool,
    shape: Shape,
    buffer: Vec<(usize, Result<T, Error>)>,
    /// Parents to examine for completion at the next barrier, each at most
    /// once (`Parent::queued`).
    completable: Vec<usize>,
    /// Parents whose source has not ended.
    live_sources: usize,
    /// Every parent before it has an ended source, so a pump starts here.
    cursor: usize,
    released: bool,
    /// Counted in `Shared::live_streams`: released and not done.
    counted_live: bool,
    /// `release_holds` ran: no parent holds anything any more.
    holds_released: bool,
    cx: FieldsCx<'a, C>,
    field: u32,
    leaf: LeafPath,
    generation: u32,
    capacity: usize,
    tag: PhantomData<fn() -> Ty>,
}

impl<'a, T, Ty, C> StreamState<'a, T, Ty, C> {
    /// Ends `p`'s source, if it had not ended, and asks the next barrier
    /// whether its stream completes.
    fn end_source(&mut self, p: usize, table: &mut Groups) {
        if self.parents[p].source.take().is_some() {
            self.live_sources -= 1;
        }
        self.queue_completion(p);
        self.update_live(p, table);
    }

    /// Keeps `p`'s holds while its source may yield or its items wait in
    /// the buffer.
    fn update_live(&mut self, p: usize, table: &mut Groups) {
        if self.holds_released {
            return;
        }
        let parent = &mut self.parents[p];
        let live = parent.source.is_some() || parent.buffered > 0;
        if live == parent.live {
            return;
        }
        parent.live = live;
        self.cx.signal.raise(STREAM);
        let (group, carried) = (parent.group, parent.carried.clone());
        for g in std::iter::once(group).chain(carried) {
            if live {
                table.hold(g);
            } else {
                table.unhold(g);
            }
        }
    }

    fn queue_completion(&mut self, parent: usize) {
        if !std::mem::replace(&mut self.parents[parent].queued, true) {
            self.completable.push(parent);
            self.cx.signal.raise(STREAM);
        }
    }

    /// Keeps `Shared::live_streams` counting this driver while it is
    /// released and not done.
    fn count_live(&mut self) {
        let live = self.released && !(self.live_sources == 0 && self.buffer.is_empty());
        if live != self.counted_live {
            self.counted_live = live;
            if live {
                self.cx.shared.live_streams.fetch_add(1, Relaxed);
            } else {
                self.cx.shared.live_streams.fetch_sub(1, Relaxed);
            }
        }
    }

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
        let object = self.parents[p].position.object;
        let field = &self.cx.header.fields[self.field as usize];
        if self.cx.shared.behavior == ErrorBehavior::Halt {
            self.cx.signal.raise(STREAM);
        }
        self.cx.shared.halt(group, || {
            let mut path =
                self.cx
                    .meta
                    .path_to(object, &field.key, &self.parents[p].position.indices);
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
        {
            let mut table = cx.shared.groups();
            for &g in &groups {
                table.retain(g);
            }
        }
        let live_sources = sources.len();
        let beneath = &cx.header.fields[field as usize].beneath;
        let mut table = cx.shared.groups();
        let parents: Vec<Parent<'a, T>> = sources
            .into_iter()
            .zip(positions)
            .zip(groups)
            .map(|((source, position), group)| {
                // A stream delivered under a carried group (a descendant)
                // cannot hold it back: it is only announced once that
                // group ships.
                let carried: SmallVec<[GroupId; 2]> = cx.meta.objects[position.object as usize]
                    .pending
                    .iter()
                    .map(|&(_, carried)| carried)
                    .filter(|&carried| {
                        !table.is_ancestor(carried, group)
                            && matches!(
                                &table.get(carried).kind,
                                GroupKind::Defer { usage, .. } if beneath.contains(usage)
                            )
                    })
                    .collect();
                table.hold(group);
                for &c in &carried {
                    table.hold(c);
                }
                Parent {
                    source: Some(source),
                    position,
                    group,
                    buffered: 0,
                    live: true,
                    carried,
                    next_index: 0,
                    unshipped: 0,
                    queued: false,
                }
            })
            .collect();
        drop(table);
        let mut by_object = vec![u32::MAX; cx.meta.objects.len()];
        let mut turn_groups = cx.groups.to_vec();
        for (p, parent) in parents.iter().enumerate() {
            by_object[parent.position.object as usize] = p as u32;
            turn_groups[parent.position.object as usize] = parent.group;
        }
        Self {
            parents,
            by_object,
            turn_groups: turn_groups.into(),
            initial_count,
            item_nullable: shape.nullable_at(1),
            shape,
            buffer: Vec::new(),
            completable: Vec::new(),
            live_sources,
            cursor: 0,
            released: false,
            counted_live: false,
            holds_released: false,
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
        if self.counted_live {
            self.cx.shared.live_streams.fetch_sub(1, Relaxed);
        }
        if let Some(mut table) = self.cx.shared.groups_for_drop() {
            for parent in &self.parents {
                table.release_ref(parent.group);
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
    /// This turn's index on its column.
    turn: u32,
    objects: bool,
    /// Innermost slots the turn opens: one per item at a depth-1 list.
    inner_len: u32,
    items: Cell<Option<TurnInput<T>>>,
    /// Owned outputs the turn's values borrow; they retire with the turn.
    keep: KeptOutputs<C>,
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
            keep: &self.keep,
            stream: None,
            level: 1,
            leaf: self.leaf.clone(),
            generation: self.generation,
            turn: self.turn,
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
                (0..self.parents.len()).map(|_| Vec::new()).collect();
            for (p, items) in pulled.iter_mut().enumerate() {
                // A parent whose group died (halted or dropped meanwhile) is
                // not pulled at all: its items could never be delivered.
                let group = self.cx.groups[self.parents[p].position.object as usize];
                if self.cx.shared.is_dead(group) {
                    let shared = self.cx.shared;
                    self.end_source(p, &mut shared.groups());
                    continue;
                }
                loop {
                    if limit.is_some_and(|k| items.len() as u32 >= k) {
                        break;
                    }
                    let Some(stream) = self.parents[p].source.as_mut() else {
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
                                let shared = self.cx.shared;
                                self.end_source(p, &mut shared.groups());
                                break;
                            }
                        }
                        None => {
                            let shared = self.cx.shared;
                            self.end_source(p, &mut shared.groups());
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
            let mut ended = Vec::new();
            for (p, items) in pulled.into_iter().enumerate() {
                let positions = cc.list(&self.parents[p].position, items.len());
                self.parents[p].next_index = items.len() as u32;
                for (item, pos) in items.into_iter().zip(positions) {
                    match item {
                        Ok(value) => {
                            values.push(value);
                            value_pos.push(pos);
                        }
                        Err(error) => {
                            if terminates || halt {
                                ended.push(p);
                            }
                            cc.descend(|cc| cc.error(&pos, error));
                        }
                    }
                }
            }
            if !values.is_empty() {
                cc.descend(|cc| T::__complete(values, value_pos, cc));
            }
            let shared = self.cx.shared;
            let mut table = shared.groups();
            for p in ended {
                self.end_source(p, &mut table);
            }
            if self.initial_count.is_none() {
                for p in 0..self.parents.len() {
                    self.end_source(p, &mut table);
                }
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
        let count = self.parents.len();
        while self.cursor < count && self.parents[self.cursor].source.is_none() {
            self.cursor += 1;
        }
        // Parents fill the buffer in index order; the ones past where it
        // fills are not looked at, so a dead group among them is noticed
        // when the pump reaches it.
        let mut p = self.cursor;
        while p < count && self.buffer.len() < self.capacity {
            let parent = p;
            p += 1;
            #[cfg(debug_assertions)]
            crate::__private::STREAM_PARENTS_VISITED.fetch_add(1, Relaxed);
            if self.parents[parent].source.is_none() {
                continue;
            }
            if self.cx.shared.is_dead(self.parents[parent].group) {
                let shared = self.cx.shared;
                self.drop_parent(parent, &mut shared.groups());
                ended = true;
                continue;
            }
            while self.buffer.len() < self.capacity {
                let Some(source) = self.parents[parent].source.as_mut() else {
                    break;
                };
                match source.as_mut().poll_next(cx) {
                    Poll::Pending => break,
                    Poll::Ready(None) => {
                        let shared = self.cx.shared;
                        self.end_source(parent, &mut shared.groups());
                        ended = true;
                    }
                    Poll::Ready(Some(item)) => {
                        let terminal = self.terminal_error(&item);
                        if let Some((beneath, error)) = &terminal {
                            let buffered = self.buffer.iter().filter(|(q, _)| *q == parent).count();
                            self.halt_at(
                                parent,
                                self.parents[parent].next_index + buffered as u32,
                                beneath,
                                error,
                                self.parents[parent].group,
                            );
                        }
                        self.buffer.push((parent, item));
                        self.parents[parent].buffered += 1;
                        if terminal.is_some() {
                            let shared = self.cx.shared;
                            self.end_source(parent, &mut shared.groups());
                            ended = true;
                        }
                    }
                }
            }
        }
        self.count_live();
        ended || !self.buffer.is_empty()
    }

    fn make_turn(&mut self, column: &mut Column<'a>) -> Option<usize> {
        if self.buffer.is_empty() {
            return None;
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
        let mut buffer = std::mem::take(&mut self.buffer);
        // One range per parent, in parent order, its items in list order:
        // the stable sort keeps each parent's items as they were pulled.
        buffer.sort_by_key(|(p, _)| *p);
        let mut values = Vec::new();
        let mut value_pos = Vec::new();
        let mut failed = Vec::new();
        let mut inner_len = 0;
        let turn = &mut column.turns[turn_index];
        let mut drained = Vec::new();
        let mut items = buffer.into_iter().peekable();
        while let Some(&(p, _)) = items.peek() {
            let mut run = Vec::new();
            while let Some((_, item)) = items.next_if(|(q, _)| *q == p) {
                run.push(item);
            }
            let items = run;
            let len = items.len() as u32;
            let start_slot = if depth == 1 {
                inner_len += len;
                inner_len - len
            } else {
                let start = turn.levels[0].len() as u32;
                turn.levels[0].extend(std::iter::repeat_n(Slot::Pending, len as usize));
                start
            };
            let object = self.parents[p].position.object;
            self.parents[p].unshipped += 1;
            self.parents[p].buffered = 0;
            drained.push(p);
            turn.ranges.push(TurnRange {
                object,
                parent: p as u32,
                start_index: self.parents[p].next_index,
                start_slot,
                len,
                shipped: false,
            });
            for (j, item) in items.into_iter().enumerate() {
                let pos = Pos {
                    slot: start_slot + j as u32,
                    object,
                    indices: smallvec::smallvec![self.parents[p].next_index + j as u32],
                };
                match item {
                    Ok(value) => {
                        values.push(value);
                        value_pos.push(pos);
                    }
                    Err(error) => failed.push((pos, error)),
                }
            }
            self.parents[p].next_index += len;
        }
        {
            // Completing the items below takes the table itself.
            let shared = self.cx.shared;
            let mut table = shared.groups();
            for p in drained {
                self.update_live(p, &mut table);
            }
        }
        let mut cx = self.cx.clone();
        cx.groups = self.turn_groups.clone();
        let batch: Box<dyn TurnBatch + 'a> = Box::new(TurnItems::<T, Ty, C> {
            cx,
            field: self.field,
            shape: self.shape,
            leaf: self.leaf.clone(),
            generation: self.generation + 1,
            turn: turn_index as u32,
            objects,
            inner_len,
            items: Cell::new(Some((values, value_pos, failed))),
            keep: KeptOutputs::new(),
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
        self.count_live();
        Some(turn_index)
    }

    fn is_done(&self) -> bool {
        self.live_sources == 0 && self.buffer.is_empty()
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
        let _ = groups;
        for parent in &self.parents {
            if !parent.live {
                continue;
            }
            f(parent.group)?;
            for &carried in &parent.carried {
                f(carried)?;
            }
        }
        ControlFlow::Continue(())
    }

    fn parent_count(&self) -> usize {
        self.parents.len()
    }

    fn group(&self, parent: usize) -> GroupId {
        self.parents[parent].group
    }

    fn parent_object(&self, parent: usize) -> u32 {
        self.parents[parent].position.object
    }

    fn parent_of(&self, object: u32) -> Option<usize> {
        match self.by_object[object as usize] {
            u32::MAX => None,
            p => Some(p as usize),
        }
    }

    fn source_ended(&self, parent: usize) -> bool {
        self.parents[parent].source.is_none() && !self.buffer.iter().any(|(q, _)| *q == parent)
    }

    fn range_shipped(&mut self, parent: usize) {
        let state = &mut self.parents[parent];
        state.unshipped -= 1;
        if state.unshipped == 0 && state.source.is_none() {
            self.queue_completion(parent);
        }
    }

    fn take_completable(&mut self) -> Vec<usize> {
        let mut parents = std::mem::take(&mut self.completable);
        parents.retain(|&p| {
            self.parents[p].queued = false;
            // Buffered items still make a turn whose shipping asks again.
            self.parents[p].unshipped == 0 && self.source_ended(p)
        });
        parents.sort_unstable();
        parents
    }

    fn queue_completion(&mut self, parent: usize) {
        StreamState::queue_completion(self, parent);
    }

    fn release(&mut self) {
        if !std::mem::replace(&mut self.released, true) {
            self.count_live();
        }
    }

    fn owns_groups(&self) -> bool {
        self.initial_count.is_some()
    }

    fn is_released(&self) -> bool {
        self.released
    }

    fn drop_parent(&mut self, parent: usize, table: &mut Groups) {
        self.buffer.retain(|(q, _)| *q != parent);
        self.parents[parent].buffered = 0;
        self.end_source(parent, table);
        self.count_live();
    }

    fn release_holds(&mut self, table: &mut Groups) {
        if std::mem::replace(&mut self.holds_released, true) {
            return;
        }
        #[cfg(debug_assertions)]
        crate::__private::STREAM_PARENTS_VISITED.fetch_add(self.parents.len(), Relaxed);
        for p in 0..self.parents.len() {
            if self.parents[p].live {
                self.parents[p].live = false;
                let (group, carried) = (self.parents[p].group, self.parents[p].carried.clone());
                for g in std::iter::once(group).chain(carried) {
                    table.unhold(g);
                }
            }
        }
    }
}
