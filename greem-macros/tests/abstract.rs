//! `#[derive(greem::Abstract)]` against `greem-test-app`'s generated schema module.

use futures::executor::block_on;
use greem::{
    Args, Context, Error, ExecuteOptions, HintRegistry, Operation, Outputs, Planning, Roots,
};
use greem_test_app::schema;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// The resolvers' call log, shared with the test that runs them.
#[derive(Clone, Default)]
struct App(Arc<Mutex<Vec<String>>>);

impl App {
    fn log(&self, entry: impl Into<String>) {
        self.0.lock().unwrap().push(entry.into());
    }
}

struct User {
    id: u32,
    name: &'static str,
}

struct Post {
    id: u32,
    title: &'static str,
    author: User,
}

#[greem::object(schema = crate::schema, context = App)]
impl User {
    fn id(&self) -> u32 {
        self.id
    }
    async fn name(
        parents: &[&Self],
        _args: &Args<schema::User::name>,
        ctx: &Context<App>,
    ) -> Vec<&'static str> {
        let names: Vec<&str> = parents.iter().map(|u| u.name).collect();
        ctx.app().log(format!("User.name {}", names.join(",")));
        names
    }
    fn email(&self, _args: &Args<schema::User::email>) -> Result<Option<String>, Error> {
        Err(Error::new(format!("no email for {}", self.name)))
    }
    fn posts(&self, _args: &Args<schema::User::posts>) -> Vec<Post> {
        vec![]
    }
}

#[greem::object(schema = crate::schema, context = App)]
impl Post {
    fn id(&self) -> u32 {
        self.id
    }
    fn title(&self) -> &str {
        self.title
    }
    #[greem(plan = "author")]
    fn author_plan(p: &mut Planning<'_, schema::Post::author, App>) {
        p.hint::<AuthorHint>(|h| h.0 = true);
    }
    fn author(&self) -> &User {
        &self.author
    }
}

/// Written by `Post.author` at tree build, accepted by `Query.search`.
#[derive(Default)]
struct AuthorHint(bool);

fn ann() -> User {
    User { id: 1, name: "Ann" }
}

fn bob() -> User {
    User { id: 2, name: "Bob" }
}

fn cid() -> User {
    User { id: 3, name: "Cid" }
}

fn hello() -> Post {
    Post {
        id: 10,
        title: "Hello",
        author: ann(),
    }
}

#[derive(greem::Abstract)]
#[greem(type = "SearchResult")]
enum Hit {
    User(User),
    Post(Post),
}

struct Query;

#[greem::object(schema = crate::schema, context = App)]
impl Query {
    fn users(&self) -> Vec<User> {
        vec![]
    }
    fn user(&self, _args: &Args<schema::Query::user>) -> Option<User> {
        None
    }
    fn node(
        &self,
        _args: &Args<schema::Query::node>,
    ) -> Option<greem::As<schema::types::User, User>> {
        None
    }
    #[greem(hints = "search")]
    fn search_hints(reg: &mut HintRegistry<'_>) {
        reg.accept::<AuthorHint>();
    }
    fn search(&self, _args: &Args<schema::Query::search>, ctx: &Context<App>) -> Vec<Hit> {
        ctx.app().log(format!(
            "Query.search author={}",
            ctx.hint::<AuthorHint>().0
        ));
        vec![Hit::User(ann()), Hit::Post(hello()), Hit::User(bob())]
    }
    fn ints(&self) -> Option<Vec<Vec<Option<i32>>>> {
        None
    }
}

/// User types named like the std and primitive items the derive's output
/// uses: it must name them by full path.
#[allow(dead_code, non_camel_case_types)]
mod shadowed {
    enum u32 {}
    enum bool {}
    enum Vec {}
    enum Option {}
    enum Result {}
    enum Send {}
    enum Sync {}
    enum Fn {}

    #[derive(greem::Abstract)]
    #[greem(type = "SearchResult")]
    enum Hit {
        User(super::User),
        Post(super::Post),
    }
}

/// One enum at both abstract positions, borrowing from the root.
#[derive(greem::Abstract)]
#[greem(schema = crate::schema, type = "Node")]
#[greem(type = "SearchResult")]
enum Found<'a> {
    #[greem(as = "User")]
    Person(&'a User),
    Post(Post),
}

struct Directory {
    people: Vec<User>,
}

#[greem::object(schema = crate::schema, type = "Query", context = App)]
impl Directory {
    fn users(&self) -> Vec<User> {
        vec![]
    }
    fn user(&self, _args: &Args<schema::Query::user>) -> Option<User> {
        None
    }
    fn node(&self, args: &Args<schema::Query::node>) -> Option<Found<'_>> {
        match args.id.as_str() {
            "10" => Some(Found::Post(hello())),
            id => self
                .people
                .iter()
                .find(|u| u.id.to_string() == id)
                .map(Found::Person),
        }
    }
    fn search(&self) -> Vec<Found<'_>> {
        self.people
            .iter()
            .map(Found::Person)
            .chain([Found::Post(hello())])
            .collect()
    }
    fn ints(&self) -> Option<Vec<Vec<Option<i32>>>> {
        None
    }
}

/// Two variants of one member: separate partition leaves, so separate scopes.
#[derive(greem::Abstract)]
#[greem(type = "SearchResult")]
enum Twice {
    #[greem(as = "User")]
    First(User),
    #[greem(as = "User")]
    Second(User),
}

/// A payload that fails as a whole fails its own position.
#[derive(greem::Abstract)]
#[greem(type = "Node")]
enum Fallible {
    User(Result<User, Error>),
    Post(Post),
}

struct Repeats;

