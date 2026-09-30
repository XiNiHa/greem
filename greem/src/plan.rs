//! The per-request Plan table: one entry per (execution-tree node, partition
//! leaf), built by the generated plan walk and frozen before generation 0.

use crate::context::{Context, DeliveryGroup, HintRegistry, Planning};
use crate::error::{Error, Location};
use crate::resolver::{Args, Field, Resolver};
use crate::tree::{Abort, Collected, FieldKind, NodeId, StreamInfo, Tree, UsageId};
use crate::value::FromInput;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

pub type PlanId = u32;

/// The partition-leaf path identifying one arm of an abstract output type.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Leaf(pub Vec<u8>);

impl Leaf {
    pub fn push(&self, step: u8) -> Leaf {
        let mut next = self.0.clone();
        next.push(step);
        Leaf(next)
    }
}

pub struct FieldHeader {
    pub key: String,
    pub name: String,
    pub kind: FieldKind,
    pub child: Option<NodeId>,
    pub composite: bool,
    pub usages: Vec<UsageId>,
    pub stream: Option<StreamInfo>,
    pub spans: Vec<Location>,
    pub args_error: Option<Error>,
    pub hints: Vec<(TypeId, Box<dyn Any + Send + Sync>)>,
    pub children: Vec<PlanId>,
    /// Defer usages of every deferred field set at or beneath this field's
    /// entries, so a stream knows which groups its items can still feed.
    pub beneath: Vec<UsageId>,
}

pub struct PlanHeader {
    pub node: NodeId,
    pub typename: &'static str,
    pub leaf: Leaf,
    pub parent: Option<(PlanId, u32)>,
    pub fields: Vec<FieldHeader>,
    /// Field indices per delivery set: index 0 is the immediate set, the rest
    /// are deferred sets keyed by their sorted usage set.
    pub sets: Vec<(Vec<UsageId>, Vec<u32>)>,
    pub introduced: Vec<UsageId>,
    pub collected: Arc<Collected>,
}

#[derive(Default)]
pub struct PlanTable {
    pub headers: Vec<PlanHeader>,
    pub typed: Vec<Option<Box<dyn Any + Send + Sync>>>,
    pub usages: Vec<crate::tree::DeferUsage>,
    index: HashMap<(NodeId, Leaf), PlanId>,
}

impl PlanTable {
    /// Fills every field's `beneath` set from its child entries, post-order.
    pub(crate) fn compute_beneath(&mut self) {
        let mut memo: Vec<Option<Vec<UsageId>>> = vec![None; self.headers.len()];
        for id in 0..self.headers.len() {
            self.beneath_of(id as PlanId, &mut memo);
        }
    }

    fn beneath_of(&mut self, id: PlanId, memo: &mut Vec<Option<Vec<UsageId>>>) -> Vec<UsageId> {
        if let Some(done) = &memo[id as usize] {
            return done.clone();
        }
        let mut all: Vec<UsageId> = self.headers[id as usize]
            .sets
            .iter()
            .flat_map(|(usages, _)| usages.iter().copied())
            .collect();
        for field in 0..self.headers[id as usize].fields.len() {
            let children = self.headers[id as usize].fields[field].children.clone();
            let mut beneath = Vec::new();
            for child in children {
                beneath.extend(self.beneath_of(child, memo));
            }
            beneath.sort_unstable();
            beneath.dedup();
            all.extend(beneath.iter().copied());
            self.headers[id as usize].fields[field].beneath = beneath;
        }
        all.sort_unstable();
        all.dedup();
        memo[id as usize] = Some(all.clone());
        all
    }

    pub fn lookup(&self, node: NodeId, leaf: &Leaf) -> Option<PlanId> {
        self.index.get(&(node, leaf.clone())).copied()
    }

    pub fn header(&self, id: PlanId) -> &PlanHeader {
        &self.headers[id as usize]
    }

    pub fn typed<P: 'static>(&self, id: PlanId) -> &P {
        self.typed[id as usize]
            .as_ref()
            .and_then(|p| p.downcast_ref::<P>())
            .expect("typed Plan payload of the expected generated type")
    }

    pub(crate) fn field_name(&self, entry: PlanId, field: u32) -> (&str, &str) {
        let header = &self.headers[entry as usize];
        (header.typename, &header.fields[field as usize].name)
    }

    pub(crate) fn delivery_group(&self, entry: PlanId, field: u32) -> DeliveryGroup {
        let field = &self.headers[entry as usize].fields[field as usize];
        if field.stream.is_some() {
            DeliveryGroup::Streamed
        } else if !field.usages.is_empty() {
            DeliveryGroup::Deferred
        } else {
            let mut cursor = self.headers[entry as usize].parent;
            while let Some((entry, index)) = cursor {
                let header = &self.headers[entry as usize];
                let f = &header.fields[index as usize];
                if f.stream.is_some() {
                    return DeliveryGroup::Streamed;
                }
                if !f.usages.is_empty() {
                    return DeliveryGroup::Deferred;
                }
                cursor = header.parent;
            }
            DeliveryGroup::Initial
        }
    }

    pub(crate) fn hint_slot(
        &self,
        entry: PlanId,
        field: u32,
        ty: TypeId,
    ) -> Option<&(dyn Any + Send + Sync)> {
        self.headers[entry as usize].fields[field as usize]
            .hints
            .iter()
            .find(|(t, _)| *t == ty)
            .map(|(_, b)| b.as_ref())
    }

    pub(crate) fn nearest_hint_mut(
        &mut self,
        entry: PlanId,
        field: u32,
        ty: TypeId,
    ) -> Option<&mut (dyn Any + Send + Sync)> {
        let mut cursor = self.headers[entry as usize].parent;
        let _ = field;
        while let Some((e, f)) = cursor {
            let header = &self.headers[e as usize];
            if header.fields[f as usize]
                .hints
                .iter()
                .any(|(t, _)| *t == ty)
            {
                return self.headers[e as usize].fields[f as usize]
                    .hints
                    .iter_mut()
                    .find(|(t, _)| *t == ty)
                    .map(|(_, b)| b.as_mut());
            }
            cursor = header.parent;
        }
        None
    }
}

