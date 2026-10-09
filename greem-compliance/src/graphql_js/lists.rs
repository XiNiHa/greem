//! The area for graphql-js v17.0.2 `src/execution/__tests__/lists-test.ts`:
//! its schema, world and resolvers. Upstream feeds one `listField` a JS value
//! per case; the world holds one such value per field, written out by the case.

use crate::harness::{Area, Harness, HasHarness, pending_once};
use futures::stream::{BoxStream, StreamExt};
use greem::{Args, Context, Error, Items, NoMutation, Roots, SchemaBuilder, Streamed};
use std::collections::BTreeSet;

pub mod schema {
    greem::include_schema!("graphql_js_lists.rs");
}

/// One list item: its value, or the error raised at its position.
pub type Item<T> = Result<T, &'static str>;

/// Upstream's `rootValue.listField` for one of the `[Int]` shapes.
#[derive(Clone, Debug, Default)]
pub enum Source<T> {
    /// `listField = null`.
    #[default]
    Null,
    /// `listField = new Error(message)`.
    Fail(&'static str),
    /// `listField = [...]`, each item a value or an error.
    Items(Vec<Item<T>>),
}

impl<T: Clone + Send + 'static> Source<T> {
    pub fn is_list(&self) -> bool {
        matches!(self, Source::Items(_))
    }

    fn owned(&self) -> Result<Option<Vec<Result<T, Error>>>, Error> {
        match self {
            Source::Null => Ok(None),
            Source::Fail(message) => Err(Error::new(*message)),
            Source::Items(items) => Ok(Some(items.iter().cloned().map(item).collect())),
        }
    }

    fn streamed(&self, yields: u32) -> Result<Option<StreamedItems<T>>, Error> {
        match self {
            Source::Null => Ok(None),
            Source::Fail(message) => Err(Error::new(*message)),
            Source::Items(items) => Ok(Some(streamed(items.clone(), yields))),
        }
    }
}

/// What `ObjectWrapper.index` does for the object at that position.
#[derive(Clone, Debug, Default)]
pub struct IndexResolution {
    pub yields: u32,
    pub error: Option<&'static str>,
}

#[derive(Clone, Debug, Default)]
pub struct World {
    pub harness: Harness,
    /// Yields before each item of a streamed field: 0 for a sync generator,
    /// 1 for an async one or a list of promises.
    pub item_yields: u32,
    pub string_set: BTreeSet<String>,
    pub boxed_strings: Vec<String>,
    pub streamed_strings: Vec<Item<Option<String>>>,
    pub streamed_floats: Vec<Option<f64>>,
    pub nullable_list_of_nullable: Source<Option<i32>>,
    pub non_null_list_of_nullable: Source<Option<i32>>,
    pub nullable_list_of_non_null: Source<i32>,
    pub non_null_list_of_non_null: Source<i32>,
    /// The objects of `streamedObjects` and `streamedNonNullObjects`, by index.
    pub objects: Vec<IndexResolution>,
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
        Roots {
            query: QueryRoot,
            mutation: NoMutation,
        }
    }
}

pub type Schema = schema::Schema<World, QueryRoot, NoMutation>;
pub type StreamedItems<T> = Streamed<BoxStream<'static, Result<T, Error>>>;

pub struct QueryRoot;

pub struct ObjectWrapper {
    pub index: usize,
    pub resolution: IndexResolution,
}

fn item<T>(item: Item<T>) -> Result<T, Error> {
    item.map_err(Error::new)
}

/// The items as a stream, each handed out after `yields` yields.
fn streamed<T: Send + 'static>(items: Vec<Item<T>>, yields: u32) -> StreamedItems<T> {
    Streamed::new(
        futures::stream::iter(items)
            .then(move |it| async move {
                for _ in 0..yields {
                    pending_once().await;
                }
                item(it)
            })
            .boxed(),
    )
}

fn required<T>(list: Result<Option<T>, Error>) -> Result<T, Error> {
    list.map(|list| list.expect("a non-null list field holds a list or fails"))
}

