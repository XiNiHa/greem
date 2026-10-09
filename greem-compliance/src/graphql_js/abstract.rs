//! The area for graphql-js v17.0.2 `src/execution/__tests__/abstract-test.ts`:
//! its schema, world and resolvers. Upstream resolves each pet's runtime type
//! with `isTypeOf`/`resolveType`; greem partitions statically, so the world
//! lists the pets as `Dog`/`Cat` values, and a throwing type resolver becomes
//! an error raised while completing each pet.

use crate::harness::{Area, Harness, HasHarness};
use greem::{Args, As, Context, Either, Error, NoMutation, Roots, SchemaBuilder};

pub mod schema {
    greem::include_schema!("graphql_js_abstract.rs");
}

#[derive(Clone, Debug, Default)]
pub struct World {
    pub harness: Harness,
    /// Upstream's `pets` resolver output.
    pub pets: Vec<Pet>,
    /// When set, completing every pet fails with this message: upstream's
    /// throwing `isTypeOf`/`resolveType`.
    pub pet_error: Option<&'static str>,
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

#[derive(Clone, Debug)]
pub enum Pet {
    Dog(Dog),
    Cat(Cat),
}

#[derive(Clone, Debug)]
pub struct Dog {
    pub name: &'static str,
    pub woofs: bool,
}

#[derive(Clone, Debug)]
pub struct Cat {
    pub name: &'static str,
    pub meows: bool,
}

pub type PetOut = Either<As<schema::types::Dog, Dog>, As<schema::types::Cat, Cat>>;

impl Pet {
    fn out(&self) -> PetOut {
        match self {
            Pet::Dog(dog) => Either::A(As::new(dog.clone())),
            Pet::Cat(cat) => Either::B(As::new(cat.clone())),
        }
    }
}

impl World {
    fn pets(&self) -> Option<Vec<Result<Option<PetOut>, Error>>> {
        Some(
            self.pets
                .iter()
                .map(|pet| match self.pet_error {
                    Some(message) => Err(Error::new(message)),
                    None => Ok(Some(pet.out())),
                })
                .collect(),
        )
    }
}

#[greem::object(schema = crate::graphql_js::r#abstract::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn pets(
        &self,
        _args: &Args<schema::Query::pets>,
        ctx: &Context<World>,
    ) -> Option<Vec<Result<Option<PetOut>, Error>>> {
        ctx.app().harness.trace("Query.pets", 1).await;
        ctx.app().pets()
    }
    async fn union_pets(
        &self,
        _args: &Args<schema::Query::unionPets>,
        ctx: &Context<World>,
    ) -> Option<Vec<Result<Option<PetOut>, Error>>> {
        ctx.app().harness.trace("Query.unionPets", 1).await;
        ctx.app().pets()
    }
}

#[greem::object(schema = crate::graphql_js::r#abstract::schema, type = "Dog", context = World)]
impl Dog {
    async fn name(
        &self,
        _args: &Args<schema::Dog::name>,
        ctx: &Context<World>,
    ) -> Option<&'static str> {
        ctx.app().harness.trace("Dog.name", 1).await;
        Some(self.name)
    }
    async fn woofs(&self, _args: &Args<schema::Dog::woofs>, ctx: &Context<World>) -> Option<bool> {
        ctx.app().harness.trace("Dog.woofs", 1).await;
        Some(self.woofs)
    }
}

#[greem::object(schema = crate::graphql_js::r#abstract::schema, type = "Cat", context = World)]
impl Cat {
    async fn name(
        &self,
        _args: &Args<schema::Cat::name>,
        ctx: &Context<World>,
    ) -> Option<&'static str> {
        ctx.app().harness.trace("Cat.name", 1).await;
        Some(self.name)
    }
    async fn meows(&self, _args: &Args<schema::Cat::meows>, ctx: &Context<World>) -> Option<bool> {
        ctx.app().harness.trace("Cat.meows", 1).await;
        Some(self.meows)
    }
}