/// Drives the generated plan walk: creates entries, coerces arguments,
/// registers hints and runs `plan` in post-order.
pub struct Walker<'w, C> {
    pub(crate) tree: &'w mut Tree,
    pub(crate) table: &'w mut PlanTable,
    pub(crate) ctx: &'w Context<'w, C>,
    parent: Option<(PlanId, u32)>,
}

impl<C> Walker<'_, C> {
    /// Creates the Plan entry for `typename` at `node` under partition `leaf`.
    pub fn enter(
        &mut self,
        node: NodeId,
        leaf: &Leaf,
        typename: &'static str,
    ) -> Result<PlanId, Abort> {
        let collected = self.tree.collect(node, typename)?;
        let id = self.table.headers.len() as PlanId;
        let fields = collected
            .fields
            .iter()
            .map(|f| FieldHeader {
                key: f.key.clone(),
                name: f.name.clone(),
                kind: f.kind,
                child: f.child,
                composite: f.composite,
                usages: f.usages.clone(),
                stream: f.stream.clone(),
                spans: f.spans.clone(),
                args_error: None,
                hints: Vec::new(),
                children: Vec::new(),
                beneath: Vec::new(),
            })
            .collect::<Vec<_>>();
        let mut sets: Vec<(Vec<UsageId>, Vec<u32>)> = vec![(Vec::new(), Vec::new())];
        for (i, f) in fields.iter().enumerate() {
            match sets.iter_mut().find(|(usages, _)| *usages == f.usages) {
                Some((_, indices)) => indices.push(i as u32),
                None => sets.push((f.usages.clone(), vec![i as u32])),
            }
        }
        self.table.headers.push(PlanHeader {
            node,
            typename,
            leaf: leaf.clone(),
            parent: self.parent,
            fields,
            sets,
            introduced: collected.introduced.clone(),
            collected: collected.clone(),
        });
        self.table.typed.push(None);
        self.table.index.insert((node, leaf.clone()), id);
        Ok(id)
    }

    pub fn field_count(&self, entry: PlanId) -> usize {
        self.table.headers[entry as usize].fields.len()
    }

    pub fn field_name(&self, entry: PlanId, field: usize) -> &str {
        &self.table.headers[entry as usize].fields[field].name
    }

    /// Coerces the arguments of a selected field; a failure is a field error
    /// recorded on the header, never a request failure.
    pub fn coerce<F: Field>(&mut self, entry: PlanId, field: usize) -> Result<Args<F>, Error> {
        let header = &self.table.headers[entry as usize];
        let selected = &header.collected.fields[field];
        let result = self.tree.coerce_arguments(selected).and_then(|input| {
            Args::<F>::from_input(&input)
                .map_err(|e| Error::framework(e.to_string(), "BAD_USER_INPUT"))
        });
        if let Err(error) = &result {
            self.table.headers[entry as usize].fields[field].args_error = Some(error.clone());
        }
        result
    }

    pub fn hints<T: Resolver<F, C>, F: Field>(&mut self, entry: PlanId, field: usize) {
        let slots = &mut self.table.headers[entry as usize].fields[field].hints;
        T::hints(&mut HintRegistry { slots });
    }

    pub fn plan<T: Resolver<F, C>, F: Field>(
        &mut self,
        entry: PlanId,
        field: usize,
        args: &Args<F>,
    ) {
        let mut planning = Planning {
            table: self.table,
            entry,
            field: field as u32,
            args,
            ctx: self.ctx,
        };
        T::plan(&mut planning);
    }

    /// Walks into the child node of a composite field, wiring the child link.
    pub fn descend(
        &mut self,
        entry: PlanId,
        field: usize,
        walk: impl FnOnce(&mut Self, NodeId, &Leaf) -> Result<(), Abort>,
    ) -> Result<(), Abort> {
        let Some(child) = self.table.headers[entry as usize].fields[field].child else {
            return Ok(());
        };
        let before = self.table.headers.len();
        let saved = self.parent;
        self.parent = Some((entry, field as u32));
        let result = walk(self, child, &Leaf::default());
        self.parent = saved;
        result?;
        let created: Vec<PlanId> = (before..self.table.headers.len())
            .map(|i| i as PlanId)
            .filter(|&id| self.table.headers[id as usize].parent == Some((entry, field as u32)))
            .collect();
        self.table.headers[entry as usize].fields[field]
            .children
            .extend(created);
        Ok(())
    }

    pub fn set_typed(&mut self, entry: PlanId, typed: Box<dyn Any + Send + Sync>) {
        self.table.typed[entry as usize] = Some(typed);
    }
}

impl<'w, C> Walker<'w, C> {
    pub(crate) fn new(
        tree: &'w mut Tree,
        table: &'w mut PlanTable,
        ctx: &'w Context<'w, C>,
    ) -> Self {
        Self {
            tree,
            table,
            ctx,
            parent: None,
        }
    }
}
