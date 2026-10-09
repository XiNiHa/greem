//! The area for graphql-js v17.0.2 `src/execution/__tests__/schema-test.ts`:
//! its blog schema, world and resolvers. The world holds the articles the
//! case writes out; `Query.feed` lists them, `Query.article` and
//! `Author.recentArticle` look them up by id.

use crate::harness::{Area, Harness, HasHarness};
use greem::{Args, Context, NoMutation, Roots, SchemaBuilder};

#[allow(clippy::module_inception)]
pub mod schema {
    greem::include_schema!("graphql_js_schema.rs");
}

#[derive(Clone, Debug, Default)]
pub struct World {
    pub harness: Harness,
    pub articles: Vec<Article>,
}

impl World {
    fn article(&self, id: &str) -> Option<Article> {
        self.articles.iter().find(|a| a.id == id).cloned()
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

#[derive(Clone, Debug)]
pub struct Article {
    pub id: String,
    pub is_published: bool,
    pub author: Author,
    pub title: String,
    pub body: String,
    pub keywords: Vec<Option<String>>,
}

#[derive(Clone, Debug)]
pub struct Author {
    pub id: String,
    pub name: String,
    /// Upstream's `getPic(uid, ...)`: the url names the author.
    pub pic_url: String,
    /// The id of the article `recentArticle` resolves to.
    pub recent_article: String,
}

pub struct Image {
    pub url: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
}

#[greem::object(schema = crate::graphql_js::schema::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn article(
        &self,
        args: &Args<schema::Query::article>,
        ctx: &Context<World>,
    ) -> Option<Article> {
        ctx.app().harness.trace("Query.article", 1).await;
        ctx.app().article(args.id.as_deref()?)
    }
    async fn feed(
        &self,
        _args: &Args<schema::Query::feed>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<Article>>> {
        ctx.app().harness.trace("Query.feed", 1).await;
        Some(ctx.app().articles.iter().cloned().map(Some).collect())
    }
}

#[greem::object(schema = crate::graphql_js::schema::schema, type = "Article", context = World)]
impl Article {
    async fn id(&self, _args: &Args<schema::Article::id>, ctx: &Context<World>) -> &str {
        ctx.app().harness.trace("Article.id", 1).await;
        &self.id
    }
    async fn is_published(
        &self,
        _args: &Args<schema::Article::isPublished>,
        ctx: &Context<World>,
    ) -> Option<bool> {
        ctx.app().harness.trace("Article.isPublished", 1).await;
        Some(self.is_published)
    }
    async fn author(
        &self,
        _args: &Args<schema::Article::author>,
        ctx: &Context<World>,
    ) -> Option<&Author> {
        ctx.app().harness.trace("Article.author", 1).await;
        Some(&self.author)
    }
    async fn title(
        &self,
        _args: &Args<schema::Article::title>,
        ctx: &Context<World>,
    ) -> Option<&str> {
        ctx.app().harness.trace("Article.title", 1).await;
        Some(&self.title)
    }
    async fn body(
        &self,
        _args: &Args<schema::Article::body>,
        ctx: &Context<World>,
    ) -> Option<&str> {
        ctx.app().harness.trace("Article.body", 1).await;
        Some(&self.body)
    }
    async fn keywords(
        &self,
        _args: &Args<schema::Article::keywords>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<String>>> {
        ctx.app().harness.trace("Article.keywords", 1).await;
        Some(self.keywords.clone())
    }
}

#[greem::object(schema = crate::graphql_js::schema::schema, type = "Author", context = World)]
impl Author {
    async fn id(&self, _args: &Args<schema::Author::id>, ctx: &Context<World>) -> Option<&str> {
        ctx.app().harness.trace("Author.id", 1).await;
        Some(&self.id)
    }
    async fn name(&self, _args: &Args<schema::Author::name>, ctx: &Context<World>) -> Option<&str> {
        ctx.app().harness.trace("Author.name", 1).await;
        Some(&self.name)
    }
    async fn pic(&self, args: &Args<schema::Author::pic>, ctx: &Context<World>) -> Option<Image> {
        ctx.app().harness.trace("Author.pic", 1).await;
        Some(Image {
            url: self.pic_url.clone(),
            width: args.width,
            height: args.height,
        })
    }
    async fn recent_article(
        &self,
        _args: &Args<schema::Author::recentArticle>,
        ctx: &Context<World>,
    ) -> Option<Article> {
        ctx.app().harness.trace("Author.recentArticle", 1).await;
        ctx.app().article(&self.recent_article)
    }
}

#[greem::object(schema = crate::graphql_js::schema::schema, type = "Image", context = World)]
impl Image {
    async fn url(&self, _args: &Args<schema::Image::url>, ctx: &Context<World>) -> Option<&str> {
        ctx.app().harness.trace("Image.url", 1).await;
        Some(&self.url)
    }
    async fn width(&self, _args: &Args<schema::Image::width>, ctx: &Context<World>) -> Option<i32> {
        ctx.app().harness.trace("Image.width", 1).await;
        self.width
    }
    async fn height(
        &self,
        _args: &Args<schema::Image::height>,
        ctx: &Context<World>,
    ) -> Option<i32> {
        ctx.app().harness.trace("Image.height", 1).await;
        self.height
    }
}
