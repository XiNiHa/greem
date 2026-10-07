use crate::error::PathSegment;
use crate::exec::column::Column;
use crate::exec::state::{GroupId, GroupState, Shared};
use crate::plan::PlanId;
use crate::tree::UsageId;
use futures::future::BoxFuture;
use std::task::{Context as TaskContext, Poll};

pub type FieldFuture<'a> = BoxFuture<'a, Column<'a>>;

#[derive(Clone, Debug)]
pub struct ObjectMeta {
    pub group: GroupId,
    pub parent: u32,
    pub indices: Vec<u32>,
    pub path: Vec<PathSegment>,
    /// Deferred group instances this object participates in: (usage, group).
    pub pending: Vec<(UsageId, GroupId)>,
    /// The groups of the field sets several of those fragments share: (set, group).
    pub shared: Vec<(usize, GroupId)>,
}

pub struct ScopeMeta {
    pub entry: PlanId,
    pub generation: u32,
    pub objects: Vec<ObjectMeta>,
    /// Mutation root: resolve fields one at a time, each subtree to completion.
    pub serial: bool,
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
    pub meta: &'a ScopeMeta,
    pub shared: &'a Shared,
    /// Which field set of the Plan entry this scope runs (0 = immediate).
    pub set: usize,
    /// Per-object delivery group.
    pub groups: Vec<GroupId>,
    pub fields: Vec<FieldState<'a>>,
    pub cursor: usize,
    pub deferred: Vec<DeferredSet<'a>>,
    /// Per object: reachable from a delivered payload (not nulled away).
    pub alive: Vec<bool>,
    pub activity: Activity,
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        if let Ok(mut table) = self.shared.groups.lock() {
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
    pub fn new(
        meta: &'a ScopeMeta,
        shared: &'a Shared,
        set: usize,
        groups: Vec<GroupId>,
        futures: Vec<FieldFuture<'a>>,
        deferred: Vec<DeferredSet<'a>>,
    ) -> Self {
        let n = meta.objects.len();
        {
            let mut table = shared.groups.lock().unwrap();
            for &g in groups
                .iter()
                .chain(deferred.iter().flat_map(|d| d.groups.iter()))
            {
                table.retain(g);
            }
        }
        Self {
            meta,
            shared,
            set,
            groups,
            fields: futures.into_iter().map(FieldState::Pending).collect(),
            cursor: 0,
            deferred,
            alive: vec![false; n],
            activity: Activity::Fresh,
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

    /// Advances this scope's subtree by one generation. `Ready(progress)` when
    /// nothing under it is pending; `progress` says whether a future completed
    /// or a stream has items waiting for a turn.
    pub fn poll_generation(&mut self, cx: &mut TaskContext<'_>) -> Poll<bool> {
        if self.activity == Activity::Quiescent {
            return Poll::Ready(false);
        }
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
        for column in self.columns_mut() {
            for turn in &mut column.turns {
                if turn.retired {
                    continue;
                }
                for child in &mut turn.children {
                    let fresh = child.with_dependent(|_, scope| scope.activity == Activity::Fresh);
                    if fresh {
                        continue;
                    }
                    match child.with_dependent_mut(|_, scope| scope.poll_generation(cx)) {
                        Poll::Ready(p) => progress |= p,
                        Poll::Pending => pending = true,
                    }
                }
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
                match scope.poll_generation(cx) {
                    Poll::Ready(p) => progress |= p,
                    Poll::Pending => pending = true,
                }
            }
        }
        if pending {
            Poll::Pending
        } else {
            Poll::Ready(progress)
        }
    }

    /// The subtree of one column has no unfinished work for `group`.
    pub fn column_live_for(&self, field: u32, group: GroupId) -> bool {
        match self.column(field) {
            Some(column) => {
                column.stream.as_ref().is_some_and(|d| d.is_live_for(group))
                    || column.turns.iter().any(|turn| {
                        turn.children
                            .iter()
                            .any(|c| c.with_dependent(|_, s| s.is_live_for(group)))
                    })
            }
            None => true,
        }
    }

    /// A released stream that may still yield items exists under this scope.
    pub fn has_live_streams(&self) -> bool {
        if self.activity == Activity::Quiescent {
            return false;
        }
        for column in self.columns() {
            if column
                .stream
                .as_ref()
                .is_some_and(|d| d.is_released() && !d.is_done())
            {
                return true;
            }
            for turn in &column.turns {
                if turn
                    .children
                    .iter()
                    .any(|c| c.with_dependent(|_, s| s.has_live_streams()))
                {
                    return true;
                }
            }
        }
        self.deferred
            .iter()
            .any(|d| d.scope().is_some_and(|s| s.has_live_streams()))
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
            for turn in &column.turns {
                if turn
                    .children
                    .iter()
                    .any(|c| c.with_dependent(|_, s| !s.is_parked()))
                {
                    return false;
                }
            }
        }
        self.deferred
            .iter()
            .all(|d| d.scope().is_none_or(|s| s.is_parked()))
    }

    /// True when nothing under this scope can still run: every future is
    /// done, every stream ended, and no deferred set is waiting.
    pub fn is_finished(&self) -> bool {
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
                    || !self.groups_settled(driver.groups()))
            {
                return false;
            }
            for turn in &column.turns {
                if turn
                    .children
                    .iter()
                    .any(|c| c.with_dependent(|_, s| !s.is_finished()))
                {
                    return false;
                }
            }
        }
        // A released deferred scope whose group has not shipped still has to be
        // read by a later barrier, so the subtree is not finished yet. The
        // recursion runs before the lock is taken: the mutex is not reentrant.
        self.deferred.iter().all(|d| match &d.state {
            DeferredSetState::Waiting(_) => false,
            DeferredSetState::Running(s) => s.is_finished() && self.groups_terminal(&d.groups),
            DeferredSetState::Dropped => true,
        })
    }

