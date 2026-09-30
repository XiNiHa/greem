use crate::plan::{PlanId, PlanTable};
use std::any::{Any, TypeId};

/// The per-invocation view every resolver receives: the application's context
/// value plus the hints the field accepted during lookbehind planning. It
/// borrows both for the request; `'req` shrinks to the resolver's `'obj`.
pub struct Context<'req, C> {
    app: &'req C,
    hints: Option<HintAddr<'req>>,
}

pub(crate) struct HintAddr<'req> {
    pub table: &'req PlanTable,
    pub entry: PlanId,
    pub field: u32,
}

impl<'req, C> Context<'req, C> {
    pub(crate) fn new(app: &'req C, hints: Option<HintAddr<'req>>) -> Self {
        Self { app, hints }
    }

    /// The application's context value.
    pub fn app(&self) -> &'req C {
        self.app
    }

    /// The hint of type `H` this field accepted in its `Resolver::hints`.
    ///
    /// Panics when `H` was not accepted by this field; use [`try_hint`](Self::try_hint)
    /// for conditional acceptors.
    pub fn hint<H: 'static>(&self) -> &H {
        match self.try_hint::<H>() {
            Some(hint) => hint,
            None => {
                let (ty, field) = self
                    .hints
                    .as_ref()
                    .map(|a| a.table.field_name(a.entry, a.field))
                    .unwrap_or(("?", "?"));
                panic!(
                    "field `{ty}.{field}` did not accept hint `{}`",
                    std::any::type_name::<H>()
                )
            }
        }
    }

    pub fn try_hint<H: 'static>(&self) -> Option<&H> {
        let addr = self.hints.as_ref()?;
        addr.table
            .hint_slot(addr.entry, addr.field, TypeId::of::<H>())?
            .downcast_ref()
    }
}

/// Declares the hint types a field's resolver accepts from its descendants.
pub struct HintRegistry<'r> {
    pub(crate) slots: &'r mut Vec<(TypeId, Box<dyn Any + Send + Sync>)>,
}

impl HintRegistry<'_> {
    pub fn accept<H: Default + Send + Sync + 'static>(&mut self) {
        let id = TypeId::of::<H>();
        if self.slots.iter().any(|(t, _)| *t == id) {
            debug_assert!(
                false,
                "hint `{}` accepted twice",
                std::any::type_name::<H>()
            );
            return;
        }
        self.slots.push((id, Box::new(H::default())));
    }
}

/// Which delivery group a field's selection belongs to, as seen during planning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryGroup {
    Initial,
    Deferred,
    Streamed,
}

/// The planning view handed to `Resolver::plan`: the field's arguments and
/// context, its position facts, and the way to write hints upward.
pub struct Planning<'p, F: crate::resolver::Field, C> {
    pub(crate) table: &'p mut PlanTable,
    pub(crate) entry: PlanId,
    pub(crate) field: u32,
    pub(crate) args: &'p crate::resolver::Args<F>,
    pub(crate) ctx: &'p Context<'p, C>,
}

impl<'p, F: crate::resolver::Field, C> Planning<'p, F, C> {
    pub fn args(&self) -> &crate::resolver::Args<F> {
        self.args
    }

    pub fn ctx(&self) -> &Context<'p, C> {
        self.ctx
    }

    pub fn field_name(&self) -> &str {
        self.table.field_name(self.entry, self.field).1
    }

    pub fn parent_type(&self) -> &str {
        self.table.field_name(self.entry, self.field).0
    }

    pub fn delivery_group(&self) -> DeliveryGroup {
        self.table.delivery_group(self.entry, self.field)
    }

    /// Mutates the hint of type `H` at the nearest accepting field above this
    /// one (a field accepting and writing `H` addresses its ancestor, not
    /// itself). A no-op when no ancestor accepted `H`.
    pub fn hint<H: 'static>(&mut self, write: impl FnOnce(&mut H)) {
        if let Some(slot) = self
            .table
            .nearest_hint_mut(self.entry, self.field, TypeId::of::<H>())
        {
            write(slot.downcast_mut().expect("hint slot type"));
        }
    }
}
