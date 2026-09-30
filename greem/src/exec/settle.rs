//! Post-hoc null propagation, liveness marking and error collection over the
//! completed columns of a delivery group.

use crate::error::{GraphQLError, PathSegment};
use crate::exec::column::{Column, ErrorId, Inner, Leaf, ObjSlot, Slot, TurnRange};
use crate::exec::scope::{FieldState, Scope, ScopeMeta};
use crate::plan::PlanTable;
use std::collections::BTreeMap;

/// `Err` carries the error that must propagate past this position.
pub(crate) type Settled = Result<(), Box<GraphQLError>>;

pub(crate) fn error_of(
    table: &PlanTable,
    meta: &ScopeMeta,
    column: &Column<'_>,
    turn: usize,
    id: ErrorId,
) -> GraphQLError {
    let record = &column.turns[turn].errors[id as usize];
    let field = &table.header(meta.entry).fields[column.field as usize];
    let mut path = meta.objects[record.object as usize].path.clone();
    path.push(PathSegment::Key(field.key.clone()));
    path.extend(
        record
            .indices
            .iter()
            .map(|&i| PathSegment::Index(i as usize)),
    );
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
    meta: &ScopeMeta,
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
    meta: &ScopeMeta,
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
    meta: &ScopeMeta,
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
    meta: &ScopeMeta,
    column: &mut Column<'_>,
    turn: usize,
    slot: u32,
    nullable: bool,
) -> Settled {
    let (child, index) = match &column.turns[turn].inner {
        Inner::Leaves(leaves) => {
            return match leaves[slot as usize] {
                Leaf::Error(id) if !nullable => {
                    Err(Box::new(error_of(table, meta, column, turn, id)))
                }
                _ => Ok(()),
            };
        }
        Inner::Objects(objects) => match objects[slot as usize] {
            ObjSlot::Object { child, index } => (child, index),
            ObjSlot::Error(id) if !nullable => {
                return Err(Box::new(error_of(table, meta, column, turn, id)));
            }
            _ => return Ok(()),
        },
    };
    let result = column.turns[turn].children[child as usize]
        .with_dependent_mut(|_, scope| settle_object(scope, index));
    match result {
        Ok(()) => Ok(()),
        Err(_) if nullable => {
            column.turns[turn].inner.set_propagated(slot);
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
    let depth = column.depth();
    if depth == 0 {
        visit_inner(column, turn, object, nulled, f);
        return;
    }
    match column.level0[object as usize] {
        Slot::Items { start, len } => {
            for slot in start..start + len {
                visit_level(column, turn, 1, slot, nulled, f);
            }
        }
        Slot::Propagated { start, len } if nulled => {
            for slot in start..start + len {
                visit_level(column, turn, 1, slot, nulled, f);
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
        visit_level(column, turn, 1, slot, nulled, f);
    }
}

fn visit_level(
    column: &Column<'_>,
    turn: usize,
    level: usize,
    slot: u32,
    nulled: bool,
    f: &mut dyn FnMut(u32, u32),
) {
    if level == column.depth() {
        visit_inner(column, turn, slot, nulled, f);
        return;
    }
    match column.turns[turn].levels[level - 1][slot as usize] {
        Slot::Items { start, len } => {
            for s in start..start + len {
                visit_level(column, turn, level + 1, s, nulled, f);
            }
        }
        Slot::Propagated { start, len } if nulled => {
            for s in start..start + len {
                visit_level(column, turn, level + 1, s, nulled, f);
            }
        }
        _ => {}
    }
}

fn visit_inner(
    column: &Column<'_>,
    turn: usize,
    slot: u32,
    nulled: bool,
    f: &mut dyn FnMut(u32, u32),
) {
    if let Inner::Objects(objects) = &column.turns[turn].inner {
        match objects[slot as usize] {
            ObjSlot::Object { child, index } => f(child, index),
            ObjSlot::Propagated { child, index } if nulled => f(child, index),
            _ => {}
        }
    }
}

/// Marks every object reachable from `object` through delivered (non-null)
/// slots as alive, so pending delivery groups on nulled objects are dropped.
pub(crate) fn mark_alive(scope: &mut Scope<'_>, object: u32) {
    scope.alive[object as usize] = true;
    for i in 0..scope.fields.len() {
        let FieldState::Done(column) = &mut scope.fields[i] else {
            continue;
        };
        let mut targets = Vec::new();
        for_each_child_of(column, 0, object, false, &mut |child, index| {
            targets.push((child, index))
        });
        for (child, index) in targets {
            column.turns[0].children[child as usize]
                .with_dependent_mut(|_, s| mark_alive(s, index));
        }
    }
}

pub(crate) fn mark_alive_range(column: &mut Column<'_>, turn: usize, range: TurnRange) {
    let mut targets = Vec::new();
    for_each_child_in_range(column, turn, range, false, &mut |child, index| {
        targets.push((child, index))
    });
    for (child, index) in targets {
        column.turns[turn].children[child as usize].with_dependent_mut(|_, s| mark_alive(s, index));
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

    pub(crate) fn sorted(mut self) -> Vec<GraphQLError> {
        self.items.sort_by_key(|(key, _)| *key);
        self.items.into_iter().map(|(_, error)| error).collect()
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
        let mut per_child: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &object in objects {
            for_each_child_of(column, 0, object, true, &mut |child, index| {
                per_child.entry(child).or_default().push(index)
            });
        }
        for (child, indices) in per_child {
            column.turns[0].children[child as usize]
                .with_dependent(|_, s| collect_errors(s, &indices, out));
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
    let mut per_child: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for_each_child_in_range(column, turn, range, true, &mut |child, index| {
        per_child.entry(child).or_default().push(index)
    });
    for (child, indices) in per_child {
        column.turns[turn].children[child as usize]
            .with_dependent(|_, s| collect_errors(s, &indices, out));
    }
}
