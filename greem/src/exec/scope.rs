use crate::error::PathSegment;
use crate::exec::column::Column;
use crate::exec::payload::Step;
use crate::exec::state::{GroupId, GroupState, Groups, Shared};
use crate::plan::PlanId;
use crate::tree::UsageId;
use futures::future::BoxFuture;
use futures::task::AtomicWaker;
use smallvec::SmallVec;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::task::{Context as TaskContext, Poll, Wake, Waker};

pub type FieldFuture<'a> = BoxFuture<'a, Column<'a>>;

#[derive(Clone, Debug)]
pub struct ObjectMeta {
    pub group: GroupId,
    /// The parent scope's object this one hangs off, and the list indices
    /// below it.
    pub parent: u32,
    pub indices: SmallVec<[u32; 4]>,
    /// Deferred group instances this object participates in: (usage, group).
    pub pending: SmallVec<[(UsageId, GroupId); 2]>,
    /// The groups of the field sets several of those fragments share: (set, group).
    pub shared: Vec<(usize, GroupId)>,
}

pub struct ScopeMeta<'a> {
    pub entry: PlanId,
    pub generation: u32,
    pub objects: Vec<ObjectMeta>,
    /// Mutation root: resolve fields one at a time, each subtree to completion.
    pub serial: bool,
    /// `None` at the root.
    pub parent: Option<ParentLink<'a>>,
}

/// Where a scope hangs in the response: the parent scope's objects and the
/// key of the field whose outputs it holds.
#[derive(Clone, Copy)]
pub struct ParentLink<'a> {
    pub meta: &'a ScopeMeta<'a>,
    pub key: &'a str,
}

impl ScopeMeta<'_> {
    /// The response path of `object`, rebuilt from the parent links.
    pub fn path(&self, object: u32) -> Vec<PathSegment> {
        let mut path = Vec::with_capacity(self.depth(object));
        self.push_path(object, &mut path);
        path
    }

    /// The path of `object`'s field `key`, then the list `indices` below it.
    pub fn path_to(&self, object: u32, key: &str, indices: &[u32]) -> Vec<PathSegment> {
        let mut path = Vec::with_capacity(self.depth(object) + 1 + indices.len());
        self.push_path(object, &mut path);
        push_field(&mut path, key, indices);
        path
    }

    /// Whether `object`'s path is exactly `expected`, without building it.
    pub fn has_path(&self, object: u32, expected: &[PathSegment]) -> bool {
        self.depth(object) == expected.len() && self.matches_prefix(object, expected).is_some()
    }

    /// The length of the prefix of `expected` that is `object`'s path, if
    /// the path is a prefix of it.
    fn matches_prefix(&self, object: u32, expected: &[PathSegment]) -> Option<usize> {
        let Some(link) = self.parent else {
            return Some(0);
        };
        let o = &self.objects[object as usize];
        let mut at = link.meta.matches_prefix(o.parent, expected)?;
        match expected.get(at) {
            Some(PathSegment::Key(key)) if key == link.key => at += 1,
            _ => return None,
        }
        for &index in &o.indices {
            match expected.get(at) {
                Some(PathSegment::Index(i)) if *i == index as usize => at += 1,
                _ => return None,
            }
        }
        Some(at)
    }

    /// The length of `object`'s path.
    pub fn depth(&self, object: u32) -> usize {
        self.parent.map_or(0, |link| {
            let o = &self.objects[object as usize];
            link.meta.depth(o.parent) + 1 + o.indices.len()
        })
    }

    fn push_path(&self, object: u32, path: &mut Vec<PathSegment>) {
        if let Some(link) = self.parent {
            let o = &self.objects[object as usize];
            link.meta.push_path(o.parent, path);
            push_field(path, link.key, &o.indices);
        }
    }
}

fn push_field(path: &mut Vec<PathSegment>, key: &str, indices: &[u32]) {
    path.push(PathSegment::Key(key.to_owned()));
    path.extend(indices.iter().map(|&i| PathSegment::Index(i as usize)));
}

/// A future or source beneath the scope woke, or a scope beneath it is new:
/// the next poll descends here.
pub const POLL: u8 = 1;
/// An object beneath the scope was decided at this barrier: its pending
/// groups are announced or dropped.
pub const ANNOUNCE: u8 = 2;
/// A stream beneath the scope has something for the barrier: a new turn, a
/// source that ended, a halt, or a child scope that finished.
pub const STREAM: u8 = 4;

