//! The area for graphql-js v17.0.2 `src/execution/__tests__/nonnull-test.ts`:
//! its schema, world and resolvers. The world is upstream's `rootValue`:
//! what each `DataType` leaf resolves to. Every nest field returns the same
//! data, as upstream's do, so one unit struct is the whole object graph.

use crate::harness::{Area, Harness, HasHarness, pending_once};
use greem::{Args, Context, Error, NoMutation, Roots, SchemaBuilder};

pub mod schema {
    greem::include_schema!("graphql_js_nonnull.rs");
}

#[derive(Clone, Debug)]
pub struct World {
    pub harness: Harness,
    /// `sync`: `None` returns null (upstream's `nullingData`), `Some` throws it.
    pub sync: Option<&'static str>,
    /// `promise`: as `sync`, after one yield.
    pub promise: Option<&'static str>,
    /// `syncNonNull` throws this. Upstream's `nullingData` returns null here,
    /// which greem's type encoding forbids (reason iv), so it always throws.
    pub sync_non_null: &'static str,
    /// `promiseNonNull` throws this after one yield.
    pub promise_non_null: &'static str,
    /// Yields `promise` takes on top of its one, so it settles after the
    /// other fields of its generation.
    pub promise_delay: u32,
}

impl World {
    /// Upstream's `throwingData`.
    pub fn throwing() -> Self {
        World {
            harness: Harness::default(),
            sync: Some("sync"),
            promise: Some("promise"),
            sync_non_null: "syncNonNull",
            promise_non_null: "promiseNonNull",
            promise_delay: 0,
        }
    }

    /// Upstream's `nullingData` at the nullable leaves.
    pub fn nulling() -> Self {
        World {
            sync: None,
            promise: None,
            ..Self::throwing()
        }
    }
}

impl HasHarness for World {
    fn harness(&self) -> &Harness {
        &self.harness
    }
}

impl Area for World {
    type Info = schema::__private::Info;
    type Query = Data;
    type Mutation = NoMutation;

    fn builder() -> SchemaBuilder<Self::Info, Self, Self::Query, Self::Mutation> {
        schema::Schema::<World>::builder().query::<Data>()
    }

    fn roots() -> Roots<Data> {
        Roots::query(Data)
    }
}

pub type Schema = schema::Schema<World, Data>;

/// One `DataType` object: the root and every nest alike.
pub struct Data;

fn leaf(value: Option<&'static str>) -> Result<Option<String>, Error> {
    match value {
        Some(message) => Err(Error::new(message)),
        None => Ok(None),
    }
}

#[greem::object(schema = crate::graphql_js::nonnull::schema, type = "DataType", context = World)]
impl Data {
    async fn sync(
        &self,
        _args: &Args<schema::DataType::sync>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        let world = ctx.app();
        world.harness.trace("DataType.sync", 1).await;
        leaf(world.sync)
    }
    async fn sync_non_null(
        &self,
        _args: &Args<schema::DataType::syncNonNull>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        let world = ctx.app();
        world.harness.trace("DataType.syncNonNull", 1).await;
        Err(Error::new(world.sync_non_null))
    }
    async fn promise(
        &self,
        _args: &Args<schema::DataType::promise>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        let world = ctx.app();
        world.harness.trace("DataType.promise", 1).await;
        for _ in 0..=world.promise_delay {
            pending_once().await;
        }
        leaf(world.promise)
    }
    async fn promise_non_null(
        &self,
        _args: &Args<schema::DataType::promiseNonNull>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        let world = ctx.app();
        world.harness.trace("DataType.promiseNonNull", 1).await;
        pending_once().await;
        Err(Error::new(world.promise_non_null))
    }
    async fn sync_nest(
        &self,
        _args: &Args<schema::DataType::syncNest>,
        ctx: &Context<World>,
    ) -> Option<Data> {
        ctx.app().harness.trace("DataType.syncNest", 1).await;
        Some(Data)
    }
    async fn sync_non_null_nest(
        &self,
        _args: &Args<schema::DataType::syncNonNullNest>,
        ctx: &Context<World>,
    ) -> Data {
        ctx.app().harness.trace("DataType.syncNonNullNest", 1).await;
        Data
    }
    async fn promise_nest(
        &self,
        _args: &Args<schema::DataType::promiseNest>,
        ctx: &Context<World>,
    ) -> Option<Data> {
        ctx.app().harness.trace("DataType.promiseNest", 1).await;
        pending_once().await;
        Some(Data)
    }
    async fn promise_non_null_nest(
        &self,
        _args: &Args<schema::DataType::promiseNonNullNest>,
        ctx: &Context<World>,
    ) -> Data {
        ctx.app()
            .harness
            .trace("DataType.promiseNonNullNest", 1)
            .await;
        pending_once().await;
        Data
    }
    async fn with_non_null_arg(
        &self,
        args: &Args<schema::DataType::withNonNullArg>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("DataType.withNonNullArg", 1).await;
        Some(format!("Passed: {}", args.cannotBeNull))
    }
}
