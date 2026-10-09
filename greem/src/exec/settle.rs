//! Post-hoc null propagation, liveness marking and error collection over the
//! completed columns of a delivery group.

use crate::error::GraphQLError;
use crate::exec::column::{Column, ErrorId, Inner, Leaf, ObjSlot, Slot, TurnRange};
use crate::exec::scope::{ANNOUNCE, DeferredSetState, FieldState, Scope, ScopeMeta};
use crate::exec::state::Groups;
use crate::plan::PlanTable;
use smallvec::SmallVec;

/// `Err` carries the error that must propagate past this position.
pub(crate) type Settled = Result<(), Box<GraphQLError>>;

pub(crate) fn error_of(
    table: &PlanTable,
    meta: &ScopeMeta<'_>,
    column: &Column<'_>,
    turn: usize,
    id: ErrorId,
) -> GraphQLError {
    let record = &column.turns[turn].errors[id as usize];
    let field = &table.header(meta.entry).fields[column.field as usize];
    let path = meta.path_to(record.object, &field.key, &record.indices);
    GraphQLError::from_error(&record.error, field.spans.clone(), path)
}

/// Settles one object: every execution error beneath it is absorbed by the
/// nearest nullable position. `Err` means the object itself must be nulled.
pub(crate) fn settle_object(scope: &mut Scope<'_>, object: u32) -> Settled {
    let table = &scope.shared.table;
    let meta = scope.meta;
    for state in &mut scope.fields {
        let FieldState::Done(column) = state else {
            continue;
        };
        settle_column(table, meta, column, object)?;
    }
    Ok(())
}

pub(crate) fn settle_column(
    table: &PlanTable,
    meta: &ScopeMeta<'_>,
    column: &mut Column<'_>,
    object: u32,
) -> Settled {
    if column.kind != crate::tree::FieldKind::Normal {
        return Ok(());
    }
    let depth = column.depth();
    if depth == 0 {
        let nullable = column.shape.inner;
        return settle_inner(table, meta, column, 0, object, nullable);
    }
    let nullable = column.shape.levels[0];
    match column.level0[object as usize] {
        Slot::Items { start, len } => {
            for slot in start..start + len {
                if let Err(error) = settle_level(table, meta, column, 0, 1, slot) {
                    if nullable {
                        column.level0[object as usize] = Slot::Propagated { start, len };
                        return Ok(());
                    }
                    return Err(error);
                }
            }
            Ok(())
        }
        Slot::Error(id) if !nullable => Err(Box::new(error_of(table, meta, column, 0, id))),
        _ => Ok(()),
    }
}

/// Settles the items of one stream turn range; `Err` fails the stream group.
pub(crate) fn settle_range(
    table: &PlanTable,
    meta: &ScopeMeta<'_>,
    column: &mut Column<'_>,
    turn: usize,
    range: TurnRange,
) -> Settled {
    for slot in range.start_slot..range.start_slot + range.len {
        settle_level(table, meta, column, turn, 1, slot)?;
    }
    Ok(())
}

fn settle_level(
    table: &PlanTable,
    meta: &ScopeMeta<'_>,
    column: &mut Column<'_>,
    turn: usize,
    level: usize,
    slot: u32,
) -> Settled {
    let depth = column.depth();
    let nullable = column.shape.nullable_at(level);
    if level == depth {
        return settle_inner(table, meta, column, turn, slot, nullable);
    }
    match column.turns[turn].levels[level - 1][slot as usize] {
        Slot::Items { start, len } => {
            for s in start..start + len {
                if let Err(error) = settle_level(table, meta, column, turn, level + 1, s) {
                    if nullable {
                        column.turns[turn].levels[level - 1][slot as usize] =
                            Slot::Propagated { start, len };
                        return Ok(());
                    }
                    return Err(error);
                }
            }
            Ok(())
        }
        Slot::Error(id) if !nullable => Err(Box::new(error_of(table, meta, column, turn, id))),
        _ => Ok(()),
    }
}

