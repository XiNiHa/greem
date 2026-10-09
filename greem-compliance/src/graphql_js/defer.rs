//! The area for graphql-js v17.0.2 `src/execution/incremental/__tests__/defer-test.ts`:
//! its schema, world and resolvers. The world is upstream's `rootValue`,
//! written out per case: each object's data, which leaves error, and which
//! resolvers are slow.

use crate::harness::{Area, Harness, HasHarness, pending_once};
use futures::StreamExt;
use futures::stream::BoxStream;
use greem::{Args, Context, Error, NoMutation, Roots, SchemaBuilder, Streamed};

pub mod schema {
    greem::include_schema!("graphql_js_defer.rs");
}

/// A nullable leaf as the case writes it: a value, `null`, or the message of
/// the error its resolver raises (upstream's throwing resolver).
pub type Leaf<T> = Result<Option<T>, &'static str>;

/// A non-null leaf: a value or the message of the error its resolver raises.
/// Upstream returns `null` there and lets the executor raise the error; greem's
/// type encoding makes a resolver raise it directly, with the same text.
pub type NonNull<T> = Result<T, &'static str>;

fn leaf<T: Clone>(leaf: &Leaf<T>) -> Result<Option<T>, Error> {
    leaf.clone().map_err(Error::new)
}

fn non_null<T: Clone>(leaf: &NonNull<T>) -> Result<T, Error> {
    leaf.clone().map_err(Error::new)
}

#[derive(Clone, Debug, Default)]
pub struct World {
    pub harness: Harness,
    pub hero: Option<Hero>,
    pub a: Option<A>,
    pub g: Option<G>,
    pub parent: Option<LateParent>,
    /// `Type.field` names whose resolver yields once before answering, once
    /// per listing: upstream's promise-returning and `resolveOnNextTick`
    /// resolvers.
    pub slow: Vec<&'static str>,
}

impl World {
    /// Upstream's default `rootValue`, `{ hero }`.
    pub fn with_hero(hero: Hero) -> Self {
        World {
            hero: Some(hero),
            ..Default::default()
        }
    }

