use crate::error::{GraphQLError, PathSegment};
use crate::plan::PlanTable;
use crate::tree::{NodeId, UsageId};
use std::collections::BinaryHeap;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

pub type GroupId = u32;

/// What an execution error does to the data around it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ErrorBehavior {
    /// Null the failed position only.
    Null,
    /// Propagate the null to the nearest nullable ancestor (the spec default).
    #[default]
    Propagate,
    /// Abort the operation with `data: null` and a single error.
    Halt,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IncrementalDelivery {
    #[default]
    Disabled,
    Enabled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExecuteOptions {
    pub error_behavior: ErrorBehavior,
    pub incremental: IncrementalDelivery,
}

#[derive(Clone, Debug)]
pub enum GroupKind {
    Initial,
    Defer {
        usage: UsageId,
        label: Option<String>,
        path: Vec<PathSegment>,
        /// A fragment nested in another one but delivered inside stream
        /// items: its position comes from `parent` (the stream group), while
        /// the enclosing fragment's group must also have completed first.
        after: Option<GroupId>,
    },
    Stream {
        node: NodeId,
        label: Option<String>,
        path: Vec<PathSegment>,
        /// The parent's index in the stream driver that owns this group.
        parent: u32,
    },
    /// A shared field set: several deferred fragments of one object select it. It has
    /// no wire identity: it runs once any member is released, ships with the
    /// first member that completes, and only its own failure fails it (and
    /// then every member), never the failure of one member.
    Shared {
        members: Vec<GroupId>,
    },
}

#[derive(Clone, Debug)]
pub enum GroupState {
    /// Created but its parent payload has not shipped yet.
    Unreleased,
    /// Its `pending` entry is in the payload being shipped; released once the
    /// sink returns.
    Announced,
    /// Its scopes may run; not yet complete.
    Released,
    /// HALT recorded this error for it: the next barrier fails it without
    /// waiting for its pending work.
    Halted(GraphQLError),
    /// Its final payload has shipped.
    Completed,
    /// Failed (boundary propagation or HALT); its `completed` entry shipped.
    /// The fragments that depend on it fail with the same error.
    Failed(Option<GraphQLError>),
    /// Dropped before running: parent position nulled or ancestor failed.
    Dropped,
}

#[derive(Debug)]
pub struct Group {
    pub kind: GroupKind,
    pub parent: Option<GroupId>,
    /// Written only through `Groups::set_state`, which keeps the counts and
    /// event lists below in step.
    pub state: GroupState,
    pub wire_id: Option<u32>,
    /// Live references from objects, scopes, drivers and child groups; a
    /// terminal group with none is reclaimed at the next barrier.
    pub refs: u32,
    /// The live shared field sets this group is a member of, by ascending id.
    sharing: Vec<GroupId>,
    /// The groups whose fate follows this one's: its children, and the
    /// fragments whose `after` dependency it is. Unordered.
    dependents: Vec<GroupId>,
    /// This group's positions in its parent's and its `after` group's
    /// `dependents`, so leaving them is O(1).
    parent_slot: u32,
    after_slot: u32,
    reclaim_queued: bool,
    /// Listed in `Groups::died`, not yet settled by a barrier.
    died_pending: bool,
}

/// The group table, with what changed since each consumer last looked:
/// the barrier and `advance` read the lists instead of scanning every group.
#[derive(Default, Debug)]
pub struct Groups {
    /// `None` is a reclaimed slot, reused by the next allocation.
    list: Vec<Option<Group>>,
    next_wire: u32,
    free: Vec<GroupId>,
    /// Groups that are Announced, Released or Halted: the response has a
    /// next payload while any exist.
    open: u32,
    /// Entered Announced since the last `advance`.
    announced: Vec<GroupId>,
    /// Became Failed or Dropped since the barrier last settled dependents.
    died: Vec<GroupId>,
    /// Entered Halted since the last barrier.
    halted: Vec<GroupId>,
    /// May be reclaimable: references reached zero, became terminal, or
    /// an ancestor died.
    reclaim: Vec<GroupId>,
}

fn is_open(state: &GroupState) -> bool {
    matches!(
        state,
        GroupState::Announced | GroupState::Released | GroupState::Halted(_)
    )
}

fn is_terminal(state: &GroupState) -> bool {
    matches!(
        state,
        GroupState::Completed | GroupState::Failed(_) | GroupState::Dropped
    )
}

impl Groups {
    pub fn new() -> Self {
        let mut groups = Groups::default();
        groups.list.push(Some(Group {
            kind: GroupKind::Initial,
            parent: None,
            state: GroupState::Released,
            wire_id: None,
            refs: 0,
            sharing: Vec::new(),
            dependents: Vec::new(),
            parent_slot: 0,
            after_slot: 0,
            reclaim_queued: false,
            died_pending: false,
        }));
        groups.open = 1;
        groups
    }

    pub fn alloc(&mut self, kind: GroupKind, parent: GroupId) -> GroupId {
        self.retain(parent);
        let after = match kind {
            GroupKind::Defer {
                after: Some(after), ..
            } => {
                self.retain(after);
                Some(after)
            }
            _ => None,
        };
        let id = self.free.pop().unwrap_or(self.list.len() as GroupId);
        if let GroupKind::Shared { members } = &kind {
            for &member in members {
                let group = self.get_mut(member);
                group.refs += 1;
                let at = group.sharing.partition_point(|&s| s < id);
                group.sharing.insert(at, id);
            }
        }
        let parent_slot = self.add_dependent(parent, id);
        let after_slot = after.map_or(0, |after| self.add_dependent(after, id));
        let group = Group {
            kind,
            parent: Some(parent),
            state: GroupState::Unreleased,
            wire_id: None,
            refs: 0,
            sharing: Vec::new(),
            dependents: Vec::new(),
            parent_slot,
            after_slot,
            reclaim_queued: false,
            died_pending: false,
        };
        if id as usize == self.list.len() {
            self.list.push(Some(group));
        } else {
            self.list[id as usize] = Some(group);
        }
        crate::__private::MAX_LIVE_GROUPS.fetch_max(
            self.list.len() - self.free.len(),
            std::sync::atomic::Ordering::Relaxed,
        );
        id
    }

    fn add_dependent(&mut self, of: GroupId, id: GroupId) -> u32 {
        let dependents = &mut self.get_mut(of).dependents;
        dependents.push(id);
        dependents.len() as u32 - 1
    }

    /// Takes `id` out of `of`'s dependents by swapping the last one into its
    /// slot and telling that group where it moved.
    fn remove_dependent(&mut self, of: GroupId, id: GroupId, slot: u32) {
        let dependents = &mut self.get_mut(of).dependents;
        debug_assert_eq!(dependents[slot as usize], id);
        dependents.swap_remove(slot as usize);
        if let Some(&moved) = dependents.get(slot as usize) {
            let moved = self.get_mut(moved);
            if moved.parent == Some(of) {
                moved.parent_slot = slot;
            } else {
                moved.after_slot = slot;
            }
        }
    }

    /// Changes a group's state, recording the change for the consumers
    /// that act on it: the open count, and the announced, died, halted and
    /// reclaim lists.
    pub fn set_state(&mut self, id: GroupId, state: GroupState) {
        let group = self.get_mut(id);
        let was_open = is_open(&group.state);
        let was_terminal = is_terminal(&group.state);
        let now_open = is_open(&state);
        let now_terminal = is_terminal(&state);
        group.state = state;
        match (was_open, now_open) {
            (false, true) => self.open += 1,
            (true, false) => self.open -= 1,
            _ => {}
        }
        match self.get(id).state {
            GroupState::Announced => self.announced.push(id),
            GroupState::Halted(_) => self.halted.push(id),
            GroupState::Failed(_) | GroupState::Dropped if !was_terminal => {
                self.died.push(id);
                self.get_mut(id).died_pending = true;
            }
            _ => {}
        }
        if now_terminal && !was_terminal {
            self.queue_reclaim(id);
            // A shared field set it belongs to may have waited for it.
            for s in self.get(id).sharing.clone() {
                self.queue_reclaim(s);
            }
        }
    }

    /// Whether any group is announced, released or halted.
    pub fn any_open(&self) -> bool {
        self.open > 0
    }

    /// The groups that entered Announced since the last call.
    pub fn take_announced(&mut self) -> Vec<GroupId> {
        std::mem::take(&mut self.announced)
    }

    /// The groups that became Failed or Dropped since the last call.
    pub fn take_died(&mut self) -> Vec<GroupId> {
        let died = std::mem::take(&mut self.died);
        for &g in &died {
            self.get_mut(g).died_pending = false;
        }
        died
    }

    /// The groups that entered Halted since the last barrier.
    pub fn halted(&self) -> &[GroupId] {
        &self.halted
    }

    pub fn clear_halted(&mut self) {
        self.halted.clear();
    }

    /// The groups whose fate follows `id`'s, transitively: its children and
    /// the fragments delivered after it, in no particular order.
    pub fn descendants(&self, id: GroupId) -> Vec<GroupId> {
        let mut found = Vec::new();
        let mut stack = vec![id];
        while let Some(g) = stack.pop() {
            for &d in &self.get(g).dependents {
                found.push(d);
                stack.push(d);
            }
        }
        found
    }

    pub fn retain(&mut self, id: GroupId) {
        self.get_mut(id).refs += 1;
    }

    /// Drops one reference; "release" alone means a group's activation.
    pub fn release_ref(&mut self, id: GroupId) {
        let group = self.get_mut(id);
        group.refs = group.refs.saturating_sub(1);
        if group.refs == 0 {
            self.queue_reclaim(id);
        }
    }

    /// Has `sweep` look at `id`: a group whose own state did not change may
    /// still have become reclaimable through a reference or an ancestor.
    pub fn queue_reclaim(&mut self, id: GroupId) {
        let group = self.get_mut(id);
        if !std::mem::replace(&mut group.reclaim_queued, true) {
            self.reclaim.push(id);
        }
    }

    /// Frees every finished group with no references left, repeating while a
    /// freed child releases its parent. A group under a failed or dropped
    /// ancestor never runs, so it counts as finished whatever its own state.
    /// The initial group is never freed.
    ///
    /// Only queued groups are looked at, in the order a scan would visit
    /// them: ascending by id, a group queued during a pass joining that pass
    /// if it lies ahead and the next one otherwise, so slots are reused in
    /// the same order a scan reuses them.
    pub fn sweep(&mut self) {
        let mut next: Vec<GroupId> = std::mem::take(&mut self.reclaim);
        while !next.is_empty() {
            let mut pass: BinaryHeap<std::cmp::Reverse<GroupId>> =
                next.drain(..).map(std::cmp::Reverse).collect();
            while let Some(std::cmp::Reverse(id)) = pass.pop() {
                let Some(group) = &mut self.list[id as usize] else {
                    continue;
                };
                group.reclaim_queued = false;
                if id == 0 {
                    continue;
                }
                if group.refs != 0 || !(is_terminal(&group.state) || self.is_dead(id)) {
                    continue;
                }
                let group = self.get(id);
                // A shared field set outlives its objects while a member
                // fragment has yet to complete: that member reads its outcome.
                if let GroupKind::Shared { members } = &group.kind
                    && members.iter().any(|&m| {
                        matches!(
                            self.get(m).state,
                            GroupState::Unreleased | GroupState::Announced | GroupState::Released
                        ) && !self.is_dead(m)
                    })
                {
                    continue;
                }
                let queued_before = self.reclaim.len();
                self.free_group(id);
                for q in self.reclaim.drain(queued_before..).collect::<Vec<_>>() {
                    if q > id {
                        pass.push(std::cmp::Reverse(q));
                    } else {
                        next.push(q);
                    }
                }
            }
        }
    }

    fn free_group(&mut self, id: GroupId) {
        let group = self.list[id as usize].take().expect("live group");
        self.free.push(id);
        // A group dropped and reclaimed between two barriers: the slot may be
        // reused before the barrier settles the dead, and it has no
        // dependents left to settle.
        if group.died_pending {
            self.died.retain(|&d| d != id);
        }
        if let Some(parent) = group.parent {
            self.remove_dependent(parent, id, group.parent_slot);
            self.release_ref(parent);
        }
        match group.kind {
            GroupKind::Defer {
                after: Some(after), ..
            } => {
                self.remove_dependent(after, id, group.after_slot);
                self.release_ref(after);
            }
            GroupKind::Shared { members } => {
                for member in members {
                    self.release_ref(member);
                    self.get_mut(member).sharing.retain(|&s| s != id);
                }
            }
            _ => {}
        }
    }

    pub fn get(&self, id: GroupId) -> &Group {
        self.list[id as usize].as_ref().expect("live group")
    }

    pub fn get_mut(&mut self, id: GroupId) -> &mut Group {
        self.list[id as usize].as_mut().expect("live group")
    }

    /// The live groups, by id.
    pub fn iter(&self) -> impl Iterator<Item = (GroupId, &Group)> {
        self.list
            .iter()
            .enumerate()
            .filter_map(|(id, group)| Some((id as GroupId, group.as_ref()?)))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (GroupId, &mut Group)> {
        self.list
            .iter_mut()
            .enumerate()
            .filter_map(|(id, group)| Some((id as GroupId, group.as_mut()?)))
    }

    /// Fails a halted group with the error that halted it, which stays on
    /// the group for the fragments that depend on it, and returns that error.
    pub fn fail_halted(&mut self, id: GroupId) -> GraphQLError {
        let group = self.get_mut(id);
        let GroupState::Halted(error) = &group.state else {
            unreachable!("group {id} is not halted")
        };
        let error = error.clone();
        self.set_state(id, GroupState::Failed(Some(error.clone())));
        error
    }

    /// A group whose work must stop: halted, failed or dropped, itself or by an ancestor.
    pub fn is_dead(&self, mut id: GroupId) -> bool {
        loop {
            let group = self.get(id);
            if matches!(
                group.state,
                GroupState::Halted(_) | GroupState::Failed(_) | GroupState::Dropped
            ) {
                return true;
            }
            match group.parent {
                Some(parent) => id = parent,
                None => return false,
            }
        }
    }

    /// Whether `ancestor` is on `id`'s parent chain (or equal to it).
    pub fn is_ancestor(&self, ancestor: GroupId, mut id: GroupId) -> bool {
        loop {
            if id == ancestor {
                return true;
            }
            match self.get(id).parent {
                Some(parent) => id = parent,
                None => return false,
            }
        }
    }

    /// The shared field sets `member` takes part in.
    pub fn sharing(&self, member: GroupId) -> Vec<GroupId> {
        self.get(member).sharing.clone()
    }

    /// Allocates the groups of one object's shared field sets: one per set
    /// of `sets` that more than one of its pending fragments selects.
    pub fn alloc_shared(
        &mut self,
        sets: &[(Vec<UsageId>, Vec<u32>)],
        pending: &[(UsageId, GroupId)],
        group: GroupId,
    ) -> Vec<(usize, GroupId)> {
        let mut shared = Vec::new();
        let member = |u: &UsageId| pending.iter().find(|(p, _)| p == u).map(|(_, g)| *g);
        for (set, (usages, _)) in sets.iter().enumerate().skip(1) {
            if usages.iter().filter_map(member).nth(1).is_none() {
                continue;
            }
            let members = usages.iter().filter_map(member).collect();
            let g = self.alloc(GroupKind::Shared { members }, group);
            self.retain(g);
            shared.push((set, g));
        }
        shared
    }

    pub fn is_released(&self, id: GroupId) -> bool {
        matches!(
            self.get(id).state,
            GroupState::Released | GroupState::Halted(_) | GroupState::Completed
        )
    }

    pub fn assign_wire_id(&mut self, id: GroupId) -> u32 {
        if let Some(wire) = self.get(id).wire_id {
            return wire;
        }
        let wire = self.next_wire;
        self.next_wire += 1;
        self.get_mut(id).wire_id = Some(wire);
        wire
    }
}

/// The non-generic per-request state every scope can reach.
pub struct Shared {
    pub groups: Mutex<Groups>,
    pub behavior: ErrorBehavior,
    pub incremental: bool,
    pub capacity: usize,
    pub table: Arc<PlanTable>,
    /// Pre-resolved `__schema`/`__type` root selections, keyed by response key.
    pub introspection: Option<serde_json::Value>,
    /// Set once any stream driver exists, so barriers without streams skip the walk.
    pub has_streams: std::sync::atomic::AtomicBool,
    /// Under `Halt`: an error was recorded and a barrier must run at once.
    pub halted: std::sync::atomic::AtomicBool,
}

impl Shared {
    pub fn groups(&self) -> MutexGuard<'_, Groups> {
        self.groups_for_drop().expect("group table poisoned")
    }

    /// For `Drop` impls: `None` once a panic poisoned the table, so unwinding
    /// never panics twice. Execution never spawns and user code cannot reach
    /// the table, so the lock is never contended: failing to take it means
    /// this thread already holds it.
    pub fn groups_for_drop(&self) -> Option<MutexGuard<'_, Groups>> {
        match self.groups.try_lock() {
            Ok(groups) => Some(groups),
            Err(TryLockError::Poisoned(_)) => None,
            Err(TryLockError::WouldBlock) => panic!("group table locked re-entrantly"),
        }
    }

    pub fn is_dead(&self, group: GroupId) -> bool {
        self.groups().is_dead(group)
    }

    /// Under `Halt`: halts a running group with the first error recorded for
    /// it, which the barrier ships even if the column holding it never completes.
    pub fn halt(&self, group: GroupId, error: impl FnOnce() -> GraphQLError) {
        if self.behavior != ErrorBehavior::Halt {
            return;
        }
        let mut groups = self.groups();
        if matches!(groups.get(group).state, GroupState::Released) {
            groups.set_state(group, GroupState::Halted(error()));
        }
        self.halted
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