/// The flags of one scope. Raising a flag raises it on every ancestor too,
/// so a pass over the tree descends only into subtrees where something
/// changed, clearing the flags as it goes. The root's flag doubles as the
/// executor's waker: raising `POLL` anywhere wakes the request.
pub struct Signal {
    bits: AtomicU8,
    parent: Option<Arc<Signal>>,
    /// The request's waker; only the root's is registered.
    outer: AtomicWaker,
}

impl Signal {
    pub fn root() -> Arc<Self> {
        Arc::new(Signal {
            bits: AtomicU8::new(POLL),
            parent: None,
            outer: AtomicWaker::new(),
        })
    }

    pub fn child(parent: &Arc<Signal>) -> Arc<Self> {
        Arc::new(Signal {
            bits: AtomicU8::new(POLL),
            parent: Some(parent.clone()),
            outer: AtomicWaker::new(),
        })
    }

    pub fn raise(&self, bits: u8) {
        let mut node = self;
        loop {
            node.bits.fetch_or(bits, Ordering::Relaxed);
            match &node.parent {
                Some(parent) => node = parent,
                None => {
                    if bits & POLL != 0 {
                        node.outer.wake();
                    }
                    return;
                }
            }
        }
    }

    /// Clears `bit` here and reports whether it was set.
    pub fn take(&self, bit: u8) -> bool {
        self.bits.fetch_and(!bit, Ordering::Relaxed) & bit != 0
    }
}

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.raise(POLL);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.raise(POLL);
    }
}

/// The owner half of a frame: the completed output batch a scope borrows.
pub trait Batch: Send {
    fn start<'this>(&'this self) -> Scope<'this>;
}

self_cell::self_cell!(
    pub struct Frame<'a> {
        owner: Box<dyn Batch + 'a>,
        #[not_covariant]
        dependent: Scope,
    }
);

impl<'a> Frame<'a> {
    pub fn from_batch(owner: Box<dyn Batch + 'a>) -> Self {
        Frame::new(owner, |owner| owner.start())
    }
}

pub enum FieldState<'a> {
    Pending(FieldFuture<'a>),
    Done(Column<'a>),
}

pub type Starter<'a> = Box<dyn FnOnce() -> Vec<FieldFuture<'a>> + Send + 'a>;

/// A deferred field set over the same objects: it waits for its groups'
/// release, then runs as a scope of its own.
pub struct DeferredSet<'a> {
    pub set: usize,
    pub groups: Vec<GroupId>,
    pub state: DeferredSetState<'a>,
    /// Holds its groups while waiting: a released fragment cannot complete
    /// before its field set has run.
    pub held: bool,
    /// Per object: nulled away before its fragment shipped, so the set
    /// delivers nothing for it and no longer holds its group.
    pub excluded: Vec<bool>,
    /// The scope's signal once it runs; its field futures wake through it.
    pub signal: Arc<Signal>,
}

impl DeferredSet<'_> {
    pub fn release_hold(&mut self, table: &mut Groups) {
        if std::mem::replace(&mut self.held, false) {
            for (o, &g) in self.groups.iter().enumerate() {
                if !self.excluded[o] {
                    table.unhold(g);
                }
            }
        }
    }

    /// `object` was nulled: its deferred fields can no longer be delivered.
    pub fn exclude(&mut self, object: u32, table: &mut Groups) {
        let o = object as usize;
        if std::mem::replace(&mut self.excluded[o], true) {
            return;
        }
        if self.held {
            table.unhold(self.groups[o]);
        }
    }

    /// The objects the set still delivers for, with their groups.
    pub fn live_objects(&self) -> impl Iterator<Item = (u32, GroupId)> + '_ {
        self.groups
            .iter()
            .enumerate()
            .filter(|&(o, _)| !self.excluded[o])
            .map(|(o, &g)| (o as u32, g))
    }
}

