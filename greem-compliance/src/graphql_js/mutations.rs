//! The area for graphql-js v17.0.2 `src/execution/__tests__/mutations-test.ts`:
//! its schema, world and resolvers. The world holds the one number the
//! mutations change, so serial execution is observable.

use crate::harness::{Area, Harness, HasHarness, pending_once};
use greem::{Args, Context, Error, Roots, SchemaBuilder};
use std::sync::{Arc, Mutex};

pub mod schema {
    greem::include_schema!("graphql_js_mutations.rs");
}

#[derive(Debug, Default)]
pub struct World {
    pub harness: Harness,
    number: Arc<Mutex<i32>>,
}

/// The same configuration over a fresh copy of the number.
impl Clone for World {
    fn clone(&self) -> Self {
        World {
            harness: self.harness.clone(),
            number: Arc::new(Mutex::new(*self.number.lock().unwrap())),
        }
    }
}

impl World {
    /// Upstream's `new Root(originalNumber)`.
    pub fn new(number: i32) -> Self {
        World {
            harness: Harness::default(),
            number: Arc::new(Mutex::new(number)),
        }
    }

    fn change(&self, new_number: Option<i32>) -> NumberHolder {
        *self.number.lock().unwrap() = new_number.unwrap_or(0);
        self.holder()
    }

    fn holder(&self) -> NumberHolder {
        NumberHolder(self.number.clone())
    }
}

impl HasHarness for World {
    fn harness(&self) -> &Harness {
        &self.harness
    }
}

impl Area for World {
    type Info = schema::__private::Info;
    type Query = QueryRoot;
    type Mutation = MutationRoot;

    fn builder() -> SchemaBuilder<Self::Info, Self, Self::Query, Self::Mutation> {
        schema::Schema::<World>::builder()
            .query::<QueryRoot>()
            .mutation::<MutationRoot>()
    }

    fn roots() -> Roots<QueryRoot, MutationRoot> {
        Roots {
            query: QueryRoot,
            mutation: MutationRoot,
        }
    }
}

pub type Schema = schema::Schema<World, QueryRoot, MutationRoot>;

pub struct QueryRoot;
pub struct MutationRoot;
pub struct NumberHolder(Arc<Mutex<i32>>);

#[greem::object(schema = crate::graphql_js::mutations::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn number_holder(
        &self,
        _args: &Args<schema::Query::numberHolder>,
        ctx: &Context<World>,
    ) -> Option<NumberHolder> {
        ctx.app().harness.trace("Query.numberHolder", 1).await;
        Some(ctx.app().holder())
    }
}

#[greem::object(schema = crate::graphql_js::mutations::schema, type = "NumberHolder", context = World)]
impl NumberHolder {
    async fn the_number(
        &self,
        _args: &Args<schema::NumberHolder::theNumber>,
        ctx: &Context<World>,
    ) -> Option<i32> {
        ctx.app().harness.trace("NumberHolder.theNumber", 1).await;
        Some(*self.0.lock().unwrap())
    }
    async fn promise_to_get_the_number(
        &self,
        _args: &Args<schema::NumberHolder::promiseToGetTheNumber>,
        ctx: &Context<World>,
    ) -> Option<i32> {
        ctx.app()
            .harness
            .trace("NumberHolder.promiseToGetTheNumber", 1)
            .await;
        pending_once().await;
        Some(*self.0.lock().unwrap())
    }
}

#[greem::object(schema = crate::graphql_js::mutations::schema, type = "Mutation", context = World)]
impl MutationRoot {
    async fn immediately_change_the_number(
        &self,
        args: &Args<schema::Mutation::immediatelyChangeTheNumber>,
        ctx: &Context<World>,
    ) -> Option<NumberHolder> {
        ctx.app()
            .harness
            .trace("Mutation.immediatelyChangeTheNumber", 1)
            .await;
        Some(ctx.app().change(args.newNumber))
    }
    async fn promise_to_change_the_number(
        &self,
        args: &Args<schema::Mutation::promiseToChangeTheNumber>,
        ctx: &Context<World>,
    ) -> Option<NumberHolder> {
        ctx.app()
            .harness
            .trace("Mutation.promiseToChangeTheNumber", 1)
            .await;
        pending_once().await;
        Some(ctx.app().change(args.newNumber))
    }
    async fn fail_to_change_the_number(
        &self,
        _args: &Args<schema::Mutation::failToChangeTheNumber>,
        ctx: &Context<World>,
    ) -> Result<Option<NumberHolder>, Error> {
        ctx.app()
            .harness
            .trace("Mutation.failToChangeTheNumber", 1)
            .await;
        Err(Error::new("Cannot change the number"))
    }
    async fn promise_and_fail_to_change_the_number(
        &self,
        _args: &Args<schema::Mutation::promiseAndFailToChangeTheNumber>,
        ctx: &Context<World>,
    ) -> Result<Option<NumberHolder>, Error> {
        ctx.app()
            .harness
            .trace("Mutation.promiseAndFailToChangeTheNumber", 1)
            .await;
        pending_once().await;
        Err(Error::new("Cannot change the number"))
    }
}