    async fn trace(&self, name: &'static str) {
        self.harness.trace(name, 1).await;
        for _ in self.slow.iter().filter(|slow| **slow == name) {
            pending_once().await;
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
    type Query = QueryRoot;
    type Mutation = NoMutation;

    fn builder() -> SchemaBuilder<Self::Info, Self, Self::Query, Self::Mutation> {
        schema::Schema::<World>::builder().query::<QueryRoot>()
    }

    fn roots() -> Roots<QueryRoot> {
        Roots {
            query: QueryRoot,
            mutation: NoMutation,
        }
    }
}

pub type Schema = schema::Schema<World, QueryRoot>;

pub struct QueryRoot;

#[derive(Clone, Debug)]
pub struct Hero {
    pub id: Leaf<String>,
    pub name: Leaf<String>,
    pub non_null_name: NonNull<String>,
    pub friends: Option<Vec<Friend>>,
    /// The list is produced item by item with a yield before each: upstream's
    /// async-iterable `friends`. The field is never streamed, so it drains in
    /// place either way.
    pub friends_async: bool,
    pub nested_object: Option<NestedObject>,
    pub another_nested_object: Option<AnotherNestedObject>,
}

impl Hero {
    /// Upstream's `hero` constant. Its `nestedObject` and
    /// `anotherNestedObject` are type objects there, which no case reads
    /// without overriding them; they are `null` here.
    pub fn luke() -> Self {
        Hero {
            id: Ok(Some("1".into())),
            name: Ok(Some("Luke".into())),
            non_null_name: Err("Hero.nonNullName is unset"),
            friends: Some(friends()),
            friends_async: false,
            nested_object: None,
            another_nested_object: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Friend {
    pub id: Leaf<String>,
    pub name: Leaf<String>,
    pub non_null_name: NonNull<String>,
}

impl Friend {
    pub fn new(id: &str, name: &str) -> Self {
        Friend {
            id: Ok(Some(id.into())),
            name: Ok(Some(name.into())),
            non_null_name: Err("Friend.nonNullName is unset"),
        }
    }
}

/// Upstream's `friends` constant.
pub fn friends() -> Vec<Friend> {
    vec![
        Friend::new("2", "Han"),
        Friend::new("3", "Leia"),
        Friend::new("4", "C-3PO"),
    ]
}

#[derive(Clone, Debug)]
pub struct NestedObject {
    pub deeper_object: Option<DeeperObject>,
    pub name: Leaf<String>,
}

impl Default for NestedObject {
    fn default() -> Self {
        NestedObject {
            deeper_object: None,
            name: Ok(None),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AnotherNestedObject {
    pub deeper_object: Option<DeeperObject>,
}

#[derive(Clone, Debug)]
pub struct DeeperObject {
    pub foo: Leaf<String>,
    pub bar: Leaf<String>,
    pub baz: Leaf<String>,
    pub bak: Leaf<String>,
}

impl Default for DeeperObject {
    fn default() -> Self {
        DeeperObject {
            foo: Ok(None),
            bar: Ok(None),
            baz: Ok(None),
            bak: Ok(None),
        }
    }
}

#[derive(Clone, Debug)]
pub struct A {
    pub b: Option<B>,
    pub some_field: Leaf<String>,
    pub non_null_error_field: NonNull<String>,
}

#[derive(Clone, Debug, Default)]
pub struct B {
    pub c: Option<C>,
    pub e: Option<E>,
}

#[derive(Clone, Debug)]
pub struct C {
    pub d: Leaf<String>,
    pub non_null_error_field: NonNull<String>,
}

#[derive(Clone, Debug)]
pub struct E {
    pub f: Leaf<String>,
}

impl Default for E {
    fn default() -> Self {
        E { f: Ok(None) }
    }
}

#[derive(Clone, Debug)]
pub struct G {
    pub h: Leaf<String>,
}

impl Default for G {
    fn default() -> Self {
        G { h: Ok(None) }
    }
}

#[derive(Clone, Debug)]
pub struct LateParent {
    pub boom: NonNull<String>,
    pub side: Option<LateSide>,
}

#[derive(Clone, Debug)]
pub struct LateSide {
    pub value: Leaf<String>,
}

impl Default for LateSide {
    fn default() -> Self {
        LateSide { value: Ok(None) }
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn hero(&self, _args: &Args<schema::Query::hero>, ctx: &Context<World>) -> Option<Hero> {
        ctx.app().trace("Query.hero").await;
        ctx.app().hero.clone()
    }
    async fn a(&self, _args: &Args<schema::Query::a>, ctx: &Context<World>) -> Option<A> {
        ctx.app().trace("Query.a").await;
        ctx.app().a.clone()
    }
    async fn g(&self, _args: &Args<schema::Query::g>, ctx: &Context<World>) -> Option<G> {
        ctx.app().trace("Query.g").await;
        ctx.app().g.clone()
    }
    async fn parent(
        &self,
        _args: &Args<schema::Query::parent>,
        ctx: &Context<World>,
    ) -> Option<LateParent> {
        ctx.app().trace("Query.parent").await;
        ctx.app().parent.clone()
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "Hero", context = World)]
impl Hero {
    async fn id(
        &self,
        _args: &Args<schema::Hero::id>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("Hero.id").await;
        leaf(&self.id)
    }
    async fn name(
        &self,
        _args: &Args<schema::Hero::name>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("Hero.name").await;
        leaf(&self.name)
    }
    async fn non_null_name(
        &self,
        _args: &Args<schema::Hero::nonNullName>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app().trace("Hero.nonNullName").await;
        non_null(&self.non_null_name)
    }
    async fn friends(
        &self,
        _args: &Args<schema::Hero::friends>,
        ctx: &Context<World>,
    ) -> Option<Streamed<BoxStream<'static, Result<Option<Friend>, Error>>>> {
        ctx.app().trace("Hero.friends").await;
        let paced = self.friends_async;
        self.friends.clone().map(|friends| {
            Streamed::new(
                futures::stream::iter(friends)
                    .then(move |friend| async move {
                        if paced {
                            pending_once().await;
                        }
                        Ok(Some(friend))
                    })
                    .boxed(),
            )
        })
    }
    async fn nested_object(
        &self,
        _args: &Args<schema::Hero::nestedObject>,
        ctx: &Context<World>,
    ) -> Option<NestedObject> {
        ctx.app().trace("Hero.nestedObject").await;
        self.nested_object.clone()
    }
    async fn another_nested_object(
        &self,
        _args: &Args<schema::Hero::anotherNestedObject>,
        ctx: &Context<World>,
    ) -> Option<AnotherNestedObject> {
        ctx.app().trace("Hero.anotherNestedObject").await;
        self.another_nested_object.clone()
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "Friend", context = World)]
impl Friend {
    async fn id(
        &self,
        _args: &Args<schema::Friend::id>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("Friend.id").await;
        leaf(&self.id)
    }
    async fn name(
        &self,
        _args: &Args<schema::Friend::name>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("Friend.name").await;
        leaf(&self.name)
    }
    async fn non_null_name(
        &self,
        _args: &Args<schema::Friend::nonNullName>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app().trace("Friend.nonNullName").await;
        non_null(&self.non_null_name)
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "NestedObject", context = World)]
impl NestedObject {
    async fn deeper_object(
        &self,
        _args: &Args<schema::NestedObject::deeperObject>,
        ctx: &Context<World>,
    ) -> Option<DeeperObject> {
        ctx.app().trace("NestedObject.deeperObject").await;
        self.deeper_object.clone()
    }
    async fn name(
        &self,
        _args: &Args<schema::NestedObject::name>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("NestedObject.name").await;
        leaf(&self.name)
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "AnotherNestedObject", context = World)]
impl AnotherNestedObject {
    async fn deeper_object(
        &self,
        _args: &Args<schema::AnotherNestedObject::deeperObject>,
        ctx: &Context<World>,
    ) -> Option<DeeperObject> {
        ctx.app().trace("AnotherNestedObject.deeperObject").await;
        self.deeper_object.clone()
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "DeeperObject", context = World)]
impl DeeperObject {
    async fn foo(
        &self,
        _args: &Args<schema::DeeperObject::foo>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("DeeperObject.foo").await;
        leaf(&self.foo)
    }
    async fn bar(
        &self,
        _args: &Args<schema::DeeperObject::bar>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("DeeperObject.bar").await;
        leaf(&self.bar)
    }
    async fn baz(
        &self,
        _args: &Args<schema::DeeperObject::baz>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("DeeperObject.baz").await;
        leaf(&self.baz)
    }
    async fn bak(
        &self,
        _args: &Args<schema::DeeperObject::bak>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("DeeperObject.bak").await;
        leaf(&self.bak)
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "A", context = World)]
impl A {
    async fn b(&self, _args: &Args<schema::A::b>, ctx: &Context<World>) -> Option<B> {
        ctx.app().trace("A.b").await;
        self.b.clone()
    }
    async fn some_field(
        &self,
        _args: &Args<schema::A::someField>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("A.someField").await;
        leaf(&self.some_field)
    }
    async fn non_null_error_field(
        &self,
        _args: &Args<schema::A::nonNullErrorField>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app().trace("A.nonNullErrorField").await;
        non_null(&self.non_null_error_field)
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "B", context = World)]
impl B {
    async fn c(&self, _args: &Args<schema::B::c>, ctx: &Context<World>) -> Option<C> {
        ctx.app().trace("B.c").await;
        self.c.clone()
    }
    async fn e(&self, _args: &Args<schema::B::e>, ctx: &Context<World>) -> Option<E> {
        ctx.app().trace("B.e").await;
        self.e.clone()
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "C", context = World)]
impl C {
    async fn d(
        &self,
        _args: &Args<schema::C::d>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("C.d").await;
        leaf(&self.d)
    }
    async fn non_null_error_field(
        &self,
        _args: &Args<schema::C::nonNullErrorField>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app().trace("C.nonNullErrorField").await;
        non_null(&self.non_null_error_field)
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "E", context = World)]
impl E {
    async fn f(
        &self,
        _args: &Args<schema::E::f>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("E.f").await;
        leaf(&self.f)
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "G", context = World)]
impl G {
    async fn h(
        &self,
        _args: &Args<schema::G::h>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("G.h").await;
        leaf(&self.h)
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "LateParent", context = World)]
impl LateParent {
    async fn boom(
        &self,
        _args: &Args<schema::LateParent::boom>,
        ctx: &Context<World>,
    ) -> Result<String, Error> {
        ctx.app().trace("LateParent.boom").await;
        non_null(&self.boom)
    }
    async fn side(
        &self,
        _args: &Args<schema::LateParent::side>,
        ctx: &Context<World>,
    ) -> Option<LateSide> {
        ctx.app().trace("LateParent.side").await;
        self.side.clone()
    }
}

#[greem::object(schema = crate::graphql_js::defer::schema, type = "LateSide", context = World)]
impl LateSide {
    async fn value(
        &self,
        _args: &Args<schema::LateSide::value>,
        ctx: &Context<World>,
    ) -> Result<Option<String>, Error> {
        ctx.app().trace("LateSide.value").await;
        leaf(&self.value)
    }
}