pub enum DeferredSetState<'a> {
    Waiting(Starter<'a>),
    Running(Box<Scope<'a>>),
    /// Every group it runs under died before release: it never starts.
    Dropped,
}

impl<'a> DeferredSet<'a> {
    pub fn scope(&self) -> Option<&Scope<'a>> {
        match &self.state {
            DeferredSetState::Running(scope) => Some(scope),
            _ => None,
        }
    }

    pub fn scope_mut(&mut self) -> Option<&mut Scope<'a>> {
        match &mut self.state {
            DeferredSetState::Running(scope) => Some(scope),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// Created in this generation; polled from the next one.
    Fresh,
    Active,
    /// Nothing under this scope can change any more; polls and liveness
    /// checks skip it. Final.
    Quiescent,
}

pub struct Scope<'a> {
    pub meta: &'a ScopeMeta<'a>,
    pub shared: &'a Shared,
    /// Where this scope hangs in the tree, from the root.
    pub path: Arc<[Step]>,
    pub signal: Arc<Signal>,
    waker: Waker,
    /// What the last poll found: a future beneath still pending.
    pending_beneath: bool,
    /// Which field set of the Plan entry this scope runs (0 = immediate).
    pub set: usize,
    /// Per-object delivery group.
    pub groups: Vec<GroupId>,
    pub fields: Vec<FieldState<'a>>,
    pub cursor: usize,
    pub deferred: Vec<DeferredSet<'a>>,
    /// Per object: reachable from a delivered payload (not nulled away).
    pub alive: Vec<bool>,
    /// Objects whose group shipped at this barrier, alive or nulled; their
    /// pending groups are announced or dropped, then the list is cleared.
    pub decided: Vec<u32>,
    pub activity: Activity,
    /// Holds every object's group while this scope has unfinished fields.
    working_held: bool,
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        if let Some(mut table) = self.shared.groups_for_drop() {
            self.release_holds(&mut table);
            for &g in self
                .groups
                .iter()
                .chain(self.deferred.iter().flat_map(|d| d.groups.iter()))
            {
                table.release_ref(g);
            }
        }
    }
}

