//! The area for graphql-js v17.0.2 `src/execution/__tests__/executor-test.ts`:
//! its schema, world and resolvers. Upstream builds one root type per case
//! (`DataType`, `Type`, `Query`, ...); here they are interfaces the one root
//! implements, so each case's fragments spread at the root unchanged.

use crate::harness::{Area, Harness, HasHarness, pending_once};
use greem::{Args, As, Context, Error, Roots, SchemaBuilder};

pub mod schema {
    greem::include_schema!("graphql_js_executor.rs");
}

/// Upstream's `rootValue`, field by field; resolvers upstream writes as code
/// (`() => 'Apple'`, `throw new Error(..)`) stay code here.
#[derive(Clone, Debug, Default)]
pub struct World {
    pub harness: Harness,
    pub a: Option<String>,
    pub b: Option<String>,
    pub c: Option<String>,
    pub d: Option<String>,
    pub e: Option<String>,
    pub f: Option<String>,
    /// Root fields read after one yield: upstream's promise-returning resolvers.
    pub promised: &'static [&'static str],
    pub deep: Option<DeepData>,
    /// `DataType.promise`: the root again.
    pub promise: Option<QueryRoot>,
    /// `Type.deep`: the root again.
    pub deep_type: Option<QueryRoot>,
    /// The message `asyncError` fails with after one yield; two cases share
    /// the field with different messages.
    pub async_error: Option<&'static str>,
    pub foo: Option<String>,
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

/// The root object; also the value at every `DataType` and `Type` position,
/// since upstream's `promise`, `deeper` and `deep` hand the root back.
#[derive(Clone, Copy, Debug, Default)]
pub struct QueryRoot;
pub struct MutationRoot;

/// Upstream's `deepData`.
#[derive(Clone, Debug, Default)]
pub struct DeepData {
    pub a: Option<String>,
    pub b: Option<String>,
    pub c: Option<Vec<Option<String>>>,
    pub deeper: Option<Vec<Option<QueryRoot>>>,
}

/// Every field resolves by code upstream (`() => ({})`, `throw`).
pub struct A;

pub struct Food;

type Member = As<schema::types::Query, QueryRoot>;

impl World {
    async fn root_field(&self, name: &'static str, value: &Option<String>) -> Option<String> {
        if self.promised.contains(&name) {
            pending_once().await;
        }
        value.clone()
    }
}

