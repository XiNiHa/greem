//! `#[greem::object]` against `greem-test-app`'s generated schema module.

use futures::StreamExt;
use futures::executor::block_on;
use greem::{
    Args, Context, Error, ExecuteOptions, HintRegistry, Operation, Planning, Roots, Streamed,
};
use greem_test_app::schema;
use serde_json::{Value, json};
use std::sync::Mutex;

#[derive(Default)]
struct App {
    log: Mutex<Vec<String>>,
}

static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

// A user type named like the marker trait, as a schema enum brought in with
// `use schema::*` would be: the macro must not pick it up.
#[allow(dead_code)]
enum Send {}

#[derive(Default)]
struct PostsHint {
    with_author: bool,
    from_posts: bool,
}

struct Query;
struct User {
    id: u32,
    name: String,
    email: Option<String>,
}
struct Post<'a> {
    id: u32,
    title: String,
    author: &'a User,
}

fn users() -> Vec<User> {
    vec![
        User {
            id: 1,
            name: "Ann".into(),
            email: Some("ann@x".into()),
        },
        User {
            id: 2,
            name: "Bob".into(),
            email: None,
        },
    ]
}

#[greem::object(schema = crate::schema, context = App)]
impl Query {
    #[greem(hints = "users")]
    fn users_hints(reg: &mut HintRegistry<'_>) {
        reg.accept::<PostsHint>();
    }
    fn users(
        &self,
        _args: &Args<schema::Query::users>,
        ctx: &Context<App>,
    ) -> Streamed<futures::stream::Iter<std::vec::IntoIter<Result<User, Error>>>> {
        LOG.lock().unwrap().push(format!(
            "users from_posts={}",
            ctx.hint::<PostsHint>().from_posts
        ));
        let items: Vec<Result<User, Error>> = users().into_iter().map(Ok).collect();
        Streamed::new(futures::stream::iter(items))
    }
    async fn user(&self, args: &Args<schema::Query::user>) -> Option<User> {
        users().into_iter().find(|u| u.id.to_string() == args.id)
    }
    fn node(
        &self,
        _args: &Args<schema::Query::node>,
    ) -> Option<greem::As<schema::types::User, User>> {
        None
    }
    fn search(&self) -> Vec<greem::As<schema::types::User, User>> {
        vec![]
    }
    fn ints(&self) -> Option<Vec<Vec<Option<i32>>>> {
        None
    }
}

#[greem::object(schema = crate::schema, type = "User", context = App)]
impl User {
    async fn id(&self) -> u32 {
        self.id
    }
    #[greem(name = "name")]
    async fn display_name(&self) -> &str {
        &self.name
    }
    async fn email(
        &self,
        _args: &Args<schema::User::email>,
        ctx: &Context<App>,
    ) -> Result<Option<String>, Error> {
        ctx.app().log.lock().unwrap().push("email".into());
        if self.id == 2 {
            Err(Error::new("no email"))
        } else {
            Ok(self.email.clone())
        }
    }
    #[greem(hints = "posts")]
    fn posts_hints(reg: &mut HintRegistry<'_>) {
        reg.accept::<PostsHint>();
    }
    #[greem(plan = "posts")]
    fn posts_plan(p: &mut Planning<'_, schema::User::posts, App>) {
        p.hint::<PostsHint>(|h| h.from_posts = true);
    }
    async fn posts<'a>(
        parents: &[&'a Self],
        args: &Args<schema::User::posts>,
        ctx: &Context<App>,
    ) -> Vec<Vec<Post<'a>>> {
        let _ = ctx.app();
        LOG.lock().unwrap().push(format!(
            "posts x{} with_author={} from_posts={}",
            parents.len(),
            ctx.hint::<PostsHint>().with_author,
            ctx.hint::<PostsHint>().from_posts
        ));
        parents
            .iter()
            .map(|u| {
                (0..args.first.unwrap_or(10).min(1) as u32)
                    .map(|i| Post {
                        id: u.id * 10 + i,
                        title: format!("{} post {i}", u.name),
                        author: u,
                    })
                    .collect()
            })
            .collect()
    }
}