impl<'a> Scope<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        meta: &'a ScopeMeta<'a>,
        shared: &'a Shared,
        path: Arc<[Step]>,
        signal: Arc<Signal>,
        set: usize,
        groups: Vec<GroupId>,
        futures: Vec<FieldFuture<'a>>,
        deferred: Vec<DeferredSet<'a>>,
    ) -> Self {
        let n = meta.objects.len();
        let working = !futures.is_empty();
        let mut deferred = deferred;
        {
            let mut table = shared.groups();
            for &g in groups
                .iter()
                .chain(deferred.iter().flat_map(|d| d.groups.iter()))
            {
                table.retain(g);
            }

            if working {
                for &g in &groups {
                    table.hold(g);
                }
            }
            for d in &mut deferred {
                if matches!(d.state, DeferredSetState::Waiting(_)) && !d.held {
                    d.held = true;
                    for &g in &d.groups {
                        table.hold(g);
                    }
                }
            }
        }
        Self {
            meta,
            shared,
            path,
            waker: Waker::from(signal.clone()),
            signal,
            pending_beneath: false,
            set,
            groups,
            fields: futures.into_iter().map(FieldState::Pending).collect(),
            cursor: 0,
            deferred,
            alive: vec![false; n],
            decided: Vec::new(),
            activity: Activity::Fresh,
            working_held: working,
        }
    }

    /// Whether a field of this scope's own has yet to resolve.
    pub fn working(&self) -> bool {
        if self.meta.serial {
            self.cursor < self.fields.len()
        } else {
            self.fields
                .iter()
                .any(|f| matches!(f, FieldState::Pending(_)))
        }
    }

    /// Drops the holds of this scope's own fields once they resolved. A
    /// stream range above may be ready now.
    pub fn release_work(&mut self, table: &mut Groups) {
        if self.working_held && !self.working() {
            self.working_held = false;
            for &g in &self.groups {
                table.unhold(g);
            }
            self.signal.raise(STREAM);
        }
    }

    /// Drops every hold under this scope: it is quiescent or being dropped,
    /// so nothing beneath it counts as unfinished work any more.
    pub fn release_holds(&mut self, table: &mut Groups) {
        if std::mem::replace(&mut self.working_held, false) {
            for &g in &self.groups {
                table.unhold(g);
            }
            self.signal.raise(STREAM);
        }
        for d in &mut self.deferred {
            d.release_hold(table);
        }
        for column in self.columns_mut() {
            if let Some(driver) = &mut column.stream {
                driver.release_holds(table);
            }
        }
    }

    pub fn columns(&self) -> impl Iterator<Item = &Column<'a>> {
        self.fields.iter().filter_map(|f| match f {
            FieldState::Done(c) => Some(c),
            FieldState::Pending(_) => None,
        })
    }

    pub fn columns_mut(&mut self) -> impl Iterator<Item = &mut Column<'a>> {
        self.fields.iter_mut().filter_map(|f| match f {
            FieldState::Done(c) => Some(c),
            FieldState::Pending(_) => None,
        })
    }

    pub fn column(&self, field: u32) -> Option<&Column<'a>> {
        self.columns().find(|c| c.field == field)
    }

    /// Advances the tree by one generation from the root. `Ready(progress)`
    /// when nothing under it is pending; `progress` says whether a future
    /// completed or a stream has items waiting for a turn.
    pub fn poll_generation(&mut self, cx: &mut TaskContext<'_>) -> Poll<bool> {
        self.signal.outer.register(cx.waker());
        self.poll_scope()
    }

    /// Polls what woke beneath this scope. A scope nothing woke under
    /// reports what its last poll found.
    fn poll_scope(&mut self) -> Poll<bool> {
        if self.activity == Activity::Quiescent {
            return Poll::Ready(false);
        }
        if !self.signal.take(POLL) {
            return if self.pending_beneath {
                Poll::Pending
            } else {
                Poll::Ready(false)
            };
        }
        #[cfg(debug_assertions)]
        crate::__private::SCOPES_POLLED.fetch_add(1, Ordering::Relaxed);
        let waker = self.waker.clone();
        let cx = &mut TaskContext::from_waker(&waker);
        let mut progress = false;
        let mut pending = false;
        let serial = self.meta.serial;
        for i in 0..self.fields.len() {
            if serial && i != self.cursor {
                continue;
            }
            if let FieldState::Pending(future) = &mut self.fields[i] {
                match future.as_mut().poll(cx) {
                    Poll::Ready(column) => {
                        self.fields[i] = FieldState::Done(column);
                        progress = true;
                    }
                    Poll::Pending => pending = true,
                }
            }
        }
        if progress && self.working_held && !self.working() {
            let shared = self.shared;
            self.release_work(&mut shared.groups());
        }
        for column in self.columns_mut() {
            for turn in &mut column.turns {
                if turn.retired {
                    continue;
                }
                turn.each_child_mut(|scope| {
                    if scope.activity == Activity::Fresh {
                        return;
                    }
                    match scope.poll_scope() {
                        Poll::Ready(p) => progress |= p,
                        Poll::Pending => pending = true,
                    }
                });
            }
            if let Some(driver) = &mut column.stream {
                progress |= driver.pump(cx);
            }
        }
        for deferred in &mut self.deferred {
            if let Some(scope) = deferred.scope_mut() {
                if scope.activity == Activity::Fresh {
                    continue;
                }
                match scope.poll_scope() {
                    Poll::Ready(p) => progress |= p,
                    Poll::Pending => pending = true,
                }
            }
        }
        self.pending_beneath = pending;
        if pending {
            Poll::Pending
        } else {
            Poll::Ready(progress)
        }
    }

    /// The subtree of one column has no unfinished work for `group`.
    pub fn column_live_for(&self, field: u32, group: GroupId, groups: &Groups) -> bool {
        match self.column(field) {
            Some(column) => {
                column
                    .stream
                    .as_ref()
                    .is_some_and(|d| d.live_groups(groups, &mut finds(group)).is_break())
                    || column
                        .turns
                        .iter()
                        .any(|turn| turn.any_child(|s| s.is_live_for(group, groups)))
            }
            None => true,
        }
    }

    /// True when no future under this scope (same or nested group) is pending
    /// and no stream can still produce work.
    pub fn is_parked(&self) -> bool {
        if self.activity == Activity::Quiescent {
            return true;
        }
        if self.meta.serial {
            if self.cursor < self.fields.len() {
                return false;
            }
        } else if self
            .fields
            .iter()
            .any(|f| matches!(f, FieldState::Pending(_)))
        {
            return false;
        }
        for column in self.columns() {
            if column.stream.as_ref().is_some_and(|d| !d.is_done()) {
                return false;
            }
            if column
                .turns
                .iter()
                .any(|turn| turn.any_child(|s| !s.is_parked()))
            {
                return false;
            }
        }
        self.deferred
            .iter()
            .all(|d| d.scope().is_none_or(|s| s.is_parked()))
    }

    /// True when nothing under this scope can still run: every future is
    /// done, every stream ended, and no deferred set is waiting.
    pub fn is_finished(&self, groups: &Groups) -> bool {
        if self.activity == Activity::Quiescent {
            return true;
        }
        if !self.is_parked() {
            return false;
        }
        for column in self.columns() {
            // A stream's turns that have not shipped, and its groups that have
            // not completed, are still read by a later barrier.
            if let Some(driver) = column.stream.as_ref().filter(|d| d.owns_groups())
                && (column
                    .turns
                    .iter()
                    .skip(1)
                    .any(|turn| !turn.retired && !turn.shipped)
                    || !all_settled(groups, driver.groups()))
            {
                return false;
            }
            if column
                .turns
                .iter()
                .any(|turn| turn.any_child(|s| !s.is_finished(groups)))
            {
                return false;
            }
        }
        // A released deferred scope whose group has not shipped still has to be
        // read by a later barrier, so the subtree is not finished yet.
        self.deferred.iter().all(|d| match &d.state {
            DeferredSetState::Waiting(_) => false,
            DeferredSetState::Running(s) => {
                s.is_finished(groups) && all_terminal(groups, &d.groups)
            }
            DeferredSetState::Dropped => true,
        })
    }

    /// True when the objects of `group` under this scope have unfinished work.
    pub fn is_live_for(&self, group: GroupId, groups: &Groups) -> bool {
        self.live_groups(groups, &mut finds(group)).is_break()
    }

    /// Visits every group whose objects under this scope have unfinished
    /// work. A group may come more than once.
    pub fn live_groups(
        &self,
        groups: &Groups,
        f: &mut dyn FnMut(GroupId) -> ControlFlow<()>,
    ) -> ControlFlow<()> {
        if self.activity == Activity::Quiescent {
            return ControlFlow::Continue(());
        }
        if self.working() {
            for &g in &self.groups {
                f(g)?;
            }
        }
        for column in self.columns() {
            if let Some(driver) = &column.stream {
                driver.live_groups(groups, f)?;
            }
            for turn in &column.turns {
                if turn.retired {
                    continue;
                }
                turn.with_stored(|stored| {
                    for child in &stored.children {
                        child.with_dependent(|_, s| s.live_groups(groups, f))?;
                    }
                    ControlFlow::Continue(())
                })?;
            }
        }
        for d in &self.deferred {
            match &d.state {
                DeferredSetState::Waiting(_) => {
                    for (_, g) in d.live_objects() {
                        f(g)?;
                    }
                }
                DeferredSetState::Running(scope) => scope.live_groups(groups, f)?,
                DeferredSetState::Dropped => {}
            }
        }
        ControlFlow::Continue(())
    }

    /// Every object of this scope, and every deferred set under it, is dead.
    pub fn all_dead(&self, groups: &Groups) -> bool {
        self.groups
            .iter()
            .chain(self.deferred.iter().flat_map(|d| d.groups.iter()))
            .all(|&g| groups.is_dead(g))
    }
}

/// A `live_groups` visitor that stops at `group`.
fn finds(group: GroupId) -> impl FnMut(GroupId) -> ControlFlow<()> {
    move |g| {
        if g == group {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// Every group completed or can no longer deliver anything.
fn all_settled(table: &Groups, groups: impl IntoIterator<Item = GroupId>) -> bool {
    groups
        .into_iter()
        .all(|g| matches!(table.get(g).state, GroupState::Completed) || table.is_dead(g))
}

fn all_terminal(table: &Groups, groups: &[GroupId]) -> bool {
    groups.iter().all(|&g| {
        matches!(
            table.get(g).state,
            GroupState::Completed | GroupState::Failed(_) | GroupState::Dropped
        )
    })
}