#[greem::object(schema = crate::graphql_js::executor::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn a(&self, _args: &Args<schema::Query::a>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Query.a", 1).await;
        ctx.app().root_field("a", &ctx.app().a).await
    }
    async fn b(&self, _args: &Args<schema::Query::b>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Query.b", 1).await;
        ctx.app().root_field("b", &ctx.app().b).await
    }
    async fn c(&self, _args: &Args<schema::Query::c>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Query.c", 1).await;
        ctx.app().root_field("c", &ctx.app().c).await
    }
    async fn d(&self, _args: &Args<schema::Query::d>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Query.d", 1).await;
        ctx.app().root_field("d", &ctx.app().d).await
    }
    async fn e(&self, _args: &Args<schema::Query::e>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Query.e", 1).await;
        ctx.app().root_field("e", &ctx.app().e).await
    }
    async fn f(&self, _args: &Args<schema::Query::f>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Query.f", 1).await;
        ctx.app().root_field("f", &ctx.app().f).await
    }
    async fn pic(&self, args: &Args<schema::Query::pic>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Query.pic", 1).await;
        args.size.map(|size| format!("Pic of size: {size}"))
    }
    async fn deep(
        &self,
        _args: &Args<schema::Query::deep>,
        ctx: &Context<World>,
    ) -> Option<DeepData> {
        ctx.app().harness.trace("Query.deep", 1).await;
        ctx.app().deep.clone()
    }
    async fn promise(
        &self,
        _args: &Args<schema::Query::promise>,
        ctx: &Context<World>,
    ) -> Option<Member> {
        ctx.app().harness.trace("Query.promise", 1).await;
        pending_once().await;
        ctx.app().promise.map(As::new)
    }
    async fn deep_type(
        &self,
        _args: &Args<schema::Query::deepType>,
        ctx: &Context<World>,
    ) -> Option<Member> {
        ctx.app().harness.trace("Query.deepType", 1).await;
        ctx.app().deep_type.map(As::new)
    }
    /// Upstream's `inspect(args)`: the arguments that were set, in order.
    async fn field(
        &self,
        args: &Args<schema::Query::field>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("Query.field", 1).await;
        let mut set = Vec::new();
        for (name, value) in [("a", &args.a), ("b", &args.b), ("c", &args.c)] {
            if let Some(v) = value {
                set.push(format!("{name}: {v}"));
            }
        }
        for (name, value) in [("d", &args.d), ("e", &args.e)] {
            if let Some(v) = value {
                set.push(format!("{name}: {v}"));
            }
        }
        Some(format!("{{ {} }}", set.join(", ")))
    }
    async fn sync(
        &self,
        _args: &Args<schema::Query::sync>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("Query.sync", 1).await;
        Some("sync".into())
    }
    async fn sync_error(
        &self,
        _args: &Args<schema::Query::syncError>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().harness.trace("Query.syncError", 1).await;
        Err(Error::new("Error getting syncError"))
    }
    /// Upstream throws a bare string; the `Display` conversion is the analogue.
    async fn sync_raw_error(
        &self,
        _args: &Args<schema::Query::syncRawError>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().harness.trace("Query.syncRawError", 1).await;
        Err(Error::from("Error getting syncRawError"))
    }
    async fn sync_return_error(
        &self,
        _args: &Args<schema::Query::syncReturnError>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().harness.trace("Query.syncReturnError", 1).await;
        Err(Error::new("Error getting syncReturnError"))
    }
    async fn sync_return_error_list(
        &self,
        _args: &Args<schema::Query::syncReturnErrorList>,
        ctx: &Context<World>,
    ) -> Option<Vec<Result<Option<String>, Error>>> {
        ctx.app()
            .harness
            .trace("Query.syncReturnErrorList", 1)
            .await;
        Some(vec![
            Ok(Some("sync0".into())),
            Err(Error::new("Error getting syncReturnErrorList1")),
            Ok(Some("sync2".into())),
            Err(Error::new("Error getting syncReturnErrorList3")),
        ])
    }
    async fn r#async(
        &self,
        _args: &Args<schema::Query::r#async>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("Query.async", 1).await;
        pending_once().await;
        Some("async".into())
    }
    async fn async_reject(
        &self,
        _args: &Args<schema::Query::asyncReject>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().harness.trace("Query.asyncReject", 1).await;
        pending_once().await;
        Err(Error::new("Error getting asyncReject"))
    }
    async fn async_reject_with_extensions(
        &self,
        _args: &Args<schema::Query::asyncRejectWithExtensions>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app()
            .harness
            .trace("Query.asyncRejectWithExtensions", 1)
            .await;
        pending_once().await;
        Err(Error::new("Error getting asyncRejectWithExtensions").with_extension("foo", "bar"))
    }
    async fn async_raw_reject(
        &self,
        _args: &Args<schema::Query::asyncRawReject>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().harness.trace("Query.asyncRawReject", 1).await;
        pending_once().await;
        Err(Error::from("Error getting asyncRawReject"))
    }
    /// Upstream rejects with no value at all; the analogue is an empty message.
    async fn async_empty_reject(
        &self,
        _args: &Args<schema::Query::asyncEmptyReject>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().harness.trace("Query.asyncEmptyReject", 1).await;
        pending_once().await;
        Err(Error::new(""))
    }
    async fn async_error(
        &self,
        _args: &Args<schema::Query::asyncError>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().harness.trace("Query.asyncError", 1).await;
        pending_once().await;
        match ctx.app().async_error {
            Some(message) => Err(Error::new(message)),
            None => Ok(None),
        }
    }
    async fn async_raw_error(
        &self,
        _args: &Args<schema::Query::asyncRawError>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().harness.trace("Query.asyncRawError", 1).await;
        pending_once().await;
        Err(Error::from("Error getting asyncRawError"))
    }
    async fn async_return_error(
        &self,
        _args: &Args<schema::Query::asyncReturnError>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().harness.trace("Query.asyncReturnError", 1).await;
        pending_once().await;
        Err(Error::new("Error getting asyncReturnError"))
    }
    async fn async_return_error_with_extensions(
        &self,
        _args: &Args<schema::Query::asyncReturnErrorWithExtensions>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app()
            .harness
            .trace("Query.asyncReturnErrorWithExtensions", 1)
            .await;
        pending_once().await;
        Err(Error::new("Error getting asyncReturnErrorWithExtensions").with_extension("foo", "bar"))
    }
    async fn foods(
        &self,
        _args: &Args<schema::Query::foods>,
        ctx: &Context<World>,
    ) -> Result<Option<Vec<Option<Food>>>, Error> {
        ctx.app().harness.trace("Query.foods", 1).await;
        pending_once().await;
        Err(Error::new("Oops"))
    }
    /// Upstream returns null at the non-null position after one yield, which
    /// greem's type encoding forbids: the resolver fails instead.
    async fn async_non_null_error(
        &self,
        _args: &Args<schema::Query::asyncNonNullError>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app().harness.trace("Query.asyncNonNullError", 1).await;
        pending_once().await;
        Err(Error::new(
            "Cannot return null for non-nullable field Query.asyncNonNullError.",
        ))
    }
    async fn nullable_a(
        &self,
        _args: &Args<schema::Query::nullableA>,
        ctx: &Context<World>,
    ) -> Option<A> {
        ctx.app().harness.trace("Query.nullableA", 1).await;
        Some(A)
    }
    async fn a_object(
        &self,
        _args: &Args<schema::Query::aObject>,
        ctx: &Context<World>,
    ) -> Option<SomeType> {
        ctx.app().harness.trace("Query.aObject", 1).await;
        None
    }
    async fn foo(&self, _args: &Args<schema::Query::foo>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Query.foo", 1).await;
        ctx.app().foo.clone()
    }
}