    /// Every group completed or can no longer deliver anything.
    fn groups_settled(&self, groups: &[GroupId]) -> bool {
        let table = self.shared.groups.lock().unwrap();
        groups
            .iter()
            .all(|&g| table.get(g).state == GroupState::Completed || table.is_dead(g))
    }

    fn groups_terminal(&self, groups: &[GroupId]) -> bool {
        let table = self.shared.groups.lock().unwrap();
        groups.iter().all(|&g| {
            matches!(
                table.get(g).state,
                GroupState::Completed | GroupState::Failed | GroupState::Dropped
            )
        })
    }

    /// True when the objects of `group` under this scope have unfinished work.
    pub fn is_live_for(&self, group: GroupId) -> bool {
        if self.activity == Activity::Quiescent {
            return false;
        }
        let has_group = self.groups.contains(&group);
        if has_group {
            if self.meta.serial {
                if self.cursor < self.fields.len() {
                    return true;
                }
            } else if self
                .fields
                .iter()
                .any(|f| matches!(f, FieldState::Pending(_)))
            {
                return true;
            }
        }
        for column in self.columns() {
            if let Some(driver) = &column.stream
                && driver.is_live_for(group)
            {
                return true;
            }
            for turn in &column.turns {
                if turn.retired {
                    continue;
                }
                if turn
                    .children
                    .iter()
                    .any(|c| c.with_dependent(|_, s| s.is_live_for(group)))
                {
                    return true;
                }
            }
        }
        self.deferred.iter().any(|d| match &d.state {
            DeferredSetState::Waiting(_) => d.groups.contains(&group),
            DeferredSetState::Running(scope) => scope.is_live_for(group),
            DeferredSetState::Dropped => false,
        })
    }

    /// Every object of this scope, and every deferred set under it, is dead.
    pub fn all_dead(&self, shared: &Shared) -> bool {
        let groups = shared.groups.lock().unwrap();
        self.groups
            .iter()
            .chain(self.deferred.iter().flat_map(|d| d.groups.iter()))
            .all(|&g| groups.is_dead(g))
    }

    pub fn clear_fresh(&mut self) {
        if self.activity == Activity::Fresh {
            self.activity = Activity::Active;
        }
        for column in self.columns_mut() {
            for turn in &mut column.turns {
                for child in &mut turn.children {
                    child.with_dependent_mut(|_, scope| scope.clear_fresh());
                }
            }
        }
        for deferred in &mut self.deferred {
            if let Some(scope) = deferred.scope_mut() {
                scope.clear_fresh();
            }
        }
    }
}