#[greem::object(schema = crate::schema, type = "Query", context = App)]
impl Repeats {
    fn users(&self) -> Vec<User> {
        vec![]
    }
    fn user(&self, _args: &Args<schema::Query::user>) -> Option<User> {
        None
    }
    fn node(&self, args: &Args<schema::Query::node>) -> Option<Fallible> {
        Some(match args.id.as_str() {
            "10" => Fallible::Post(hello()),
            _ => Fallible::User(Err(Error::new("gone"))),
        })
    }
    fn search(&self) -> Vec<Twice> {
        vec![
            Twice::Second(bob()),
            Twice::First(ann()),
            Twice::Second(cid()),
        ]
    }
    fn ints(&self) -> Option<Vec<Vec<Option<i32>>>> {
        None
    }
}

/// Hits kept in an owned slice complete by reference.
struct Shelf {
    hits: Arc<[Hit]>,
}

#[greem::object(schema = crate::schema, type = "Query", context = App)]
impl Shelf {
    fn users(&self) -> Vec<User> {
        vec![]
    }
    fn user(&self, _args: &Args<schema::Query::user>) -> Option<User> {
        None
    }
    fn node(
        &self,
        _args: &Args<schema::Query::node>,
    ) -> Option<greem::As<schema::types::User, User>> {
        None
    }
    fn search(&self) -> Arc<[Hit]> {
        self.hits.clone()
    }
    fn ints(&self) -> Option<Vec<Vec<Option<i32>>>> {
        None
    }
}

struct Mutation;

#[greem::object(schema = crate::schema, context = App)]
impl Mutation {
    fn rename(&self, _args: &Args<schema::Mutation::rename>) -> Option<User> {
        None
    }
    fn fail(&self) -> String {
        String::new()
    }
}

fn run<Q: Outputs<schema::types::Query, App> + Send + Sync>(
    query: Q,
    document: &str,
) -> (Value, Vec<String>) {
    let schema = schema::Schema::<App>::builder()
        .query::<Q>()
        .mutation::<Mutation>()
        .build()
        .unwrap();
    let document = schema.parse(document).unwrap();
    let app = App::default();
    let output = block_on(schema.execute(
        Roots {
            query,
            mutation: Mutation,
        },
        app.clone(),
        Operation::new(document),
        ExecuteOptions::default(),
    ));
    let value: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
    let log = app.0.lock().unwrap().clone();
    (value, log)
}

#[test]
fn each_variant_completes_as_its_member() {
    let (v, _) = run(
        Query,
        "{ search { __typename ... on User { name } ... on Post { title author { name } } } }",
    );
    assert_eq!(
        v,
        json!({"data": {"search": [
            {"__typename": "User", "name": "Ann"},
            {"__typename": "Post", "title": "Hello", "author": {"name": "Ann"}},
            {"__typename": "User", "name": "Bob"}
        ]}})
    );
}

#[test]
fn one_enum_outputs_every_abstract_type_it_targets() {
    let directory = Directory {
        people: vec![ann(), bob()],
    };
    let (v, _) = run(
        directory,
        r#"{
            a: node(id: "2") { __typename id ... on User { name } }
            b: node(id: "10") { __typename id ... on Post { title } }
            c: node(id: "x") { id }
            search { __typename ... on User { name } ... on Post { title } }
        }"#,
    );
    assert_eq!(
        v,
        json!({"data": {
            "a": {"__typename": "User", "id": "2", "name": "Bob"},
            "b": {"__typename": "Post", "id": "10", "title": "Hello"},
            "c": null,
            "search": [
                {"__typename": "User", "name": "Ann"},
                {"__typename": "User", "name": "Bob"},
                {"__typename": "Post", "title": "Hello"}
            ]
        }})
    );
}

#[test]
fn repeated_members_are_separate_scopes_in_declaration_order() {
    let (v, log) = run(Repeats, "{ search { ... on User { name email } } }");
    assert_eq!(log, ["User.name Ann", "User.name Bob,Cid"]);
    let paths: Vec<&Value> = v["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["path"])
        .collect();
    assert_eq!(
        paths,
        [
            &json!(["search", 1, "email"]),
            &json!(["search", 0, "email"]),
            &json!(["search", 2, "email"])
        ]
    );
}

#[test]
fn hints_written_under_a_variant_reach_the_accepting_field() {
    let (_, log) = run(Query, "{ search { ... on Post { author { name } } } }");
    assert!(
        log.contains(&"Query.search author=true".to_owned()),
        "{log:?}"
    );
    let (_, log) = run(Query, "{ search { ... on User { name } } }");
    assert!(
        log.contains(&"Query.search author=false".to_owned()),
        "{log:?}"
    );
}

#[test]
fn owned_slices_complete_their_variants_by_reference() {
    let shelf = Shelf {
        hits: Arc::from([Hit::Post(hello()), Hit::User(bob())]),
    };
    let (v, _) = run(
        shelf,
        "{ search { __typename ... on User { name } ... on Post { author { name } } } }",
    );
    assert_eq!(
        v,
        json!({"data": {"search": [
            {"__typename": "Post", "author": {"name": "Ann"}},
            {"__typename": "User", "name": "Bob"}
        ]}})
    );
}

#[test]
fn a_failed_payload_fails_its_own_position() {
    let (v, _) = run(
        Repeats,
        r#"{ a: node(id: "1") { id } b: node(id: "10") { id } }"#,
    );
    assert_eq!(
        v,
        json!({
            "errors": [{"message": "gone", "locations": [{"line": 1, "column": 3}], "path": ["a"]}],
            "data": {"a": null, "b": {"id": "10"}}
        })
    );
}