#[greem::object(schema = crate::graphql_js::lists::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn string_set(
        &self,
        _args: &Args<schema::Query::stringSet>,
        ctx: &Context<World>,
    ) -> Option<Items<BTreeSet<Option<String>>>> {
        ctx.app().harness.trace("Query.stringSet", 1).await;
        Some(Items(
            ctx.app().string_set.iter().cloned().map(Some).collect(),
        ))
    }

    async fn boxed_strings(
        &self,
        _args: &Args<schema::Query::boxedStrings>,
        ctx: &Context<World>,
    ) -> Option<Box<[Option<String>]>> {
        ctx.app().harness.trace("Query.boxedStrings", 1).await;
        Some(ctx.app().boxed_strings.iter().cloned().map(Some).collect())
    }

    async fn streamed_strings(
        &self,
        _args: &Args<schema::Query::streamedStrings>,
        ctx: &Context<World>,
    ) -> Option<StreamedItems<Option<String>>> {
        ctx.app().harness.trace("Query.streamedStrings", 1).await;
        Some(streamed(
            ctx.app().streamed_strings.clone(),
            ctx.app().item_yields,
        ))
    }

    async fn streamed_floats(
        &self,
        _args: &Args<schema::Query::streamedFloats>,
        ctx: &Context<World>,
    ) -> Option<StreamedItems<Option<f64>>> {
        ctx.app().harness.trace("Query.streamedFloats", 1).await;
        let items = ctx.app().streamed_floats.iter().map(|f| Ok(*f)).collect();
        Some(streamed(items, ctx.app().item_yields))
    }

    async fn nullable_list_of_nullable(
        &self,
        _args: &Args<schema::Query::nullableListOfNullable>,
        ctx: &Context<World>,
    ) -> Result<Option<Vec<Result<Option<i32>, Error>>>, Error> {
        ctx.app()
            .harness
            .trace("Query.nullableListOfNullable", 1)
            .await;
        ctx.app().nullable_list_of_nullable.owned()
    }

    async fn non_null_list_of_nullable(
        &self,
        _args: &Args<schema::Query::nonNullListOfNullable>,
        ctx: &Context<World>,
    ) -> Result<Vec<Result<Option<i32>, Error>>, Error> {
        ctx.app()
            .harness
            .trace("Query.nonNullListOfNullable", 1)
            .await;
        required(ctx.app().non_null_list_of_nullable.owned())
    }

    async fn nullable_list_of_non_null(
        &self,
        _args: &Args<schema::Query::nullableListOfNonNull>,
        ctx: &Context<World>,
    ) -> Result<Option<Vec<Result<i32, Error>>>, Error> {
        ctx.app()
            .harness
            .trace("Query.nullableListOfNonNull", 1)
            .await;
        ctx.app().nullable_list_of_non_null.owned()
    }

    async fn non_null_list_of_non_null(
        &self,
        _args: &Args<schema::Query::nonNullListOfNonNull>,
        ctx: &Context<World>,
    ) -> Result<Vec<Result<i32, Error>>, Error> {
        ctx.app()
            .harness
            .trace("Query.nonNullListOfNonNull", 1)
            .await;
        required(ctx.app().non_null_list_of_non_null.owned())
    }

    async fn streamed_nullable_list_of_nullable(
        &self,
        _args: &Args<schema::Query::streamedNullableListOfNullable>,
        ctx: &Context<World>,
    ) -> Result<Option<StreamedItems<Option<i32>>>, Error> {
        ctx.app()
            .harness
            .trace("Query.streamedNullableListOfNullable", 1)
            .await;
        ctx.app()
            .nullable_list_of_nullable
            .streamed(ctx.app().item_yields)
    }

    async fn streamed_non_null_list_of_nullable(
        &self,
        _args: &Args<schema::Query::streamedNonNullListOfNullable>,
        ctx: &Context<World>,
    ) -> Result<StreamedItems<Option<i32>>, Error> {
        ctx.app()
            .harness
            .trace("Query.streamedNonNullListOfNullable", 1)
            .await;
        required(
            ctx.app()
                .non_null_list_of_nullable
                .streamed(ctx.app().item_yields),
        )
    }

    async fn streamed_nullable_list_of_non_null(
        &self,
        _args: &Args<schema::Query::streamedNullableListOfNonNull>,
        ctx: &Context<World>,
    ) -> Result<Option<StreamedItems<i32>>, Error> {
        ctx.app()
            .harness
            .trace("Query.streamedNullableListOfNonNull", 1)
            .await;
        ctx.app()
            .nullable_list_of_non_null
            .streamed(ctx.app().item_yields)
    }

    async fn streamed_non_null_list_of_non_null(
        &self,
        _args: &Args<schema::Query::streamedNonNullListOfNonNull>,
        ctx: &Context<World>,
    ) -> Result<StreamedItems<i32>, Error> {
        ctx.app()
            .harness
            .trace("Query.streamedNonNullListOfNonNull", 1)
            .await;
        required(
            ctx.app()
                .non_null_list_of_non_null
                .streamed(ctx.app().item_yields),
        )
    }

    async fn streamed_objects(
        &self,
        _args: &Args<schema::Query::streamedObjects>,
        ctx: &Context<World>,
    ) -> Option<StreamedItems<Option<ObjectWrapper>>> {
        ctx.app().harness.trace("Query.streamedObjects", 1).await;
        let objects = ctx.app().objects();
        Some(streamed(
            objects.into_iter().map(|o| Ok(Some(o))).collect(),
            ctx.app().item_yields,
        ))
    }

    async fn streamed_non_null_objects(
        &self,
        _args: &Args<schema::Query::streamedNonNullObjects>,
        ctx: &Context<World>,
    ) -> Option<StreamedItems<ObjectWrapper>> {
        ctx.app()
            .harness
            .trace("Query.streamedNonNullObjects", 1)
            .await;
        let objects = ctx.app().objects();
        Some(streamed(
            objects.into_iter().map(Ok).collect(),
            ctx.app().item_yields,
        ))
    }
}

impl World {
    fn objects(&self) -> Vec<ObjectWrapper> {
        self.objects
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, resolution)| ObjectWrapper { index, resolution })
            .collect()
    }
}

#[greem::object(schema = crate::graphql_js::lists::schema, type = "ObjectWrapper", context = World)]
impl ObjectWrapper {
    async fn index(
        &self,
        _args: &Args<schema::ObjectWrapper::index>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app().harness.trace("ObjectWrapper.index", 1).await;
        for _ in 0..self.resolution.yields {
            pending_once().await;
        }
        match self.resolution.error {
            Some(message) => Err(Error::new(message)),
            None => Ok(self.index.to_string()),
        }
    }
}