fn settle_inner(
    table: &PlanTable,
    meta: &ScopeMeta<'_>,
    column: &mut Column<'_>,
    turn: usize,
    slot: u32,
    nullable: bool,
) -> Settled {
    let found = column.turns[turn].with_stored(|stored| match &stored.inner {
        Inner::Leaves(leaves) => match leaves[slot as usize] {
            Leaf::Error(id) => Err(id),
            _ => Ok(None),
        },
        Inner::Objects(objects) => match objects[slot as usize] {
            ObjSlot::Object { child, index } => Ok(Some((child, index))),
            ObjSlot::Error(id) => Err(id),
            _ => Ok(None),
        },
    });
    let (child, index) = match found {
        Ok(Some(link)) => link,
        Err(id) if !nullable => return Err(Box::new(error_of(table, meta, column, turn, id))),
        _ => return Ok(()),
    };
    let result = column.turns[turn].with_child_mut(child, |scope| settle_object(scope, index));
    match result {
        Ok(()) => Ok(()),
        Err(_) if nullable => {
            column.turns[turn].with_stored_mut(|stored| stored.inner.set_propagated(slot));
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Calls `f(child, index)` for every child object reachable from `object` in
/// `column`'s turn 0: through delivered slots only, or also through slots
/// nulled by propagation when `nulled` is set.
pub(crate) fn for_each_child_of(
    column: &Column<'_>,
    turn: usize,
    object: u32,
    nulled: bool,
    f: &mut dyn FnMut(u32, u32),
) {
    for_each_child_marked(column, turn, object, &mut |child, index, delivered| {
        if delivered || nulled {
            f(child, index)
        }
    });
}

/// Calls `f(child, index, delivered)` for every child object of `object`
/// in `column`'s turn 0, delivered or nulled by propagation on the way.
fn for_each_child_marked(
    column: &Column<'_>,
    turn: usize,
    object: u32,
    f: &mut dyn FnMut(u32, u32, bool),
) {
    let depth = column.depth();
    if depth == 0 {
        visit_inner(column, turn, object, true, f);
        return;
    }
    match column.level0[object as usize] {
        Slot::Items { start, len } => {
            for slot in start..start + len {
                visit_level(column, turn, 1, slot, true, f);
            }
        }
        Slot::Propagated { start, len } => {
            for slot in start..start + len {
                visit_level(column, turn, 1, slot, false, f);
            }
        }
        _ => {}
    }
}

pub(crate) fn for_each_child_in_range(
    column: &Column<'_>,
    turn: usize,
    range: TurnRange,
    nulled: bool,
    f: &mut dyn FnMut(u32, u32),
) {
    for slot in range.start_slot..range.start_slot + range.len {
        visit_level(
            column,
            turn,
            1,
            slot,
            true,
            &mut |child, index, delivered| {
                if delivered || nulled {
                    f(child, index)
                }
            },
        );
    }
}

fn visit_level(
    column: &Column<'_>,
    turn: usize,
    level: usize,
    slot: u32,
    delivered: bool,
    f: &mut dyn FnMut(u32, u32, bool),
) {
    if level == column.depth() {
        visit_inner(column, turn, slot, delivered, f);
        return;
    }
    match column.turns[turn].levels[level - 1][slot as usize] {
        Slot::Items { start, len } => {
            for s in start..start + len {
                visit_level(column, turn, level + 1, s, delivered, f);
            }
        }
        Slot::Propagated { start, len } => {
            for s in start..start + len {
                visit_level(column, turn, level + 1, s, false, f);
            }
        }
        _ => {}
    }
}

fn visit_inner(
    column: &Column<'_>,
    turn: usize,
    slot: u32,
    delivered: bool,
    f: &mut dyn FnMut(u32, u32, bool),
) {
    column.turns[turn].with_stored(|stored| {
        if let Inner::Objects(objects) = &stored.inner {
            match objects[slot as usize] {
                ObjSlot::Object { child, index } => f(child, index, delivered),
                ObjSlot::Propagated { child, index } => f(child, index, false),
                _ => {}
            }
        }
    });
}

/// Decides every object beneath `object`, which its group just shipped:
/// the ones reachable through delivered (non-null) slots are alive, the
/// rest were nulled away and leave the deferred sets still waiting on
/// them. Each decided object is listed on its scope for the announcement
/// of its pending groups.
pub(crate) fn mark_alive(
    scope: &mut Scope<'_>,
    object: u32,
    targets: &mut Vec<Target>,
    groups: &mut Groups,
) {
    targets.clear();
    decide_with(scope, object, true, targets, groups);
}

/// A child object to decide: its scope's index in the turn, its index
/// there, and whether it was delivered.
pub(crate) type Target = (u32, u32, bool);

/// `mark_alive` with one target stack for the whole subtree: each level
/// pushes its children above its caller's and truncates back when done.
fn decide_with(
    scope: &mut Scope<'_>,
    object: u32,
    alive: bool,
    targets: &mut Vec<Target>,
    groups: &mut Groups,
) {
    if alive {
        scope.alive[object as usize] = true;
    } else {
        for d in &mut scope.deferred {
            if matches!(d.state, DeferredSetState::Waiting(_)) {
                d.exclude(object, groups);
            }
        }
    }
    if scope.decided.is_empty() {
        scope.signal.raise(ANNOUNCE);
    }
    scope.decided.push(object);
    for i in 0..scope.fields.len() {
        let FieldState::Done(column) = &mut scope.fields[i] else {
            continue;
        };
        let start = targets.len();
        for_each_child_marked(column, 0, object, &mut |child, index, delivered| {
            targets.push((child, index, alive && delivered))
        });
        for t in start..targets.len() {
            let (child, index, alive) = targets[t];
            column.turns[0]
                .with_child_mut(child, |s| decide_with(s, index, alive, targets, groups));
        }
        targets.truncate(start);
    }
}

/// Decides the items of a stream turn range that just shipped, like `mark_alive`.
pub(crate) fn mark_alive_range(
    column: &mut Column<'_>,
    turn: usize,
    range: TurnRange,
    targets: &mut Vec<Target>,
    groups: &mut Groups,
) {
    targets.clear();
    for slot in range.start_slot..range.start_slot + range.len {
        visit_level(
            column,
            turn,
            1,
            slot,
            true,
            &mut |child, index, delivered| targets.push((child, index, delivered)),
        );
    }
    for t in 0..targets.len() {
        let (child, index, alive) = targets[t];
        column.turns[turn].with_child_mut(child, |s| decide_with(s, index, alive, targets, groups));
    }
}

/// Errors gathered for one payload, keyed for the deterministic order:
/// generation, then scope in traversal order, then field, then object.
#[derive(Default)]
pub(crate) struct ErrorSink {
    next_seq: u32,
    /// The serial field being collected (1-based), 0 outside serial scopes:
    /// a serial field's errors, its subtree's included, precede the next field's.
    serial: u32,
    items: Vec<((u32, u32, u32), GraphQLError)>,
}

impl ErrorSink {
    fn seq(&mut self) -> u32 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    /// Takes the errors gathered so far, in order, leaving the sink ready
    /// for the next payload entry.
    pub(crate) fn drain_sorted(&mut self) -> Vec<GraphQLError> {
        if self.items.is_empty() {
            return Vec::new();
        }
        self.items.sort_by_key(|(key, _)| *key);
        self.items.drain(..).map(|(_, error)| error).collect()
    }
}

/// Collects every error recorded beneath `objects` of `scope`: this scope's
/// records field by field, then each child scope once with every reached index.
pub(crate) fn collect_errors(scope: &Scope<'_>, objects: &[u32], out: &mut ErrorSink) {
    let table = &scope.shared.table;
    let seq = out.seq();
    let own = |column: &Column<'_>, out: &mut ErrorSink| {
        for &object in objects {
            for (id, record) in column.turns[0].errors.iter().enumerate() {
                if record.object == object {
                    out.items.push((
                        (out.serial, record.generation, seq),
                        error_of(table, scope.meta, column, 0, id as ErrorId),
                    ));
                }
            }
        }
    };
    let beneath = |column: &Column<'_>, out: &mut ErrorSink| {
        let mut per_child = PerChild::default();
        for &object in objects {
            for_each_child_of(column, 0, object, true, &mut |child, index| {
                per_child.push(child, index)
            });
        }
        for (child, indices) in &per_child.0 {
            column.turns[0].with_child(*child, |s| collect_errors(s, indices, out));
        }
    };
    if scope.meta.serial {
        // Serial fields run one after another, each with its whole subtree.
        for column in scope.columns() {
            out.serial = column.field + 1;
            own(column, out);
            beneath(column, out);
        }
        out.serial = 0;
        return;
    }
    for column in scope.columns() {
        own(column, out);
    }
    for column in scope.columns() {
        beneath(column, out);
    }
}

/// Errors of the items in a stream turn range, including their descendants.
pub(crate) fn collect_range_errors(
    scope: &Scope<'_>,
    column: &Column<'_>,
    turn: usize,
    range: TurnRange,
    out: &mut ErrorSink,
) {
    let table = &scope.shared.table;
    let seq = out.seq();
    for (id, record) in column.turns[turn].errors.iter().enumerate() {
        if record.object == range.object
            && let Some(&index) = record.indices.first()
            && index >= range.start_index
            && index < range.start_index + range.len
        {
            out.items.push((
                (out.serial, record.generation, seq),
                error_of(table, scope.meta, column, turn, id as ErrorId),
            ));
        }
    }
    let mut per_child = PerChild::default();
    for_each_child_in_range(column, turn, range, true, &mut |child, index| {
        per_child.push(child, index)
    });
    for (child, indices) in &per_child.0 {
        column.turns[turn].with_child(*child, |s| collect_errors(s, indices, out));
    }
}

/// The reached indices of each child scope, by child. A column's turn has
/// one child scope per concrete output type, and one root object reaches
/// one index in it, so the common case stays on the stack.
#[derive(Default)]
struct PerChild(SmallVec<[(u32, SmallVec<[u32; 1]>); 1]>);

impl PerChild {
    fn push(&mut self, child: u32, index: u32) {
        let at = self.0.partition_point(|(c, _)| *c < child);
        match self.0.get_mut(at) {
            Some((c, indices)) if *c == child => indices.push(index),
            _ => self.0.insert(at, (child, SmallVec::from_slice(&[index]))),
        }
    }
}
