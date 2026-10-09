//! The area for graphql-js v17.0.2 `src/execution/incremental/__tests__/stream-test.ts`:
//! its schema, world and resolvers. Every list field's data is a [`Source`]:
//! upstream's sync list, list of promises or async iterable, each a
//! `Streamed` of owned items since one resolver has one output type (a sync
//! list is the ready stream a `Vec` becomes at a streamed field). Item
//! errors (`Err`) stand in for upstream's rejected promises, iterables that
//! throw, and nulls at non-null items, which the type encoding forbids.

use crate::harness::{Area, Harness, HasHarness, pending_once};
use futures::StreamExt;
use futures::stream::BoxStream;
use greem::{Args, Context, Error, NoMutation, Roots, SchemaBuilder, Streamed};

pub mod schema {
    greem::include_schema!("graphql_js_stream.rs");
}

/// A list field's data in the shape upstream's case produces it.
#[derive(Clone, Debug)]
pub enum Source<T> {
    /// A sync list: a ready stream, what a `Vec` becomes at a streamed field.
    List(Vec<Result<T, Error>>),
    /// A list of promises: the same ready stream after one yield.
    Promises(Vec<Result<T, Error>>),
    /// An async iterable: one yield before each item, then `end_yields`
    /// more before it ends (upstream's generator awaiting after its last yield).
    Iterable {
        items: Vec<Result<T, Error>>,
        end_yields: u32,
    },
}

impl<T> Default for Source<T> {
    fn default() -> Self {
        Source::List(Vec::new())
    }
}

impl<T: Clone + Send + 'static> Source<T> {
    async fn open(&self) -> Streamed<BoxStream<'static, Result<T, Error>>> {
        match self {
            Source::List(items) => Streamed::new(futures::stream::iter(items.clone()).boxed()),
            Source::Promises(items) => {
                pending_once().await;
                Streamed::new(futures::stream::iter(items.clone()).boxed())
            }
            Source::Iterable { items, end_yields } => {
                let end_yields = *end_yields;
                let items = futures::stream::iter(items.clone()).then(|item| async move {
                    pending_once().await;
                    item
                });
                let end = futures::stream::once(async move {
                    for _ in 0..end_yields {
                        pending_once().await;
                    }
                })
                .filter_map(|()| futures::future::ready(None));
                Streamed::new(items.chain(end).boxed())
            }
        }
    }
}

type Items<T> = Option<Streamed<BoxStream<'static, Result<T, Error>>>>;

async fn yields(n: u32) {
    for _ in 0..n {
        pending_once().await;
    }
}

