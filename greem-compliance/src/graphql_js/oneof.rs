//! The area for graphql-js v17.0.2 `src/execution/__tests__/oneof-test.ts`:
//! its schema, world and resolvers. `Query.test` echoes its `@oneOf` input
//! back as a `TestObject`, as upstream's `rootValue.test` does.

use crate::harness::{Area, Harness, HasHarness};
use greem::{Args, Context, NoMutation, Roots, SchemaBuilder};

pub mod schema {
    greem::include_schema!("graphql_js_oneof.rs");
}

#[derive(Clone, Debug, Default)]
pub struct World {
    pub harness: Harness,
}

impl HasHarness for World {
    fn harness(&self) -> &Harness {
        &self.harness
    }
}

impl Area for World {
    type Info = schema::__private::Info;
    type Query = QueryRoot;
    type Mutation = NoMutation;

    fn builder() -> SchemaBuilder<Self::Info, Self, Self::Query, Self::Mutation> {
        schema::Schema::<World>::builder().query::<QueryRoot>()
    }

    fn roots() -> Roots<QueryRoot, NoMutation> {
        Roots::query(QueryRoot)
    }
}

pub type Schema = schema::Schema<World, QueryRoot>;

pub struct QueryRoot;

pub struct TestObject {
    pub a: Option<String>,
    pub b: Option<i32>,
}

#[greem::object(schema = crate::graphql_js::oneof::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn test(
        &self,
        args: &Args<schema::Query::test>,
        ctx: &Context<World>,
    ) -> Option<TestObject> {
        ctx.app().harness.trace("Query.test", 1).await;
        Some(TestObject {
            a: args.input.a.clone(),
            b: args.input.b,
        })
    }
}

#[greem::object(schema = crate::graphql_js::oneof::schema, type = "TestObject", context = World)]
impl TestObject {
    async fn a(&self, _args: &Args<schema::TestObject::a>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("TestObject.a", 1).await;
        self.a.clone()
    }
    async fn b(&self, _args: &Args<schema::TestObject::b>, ctx: &Context<World>) -> Option<i32> {
        ctx.app().harness.trace("TestObject.b", 1).await;
        self.b
    }
}