#[greem::object(schema = crate::schema, type = "Post", context = App)]
impl Post<'_> {
    fn id(&self) -> u32 {
        self.id
    }
    fn title(&self) -> &String {
        &self.title
    }
    // Disabled hooks: their generated `hints`/`plan` must disappear with them.
    #[cfg(any())]
    #[greem(hints = "title")]
    fn title_hints(reg: &mut HintRegistry<'_>) {
        reg.accept::<PostsHint>();
    }
    #[cfg_attr(all(), cfg(any()))]
    #[greem(plan = "title")]
    fn title_plan(_p: &mut Planning<'_, schema::Post::title, App>) {}
    #[greem(plan = "author")]
    fn author_plan(p: &mut Planning<'_, schema::Post::author, App>) {
        p.hint::<PostsHint>(|h| h.with_author = true);
    }
    async fn author(&self) -> Result<&User, Error> {
        Ok(self.author)
    }
}

struct Mutation;
#[greem::object(schema = crate::schema, context = App)]
impl Mutation {
    fn rename(&self, args: &Args<schema::Mutation::rename>) -> Option<User> {
        Some(User {
            id: 0,
            name: args.name.clone(),
            email: None,
        })
    }
    fn fail(&self) -> Result<String, Error> {
        Err(Error::new("boom"))
    }
}

fn run(query: &str) -> (Value, Vec<String>) {
    let schema = schema::Schema::<App>::builder()
        .query::<Query>()
        .mutation::<Mutation>()
        .build()
        .unwrap();
    let document = schema.parse(query).unwrap();
    let app = App::default();
    let output = block_on(schema.execute(
        Roots {
            query: Query,
            mutation: Mutation,
        },
        app,
        Operation {
            document: document.clone(),
            operation_name: None,
            variables: Value::Null,
        },
        ExecuteOptions::default(),
    ));
    let value: Value = serde_json::from_slice(&output.payloads[0].json).unwrap();
    (value, Vec::new())
}

#[test]
fn object_sugar_end_to_end() {
    let (v, _) = run("{ users { id name email posts(first: 1) { id title author { name } } } }");
    assert_eq!(
        v["data"],
        json!({"users": [
            {"id": "1", "name": "Ann", "email": "ann@x", "posts": [{"id": "10", "title": "Ann post 0", "author": {"name": "Ann"}}]},
            {"id": "2", "name": "Bob", "email": null, "posts": [{"id": "20", "title": "Bob post 0", "author": {"name": "Bob"}}]}
        ]})
    );
    assert_eq!(v["errors"][0]["path"], json!(["users", 1, "email"]));
    assert_eq!(v["errors"][0]["message"], json!("no email"));
}

#[test]
fn set_based_and_hints() {
    let schema = schema::Schema::<App>::builder()
        .query::<Query>()
        .mutation::<Mutation>()
        .build()
        .unwrap();
    let document = schema
        .parse("{ users { posts { author { id } } } }")
        .unwrap();
    let mut seen = Vec::new();
    block_on(
        schema
            .execute_stream(
                Roots {
                    query: Query,
                    mutation: Mutation,
                },
                App::default(),
                Operation {
                    document: document.clone(),
                    operation_name: None,
                    variables: Value::Null,
                },
                ExecuteOptions::default(),
                |payload| seen.push(serde_json::to_value(&payload).unwrap()),
            )
            .for_each(|()| async {}),
    );
    assert_eq!(
        seen[0]["data"]["users"][1]["posts"][0]["author"]["id"],
        json!("2")
    );
    // The hint written under Post.author reached User.posts, and the
    // set-based resolver ran once for both users.
    let log = LOG.lock().unwrap().clone();
    assert!(
        log.contains(&"posts x2 with_author=true from_posts=false".to_string()),
        "{log:?}"
    );
    // User.posts accepts and writes the same hint: its write lands on the
    // nearest accepting field above it (Query.users), not on its own slot.
    assert!(
        log.contains(&"users from_posts=true".to_string()),
        "{log:?}"
    );
}

#[test]
fn mutation_and_name_override() {
    let (v, _) = run(r#"mutation { rename(id: "1", name: "Zed") { name } }"#);
    assert_eq!(v["data"], json!({"rename": {"name": "Zed"}}));
}
