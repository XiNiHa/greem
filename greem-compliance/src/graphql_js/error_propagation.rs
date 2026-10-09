//! The area for graphql-js v17.0.2 `src/execution/__tests__/errorPropagation-test.ts`:
//! its schema, world and resolvers. The one field always throws, as
//! upstream's `throwingData.foo` does.

use crate::harness::{Area, Harness, HasHarness};
use greem::{Args, Context, Error, NoMutation, Roots, SchemaBuilder};

pub mod schema {
    greem::include_schema!("graphql_js_error_propagation.rs");
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

    fn roots() -> Roots<QueryRoot> {
        Roots::query(QueryRoot)
    }
}

pub type Schema = schema::Schema<World, QueryRoot>;

pub struct QueryRoot;

#[greem::object(schema = crate::graphql_js::error_propagation::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn foo(
        &self,
        _args: &Args<schema::Query::foo>,
        ctx: &Context<World>,
    ) -> Result<i32, Error> {
        ctx.app().harness.trace("Query.foo", 1).await;
        Err(Error::new("bar"))
    }
}