#[greem::object(schema = crate::graphql_js::executor::schema, type = "DeepDataType", context = World)]
impl DeepData {
    async fn a(
        &self,
        _args: &Args<schema::DeepDataType::a>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("DeepDataType.a", 1).await;
        self.a.clone()
    }
    async fn b(
        &self,
        _args: &Args<schema::DeepDataType::b>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("DeepDataType.b", 1).await;
        self.b.clone()
    }
    async fn c(
        &self,
        _args: &Args<schema::DeepDataType::c>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<String>>> {
        ctx.app().harness.trace("DeepDataType.c", 1).await;
        self.c.clone()
    }
    async fn deeper(
        &self,
        _args: &Args<schema::DeepDataType::deeper>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<Member>>> {
        ctx.app().harness.trace("DeepDataType.deeper", 1).await;
        self.deeper
            .as_ref()
            .map(|items| items.iter().map(|item| item.map(As::new)).collect())
    }
}

#[greem::object(schema = crate::graphql_js::executor::schema, type = "Food", context = World)]
impl Food {
    async fn name(&self, _args: &Args<schema::Food::name>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Food.name", 1).await;
        None
    }
}

#[greem::object(schema = crate::graphql_js::executor::schema, type = "A", context = World)]
impl A {
    async fn nullable_a(
        &self,
        _args: &Args<schema::A::nullableA>,
        ctx: &Context<World>,
    ) -> Option<A> {
        ctx.app().harness.trace("A.nullableA", 1).await;
        Some(A)
    }
    async fn non_null_a(&self, _args: &Args<schema::A::nonNullA>, ctx: &Context<World>) -> A {
        ctx.app().harness.trace("A.nonNullA", 1).await;
        A
    }
    async fn throws(
        &self,
        _args: &Args<schema::A::throws>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app().harness.trace("A.throws", 1).await;
        Err(Error::new("Catch me if you can"))
    }
}

pub struct SomeType;

#[greem::object(schema = crate::graphql_js::executor::schema, type = "SomeType", context = World)]
impl SomeType {
    async fn b(&self, _args: &Args<schema::SomeType::b>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("SomeType.b", 1).await;
        None
    }
}

#[greem::object(schema = crate::graphql_js::executor::schema, type = "Mutation", context = World)]
impl MutationRoot {
    async fn c(&self, _args: &Args<schema::Mutation::c>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Mutation.c", 1).await;
        ctx.app().c.clone()
    }
}
