//! The area for graphql-js v17.0.2 `src/execution/__tests__/directives-test.ts`:
//! its schema, world and resolvers. The world is upstream's `rootValue`: the
//! two strings the root fields read.

use crate::harness::{Area, Harness, HasHarness};
use greem::{Args, Context, NoMutation, Roots, SchemaBuilder};

pub mod schema {
    greem::include_schema!("graphql_js_directives.rs");
}

#[derive(Debug, Default, Clone)]
pub struct World {
    pub harness: Harness,
    pub a: String,
    pub b: String,
}

impl World {
    /// Upstream's `rootValue`: `{ a: 'a', b: 'b' }`.
    pub fn new(a: &str, b: &str) -> Self {
        World {
            harness: Harness::default(),
            a: a.to_owned(),
            b: b.to_owned(),
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
    type Query = TestType;
    type Mutation = NoMutation;

    fn builder() -> SchemaBuilder<Self::Info, Self, Self::Query, Self::Mutation> {
        schema::Schema::<World>::builder().query::<TestType>()
    }

    fn roots() -> Roots<TestType, NoMutation> {
        Roots::query(TestType)
    }
}

pub type Schema = schema::Schema<World, TestType, NoMutation>;

pub struct TestType;

#[greem::object(schema = crate::graphql_js::directives::schema, type = "TestType", context = World)]
impl TestType {
    async fn a(&self, _args: &Args<schema::TestType::a>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("TestType.a", 1).await;
        Some(ctx.app().a.clone())
    }
    async fn b(&self, _args: &Args<schema::TestType::b>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("TestType.b", 1).await;
        Some(ctx.app().b.clone())
    }
}