#[derive(Clone, Debug, Default)]
pub struct World {
    pub harness: Harness,
    pub scalar_list: Source<Option<String>>,
    pub scalar_list_list: Source<Option<Vec<Option<String>>>>,
    pub friend_list: Source<Option<Friend>>,
    pub non_null_friend_list: Source<Friend>,
    pub nested_object: Option<NestedObject>,
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

pub type Schema = schema::Schema<World, QueryRoot, NoMutation>;

pub struct QueryRoot;

/// Upstream's `friends[index]`, with `nonNullName` its name.
pub fn friend(index: usize) -> Friend {
    let (id, name) = [(1, "Luke"), (2, "Han"), (3, "Leia")][index];
    Friend {
        id: Some(id.to_string()),
        name: Some(name.to_owned()),
        non_null_name: Ok(name.to_owned()),
        yields: 0,
    }
}

#[derive(Clone, Debug)]
pub struct Friend {
    pub id: Option<String>,
    pub name: Option<String>,
    pub non_null_name: Result<String, Error>,
    /// Yields before each field resolves: upstream's promise-valued fields.
    pub yields: u32,
}

#[derive(Clone, Debug)]
pub struct NestedObject {
    pub scalar_field: Result<Option<String>, Error>,
    pub non_null_scalar_field: Result<String, Error>,
    pub nested_friend_list: Source<Option<Friend>>,
    pub deeper_nested_object: Option<DeeperNestedObject>,
    /// Yields before each scalar field resolves.
    pub yields: u32,
}

impl Default for NestedObject {
    fn default() -> Self {
        NestedObject {
            scalar_field: Ok(None),
            non_null_scalar_field: Ok(String::new()),
            nested_friend_list: Source::default(),
            deeper_nested_object: None,
            yields: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeeperNestedObject {
    pub non_null_scalar_field: Result<String, Error>,
    pub deeper_nested_friend_list: Source<Option<Friend>>,
    /// Yields before `nonNullScalarField` resolves.
    pub yields: u32,
}

#[greem::object(schema = crate::graphql_js::stream::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn scalar_list(
        &self,
        _args: &Args<schema::Query::scalarList>,
        ctx: &Context<World>,
    ) -> Items<Option<String>> {
        ctx.app().harness.trace("Query.scalarList", 1).await;
        Some(ctx.app().scalar_list.open().await)
    }
    async fn scalar_list_list(
        &self,
        _args: &Args<schema::Query::scalarListList>,
        ctx: &Context<World>,
    ) -> Items<Option<Vec<Option<String>>>> {
        ctx.app().harness.trace("Query.scalarListList", 1).await;
        Some(ctx.app().scalar_list_list.open().await)
    }
    async fn friend_list(
        &self,
        _args: &Args<schema::Query::friendList>,
        ctx: &Context<World>,
    ) -> Items<Option<Friend>> {
        ctx.app().harness.trace("Query.friendList", 1).await;
        Some(ctx.app().friend_list.open().await)
    }
    async fn non_null_friend_list(
        &self,
        _args: &Args<schema::Query::nonNullFriendList>,
        ctx: &Context<World>,
    ) -> Items<Friend> {
        ctx.app().harness.trace("Query.nonNullFriendList", 1).await;
        Some(ctx.app().non_null_friend_list.open().await)
    }
    async fn nested_object(
        &self,
        _args: &Args<schema::Query::nestedObject>,
        ctx: &Context<World>,
    ) -> Option<NestedObject> {
        ctx.app().harness.trace("Query.nestedObject", 1).await;
        ctx.app().nested_object.clone()
    }
}

#[greem::object(schema = crate::graphql_js::stream::schema, type = "Friend", context = World)]
impl Friend {
    async fn id(&self, _args: &Args<schema::Friend::id>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Friend.id", 1).await;
        yields(self.yields).await;
        self.id.clone()
    }
    async fn name(
        &self,
        _args: &Args<schema::Friend::name>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("Friend.name", 1).await;
        yields(self.yields).await;
        self.name.clone()
    }
    async fn non_null_name(
        &self,
        _args: &Args<schema::Friend::nonNullName>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app().harness.trace("Friend.nonNullName", 1).await;
        yields(self.yields).await;
        self.non_null_name.clone()
    }
}

#[greem::object(schema = crate::graphql_js::stream::schema, type = "NestedObject", context = World)]
impl NestedObject {
    async fn scalar_field(
        &self,
        _args: &Args<schema::NestedObject::scalarField>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().harness.trace("NestedObject.scalarField", 1).await;
        yields(self.yields).await;
        self.scalar_field.clone()
    }
    async fn non_null_scalar_field(
        &self,
        _args: &Args<schema::NestedObject::nonNullScalarField>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app()
            .harness
            .trace("NestedObject.nonNullScalarField", 1)
            .await;
        yields(self.yields).await;
        self.non_null_scalar_field.clone()
    }
    async fn nested_friend_list(
        &self,
        _args: &Args<schema::NestedObject::nestedFriendList>,
        ctx: &Context<World>,
    ) -> Items<Option<Friend>> {
        ctx.app()
            .harness
            .trace("NestedObject.nestedFriendList", 1)
            .await;
        Some(self.nested_friend_list.open().await)
    }
    async fn deeper_nested_object(
        &self,
        _args: &Args<schema::NestedObject::deeperNestedObject>,
        ctx: &Context<World>,
    ) -> Option<DeeperNestedObject> {
        ctx.app()
            .harness
            .trace("NestedObject.deeperNestedObject", 1)
            .await;
        self.deeper_nested_object.clone()
    }
}

#[greem::object(schema = crate::graphql_js::stream::schema, type = "DeeperNestedObject", context = World)]
impl DeeperNestedObject {
    async fn non_null_scalar_field(
        &self,
        _args: &Args<schema::DeeperNestedObject::nonNullScalarField>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app()
            .harness
            .trace("DeeperNestedObject.nonNullScalarField", 1)
            .await;
        yields(self.yields).await;
        self.non_null_scalar_field.clone()
    }
    async fn deeper_nested_friend_list(
        &self,
        _args: &Args<schema::DeeperNestedObject::deeperNestedFriendList>,
        ctx: &Context<World>,
    ) -> Items<Option<Friend>> {
        ctx.app()
            .harness
            .trace("DeeperNestedObject.deeperNestedFriendList", 1)
            .await;
        Some(self.deeper_nested_friend_list.open().await)
    }
}
