use crate::error::{GraphQLError, PathSegment};
use crate::plan::PlanTable;
use crate::tree::{NodeId, UsageId};
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
    pub state: GroupState,
    pub wire_id: Option<u32>,
    /// Live references from objects, scopes, drivers and child groups; a
    /// terminal group with none is reclaimed at the next barrier.
    pub refs: u32,
}

#[derive(Default, Debug)]
pub struct Groups {
    /// `None` is a reclaimed slot, reused by the next allocation.
    list: Vec<Option<Group>>,
    next_wire: u32,
    free: Vec<GroupId>,
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
        }));
        groups
    }

    pub fn alloc(&mut self, kind: GroupKind, parent: GroupId) -> GroupId {
        self.retain(parent);
        if let GroupKind::Defer {
            after: Some(after), ..
        } = kind
        {
            self.retain(after);
        }
        if let GroupKind::Shared { members } = &kind {
            for &member in members {
                self.retain(member);
            }
        }
        let group = Group {
            kind,
            parent: Some(parent),
            state: GroupState::Unreleased,
            wire_id: None,
            refs: 0,
        };
        let id = match self.free.pop() {
            Some(id) => {
                self.list[id as usize] = Some(group);
                id
            }
            None => {
                self.list.push(Some(group));
                (self.list.len() - 1) as GroupId
            }
        };
        crate::__private::MAX_LIVE_GROUPS.fetch_max(
            self.list.len() - self.free.len(),
            std::sync::atomic::Ordering::Relaxed,
        );
        id
    }

    pub fn retain(&mut self, id: GroupId) {
        self.get_mut(id).refs += 1;
    }

    /// Drops one reference; "release" alone means a group's activation.
    pub fn release_ref(&mut self, id: GroupId) {
        let group = self.get_mut(id);
        group.refs = group.refs.saturating_sub(1);
    }

    /// Frees every finished group with no references left, repeating while a
    /// freed child releases its parent. A group under a failed or dropped
    /// ancestor never runs, so it counts as finished whatever its own state.
    /// The initial group is never freed.
    pub fn sweep(&mut self) {
        loop {
            let mut freed_any = false;
            for id in 1..self.list.len() {
                let Some(group) = &self.list[id] else {
                    continue;
                };
                let terminal = matches!(
                    group.state,
                    GroupState::Completed | GroupState::Failed(_) | GroupState::Dropped
                );
                if group.refs != 0 || !(terminal || self.is_dead(id as GroupId)) {
                    continue;
                }
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
                let group = self.list[id].take().expect("live group");
                self.free.push(id as GroupId);
                if let Some(parent) = group.parent {
                    self.release_ref(parent);
                }
                match group.kind {
                    GroupKind::Defer {
                        after: Some(after), ..
                    } => self.release_ref(after),
                    GroupKind::Shared { members } => {
                        for member in members {
                            self.release_ref(member);
                        }
                    }
                    _ => {}
                }
                freed_any = true;
            }
            if !freed_any {
                break;
            }
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

    /// The ids of the live groups, for loops that change groups as they go.
    pub fn ids(&self) -> Vec<GroupId> {
        self.iter().map(|(id, _)| id).collect()
    }

    /// Fails a halted group with the error that halted it, which stays on
    /// the group for the fragments that depend on it, and returns that error.
    pub fn fail_halted(&mut self, id: GroupId) -> GraphQLError {
        let group = self.get_mut(id);
        let GroupState::Halted(error) = &group.state else {
            unreachable!("group {id} is not halted")
        };
        let error = error.clone();
        group.state = GroupState::Failed(Some(error.clone()));
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
        self.iter()
            .filter(|(_, group)| {
                matches!(&group.kind, GroupKind::Shared { members } if members.contains(&member))
            })
            .map(|(id, _)| id)
            .collect()
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
        for (set, (usages, _)) in sets.iter().enumerate().skip(1) {
            let members: Vec<GroupId> = usages
                .iter()
                .filter_map(|u| pending.iter().find(|(p, _)| p == u).map(|(_, g)| *g))
                .collect();
            if members.len() > 1 {
                let g = self.alloc(GroupKind::Shared { members }, group);
                self.retain(g);
                shared.push((set, g));
            }
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
        let group = groups.get_mut(group);
        if matches!(group.state, GroupState::Released) {
            group.state = GroupState::Halted(error());
        }
        self.halted
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
