//! The area for graphql-js v17.0.2 `src/execution/__tests__/union-interface-test.ts`:
//! its schema, world and resolvers. Upstream's dataset is cyclic (a mother's
//! progeny is her child), so the world keeps each kind in a vector and the
//! objects are indices into it; `people[0]` is the root value.

use crate::harness::{Area, Harness, HasHarness};
use greem::{Args, As, Context, Either, NoMutation, Roots, SchemaBuilder};

pub mod schema {
    greem::include_schema!("graphql_js_union_interface.rs");
}

#[derive(Clone, Debug, Default)]
pub struct World {
    pub harness: Harness,
    pub dogs: Vec<DogData>,
    pub cats: Vec<CatData>,
    pub plants: Vec<PlantData>,
    /// `people[0]` is the root value.
    pub people: Vec<PersonData>,
}

/// One object of the world, by kind and index.
#[derive(Clone, Copy, Debug)]
pub enum Ref {
    Dog(usize),
    Cat(usize),
    Person(usize),
    Plant(usize),
}

#[derive(Clone, Debug)]
pub struct DogData {
    pub name: &'static str,
    pub barks: bool,
    pub mother: Option<usize>,
    pub father: Option<usize>,
    pub progeny: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct CatData {
    pub name: &'static str,
    pub meows: bool,
    pub mother: Option<usize>,
    pub father: Option<usize>,
    pub progeny: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct PlantData {
    pub name: &'static str,
}

#[derive(Clone, Debug)]
pub struct PersonData {
    pub name: &'static str,
    pub pets: Option<Vec<Ref>>,
    pub friends: Option<Vec<Ref>>,
    pub responsibilities: Option<Vec<Ref>>,
    pub progeny: Vec<usize>,
    pub mother: Option<usize>,
    pub father: Option<usize>,
}

impl World {
    /// Upstream's module-level dataset: `garfield` (cat 0) and `odie` (dog 0)
    /// with their mothers (cat 1, dog 1), `fern` (plant 0), `john` (person 0,
    /// the root) and `liz` (person 1).
    pub fn upstream() -> Self {
        World {
            harness: Harness::default(),
            dogs: vec![
                DogData {
                    name: "Odie",
                    barks: true,
                    mother: Some(1),
                    father: None,
                    progeny: vec![],
                },
                DogData {
                    name: "Odie's Mom",
                    barks: true,
                    mother: None,
                    father: None,
                    progeny: vec![0],
                },
            ],
            cats: vec![
                CatData {
                    name: "Garfield",
                    meows: false,
                    mother: Some(1),
                    father: None,
                    progeny: vec![],
                },
                CatData {
                    name: "Garfield's Mom",
                    meows: false,
                    mother: None,
                    father: None,
                    progeny: vec![0],
                },
            ],
            plants: vec![PlantData { name: "Fern" }],
            people: vec![
                PersonData {
                    name: "John",
                    pets: Some(vec![Ref::Cat(0), Ref::Dog(0)]),
                    friends: Some(vec![Ref::Person(1), Ref::Dog(0)]),
                    responsibilities: Some(vec![Ref::Cat(0), Ref::Plant(0)]),
                    progeny: vec![],
                    mother: None,
                    father: None,
                },
                PersonData {
                    name: "Liz",
                    pets: None,
                    friends: None,
                    responsibilities: None,
                    progeny: vec![],
                    mother: None,
                    father: None,
                },
            ],
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
    type Query = Person;
    type Mutation = NoMutation;

    fn builder() -> SchemaBuilder<Self::Info, Self, Self::Query, Self::Mutation> {
        schema::Schema::<World>::builder().query::<Person>()
    }

    fn roots() -> Roots<Person, NoMutation> {
        Roots::query(Person(0))
    }
}

pub type Schema = schema::Schema<World, Person, NoMutation>;

#[derive(Clone, Copy, Debug)]
pub struct Dog(pub usize);
#[derive(Clone, Copy, Debug)]
pub struct Cat(pub usize);
#[derive(Clone, Copy, Debug)]
pub struct Person(pub usize);
#[derive(Clone, Copy, Debug)]
pub struct Plant(pub usize);

pub struct TypeA {
    pub id: &'static str,
    pub name_a: &'static str,
}
pub struct TypeB {
    pub id: &'static str,
    pub name_b: &'static str,
}

use schema::types;

pub type PetOut = Either<As<types::Dog, Dog>, As<types::Cat, Cat>>;
pub type NamedOut = Either<
    As<types::Person, Person>,
    Either<As<types::Dog, Dog>, Either<As<types::Cat, Cat>, As<types::Plant, Plant>>>,
>;
pub type PetOrPlantOut =
    Either<As<types::Plant, Plant>, Either<As<types::Dog, Dog>, As<types::Cat, Cat>>>;
pub type SearchableOut = Either<As<types::TypeA, TypeA>, As<types::TypeB, TypeB>>;

impl Ref {
    fn pet(self) -> PetOut {
        match self {
            Ref::Dog(i) => Either::A(As::new(Dog(i))),
            Ref::Cat(i) => Either::B(As::new(Cat(i))),
            other => panic!("{other:?} is not a Pet"),
        }
    }

    fn named(self) -> NamedOut {
        match self {
            Ref::Person(i) => Either::A(As::new(Person(i))),
            Ref::Dog(i) => Either::B(Either::A(As::new(Dog(i)))),
            Ref::Cat(i) => Either::B(Either::B(Either::A(As::new(Cat(i))))),
            Ref::Plant(i) => Either::B(Either::B(Either::B(As::new(Plant(i))))),
        }
    }

    fn pet_or_plant(self) -> PetOrPlantOut {
        match self {
            Ref::Plant(i) => Either::A(As::new(Plant(i))),
            Ref::Dog(i) => Either::B(Either::A(As::new(Dog(i)))),
            Ref::Cat(i) => Either::B(Either::B(As::new(Cat(i)))),
            other => panic!("{other:?} is not a PetOrPlantType"),
        }
    }
}

fn list<T>(refs: &Option<Vec<Ref>>, out: impl Fn(Ref) -> T) -> Option<Vec<Option<T>>> {
    refs.as_ref()
        .map(|refs| refs.iter().map(|&r| Some(out(r))).collect())
}

#[greem::object(schema = crate::graphql_js::union_interface::schema, type = "Dog", context = World)]
impl Dog {
    async fn name(
        &self,
        _args: &Args<schema::Dog::name>,
        ctx: &Context<World>,
    ) -> Option<&'static str> {
        ctx.app().harness.trace("Dog.name", 1).await;
        Some(ctx.app().dogs[self.0].name)
    }
    async fn barks(&self, _args: &Args<schema::Dog::barks>, ctx: &Context<World>) -> Option<bool> {
        ctx.app().harness.trace("Dog.barks", 1).await;
        Some(ctx.app().dogs[self.0].barks)
    }
    async fn progeny(
        &self,
        _args: &Args<schema::Dog::progeny>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<Dog>>> {
        ctx.app().harness.trace("Dog.progeny", 1).await;
        Some(
            ctx.app().dogs[self.0]
                .progeny
                .iter()
                .map(|&i| Some(Dog(i)))
                .collect(),
        )
    }
    async fn mother(&self, _args: &Args<schema::Dog::mother>, ctx: &Context<World>) -> Option<Dog> {
        ctx.app().harness.trace("Dog.mother", 1).await;
        ctx.app().dogs[self.0].mother.map(Dog)
    }
    async fn father(&self, _args: &Args<schema::Dog::father>, ctx: &Context<World>) -> Option<Dog> {
        ctx.app().harness.trace("Dog.father", 1).await;
        ctx.app().dogs[self.0].father.map(Dog)
    }
}

#[greem::object(schema = crate::graphql_js::union_interface::schema, type = "Cat", context = World)]
impl Cat {
    async fn name(
        &self,
        _args: &Args<schema::Cat::name>,
        ctx: &Context<World>,
    ) -> Option<&'static str> {
        ctx.app().harness.trace("Cat.name", 1).await;
        Some(ctx.app().cats[self.0].name)
    }
    async fn meows(&self, _args: &Args<schema::Cat::meows>, ctx: &Context<World>) -> Option<bool> {
        ctx.app().harness.trace("Cat.meows", 1).await;
        Some(ctx.app().cats[self.0].meows)
    }
    async fn progeny(
        &self,
        _args: &Args<schema::Cat::progeny>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<Cat>>> {
        ctx.app().harness.trace("Cat.progeny", 1).await;
        Some(
            ctx.app().cats[self.0]
                .progeny
                .iter()
                .map(|&i| Some(Cat(i)))
                .collect(),
        )
    }
    async fn mother(&self, _args: &Args<schema::Cat::mother>, ctx: &Context<World>) -> Option<Cat> {
        ctx.app().harness.trace("Cat.mother", 1).await;
        ctx.app().cats[self.0].mother.map(Cat)
    }
    async fn father(&self, _args: &Args<schema::Cat::father>, ctx: &Context<World>) -> Option<Cat> {
        ctx.app().harness.trace("Cat.father", 1).await;
        ctx.app().cats[self.0].father.map(Cat)
    }
}

#[greem::object(schema = crate::graphql_js::union_interface::schema, type = "Plant", context = World)]
impl Plant {
    async fn name(
        &self,
        _args: &Args<schema::Plant::name>,
        ctx: &Context<World>,
    ) -> Option<&'static str> {
        ctx.app().harness.trace("Plant.name", 1).await;
        Some(ctx.app().plants[self.0].name)
    }
}

#[greem::object(schema = crate::graphql_js::union_interface::schema, type = "Person", context = World)]
impl Person {
    async fn name(
        &self,
        _args: &Args<schema::Person::name>,
        ctx: &Context<World>,
    ) -> Option<&'static str> {
        ctx.app().harness.trace("Person.name", 1).await;
        Some(ctx.app().people[self.0].name)
    }
    async fn pets(
        &self,
        _args: &Args<schema::Person::pets>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<PetOut>>> {
        ctx.app().harness.trace("Person.pets", 1).await;
        list(&ctx.app().people[self.0].pets, Ref::pet)
    }
    async fn friends(
        &self,
        _args: &Args<schema::Person::friends>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<NamedOut>>> {
        ctx.app().harness.trace("Person.friends", 1).await;
        list(&ctx.app().people[self.0].friends, Ref::named)
    }
    async fn responsibilities(
        &self,
        _args: &Args<schema::Person::responsibilities>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<PetOrPlantOut>>> {
        ctx.app().harness.trace("Person.responsibilities", 1).await;
        list(
            &ctx.app().people[self.0].responsibilities,
            Ref::pet_or_plant,
        )
    }
    async fn progeny(
        &self,
        _args: &Args<schema::Person::progeny>,
        ctx: &Context<World>,
    ) -> Option<Vec<Option<Person>>> {
        ctx.app().harness.trace("Person.progeny", 1).await;
        Some(
            ctx.app().people[self.0]
                .progeny
                .iter()
                .map(|&i| Some(Person(i)))
                .collect(),
        )
    }
    async fn mother(
        &self,
        _args: &Args<schema::Person::mother>,
        ctx: &Context<World>,
    ) -> Option<Person> {
        ctx.app().harness.trace("Person.mother", 1).await;
        ctx.app().people[self.0].mother.map(Person)
    }
    async fn father(
        &self,
        _args: &Args<schema::Person::father>,
        ctx: &Context<World>,
    ) -> Option<Person> {
        ctx.app().harness.trace("Person.father", 1).await;
        ctx.app().people[self.0].father.map(Person)
    }
    /// Upstream's `search` resolver: `a` and `b` are the only objects.
    async fn search(
        &self,
        args: &Args<schema::Person::search>,
        ctx: &Context<World>,
    ) -> Option<SearchableOut> {
        ctx.app().harness.trace("Person.search", 1).await;
        match args.id.as_deref() {
            Some("a") => Some(Either::A(As::new(TypeA {
                id: "a",
                name_a: "Object A",
            }))),
            Some("b") => Some(Either::B(As::new(TypeB {
                id: "b",
                name_b: "Object B",
            }))),
            _ => None,
        }
    }
}

#[greem::object(schema = crate::graphql_js::union_interface::schema, type = "TypeA", context = World)]
impl TypeA {
    async fn id(
        &self,
        _args: &Args<schema::TypeA::id>,
        ctx: &Context<World>,
    ) -> Option<&'static str> {
        ctx.app().harness.trace("TypeA.id", 1).await;
        Some(self.id)
    }
    async fn name_a(
        &self,
        _args: &Args<schema::TypeA::nameA>,
        ctx: &Context<World>,
    ) -> Option<&'static str> {
        ctx.app().harness.trace("TypeA.nameA", 1).await;
        Some(self.name_a)
    }
}

#[greem::object(schema = crate::graphql_js::union_interface::schema, type = "TypeB", context = World)]
impl TypeB {
    async fn id(
        &self,
        _args: &Args<schema::TypeB::id>,
        ctx: &Context<World>,
    ) -> Option<&'static str> {
        ctx.app().harness.trace("TypeB.id", 1).await;
        Some(self.id)
    }
    async fn name_b(
        &self,
        _args: &Args<schema::TypeB::nameB>,
        ctx: &Context<World>,
    ) -> Option<&'static str> {
        ctx.app().harness.trace("TypeB.nameB", 1).await;
        Some(self.name_b)
    }
}
