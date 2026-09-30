// The application over the hand-written schema module: types, resolvers and
// the schema constructor, shared by the runtime, macro-free and reference tests.

// ---- application ------------------------------------------------------------

#[derive(Default)]
struct App {
    log: Mutex<Vec<String>>,
    calls: Option<std::sync::Arc<Mutex<usize>>>,
    fail_email: bool,
    fail_name: bool,
    fail_author: bool,
}

impl App {
    fn log(&self, entry: impl Into<String>) {
        self.log.lock().unwrap().push(entry.into());
        if let Some(calls) = &self.calls {
            *calls.lock().unwrap() += 1;
        }
    }
}

struct QueryRoot;
struct MutationRoot;
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

impl Resolver<schema::Query::users, App> for QueryRoot {
    type Output<'obj>
        = Vec<User>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log(format!("Query.users x{}", parents.len()));
        Ok(parents.iter().map(|_| users()).collect())
    }
}
impl Resolver<schema::Query::user, App> for QueryRoot {
    type Output<'obj>
        = Option<User>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<schema::Query::user>,
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app()
            .log(format!("Query.user({}) x{}", args.id, parents.len()));
        Ok(parents
            .iter()
            .map(|_| users().into_iter().find(|u| u.id.to_string() == args.id))
            .collect())
    }
}
impl Resolver<schema::Query::node, App> for QueryRoot {
    type Output<'obj>
        = Option<Either<As<schema::types::User, User>, As<schema::types::Post, Post<'static>>>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<schema::Query::node>,
        _: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        static ANN: User = User {
            id: 1,
            name: String::new(),
            email: None,
        };
        Ok(parents
            .iter()
            .map(|_| match args.id.as_str() {
                "1" => Some(Either::A(As::new(users().remove(0)))),
                "p1" => Some(Either::B(As::new(Post {
                    id: 10,
                    title: "Hello".into(),
                    author: &ANN,
                }))),
                _ => None,
            })
            .collect())
    }
}
impl Resolver<schema::Query::search, App> for QueryRoot {
    type Output<'obj>
        = Vec<Either<As<schema::types::User, User>, As<schema::types::Post, Post<'static>>>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        _: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        static ANN: User = User {
            id: 1,
            name: String::new(),
            email: None,
        };
        Ok(parents
            .iter()
            .map(|_| {
                let mut u = users();
                vec![
                    Either::A(As::new(u.remove(0))),
                    Either::B(As::new(Post {
                        id: 10,
                        title: "Hello".into(),
                        author: &ANN,
                    })),
                    Either::A(As::new(u.remove(0))),
                ]
            })
            .collect())
    }
}
impl Resolver<schema::Query::ints, App> for QueryRoot {
    type Output<'obj>
        = Option<Vec<Vec<Option<i32>>>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        _: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        Ok(parents
            .iter()
            .map(|_| Some(vec![vec![Some(1), None], vec![]]))
            .collect())
    }
}

impl Resolver<schema::User::id, App> for User {
    type Output<'obj>
        = u32
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log(format!("User.id x{}", parents.len()));
        Ok(parents.iter().map(|u| u.id).collect())
    }
}
impl Resolver<schema::User::name, App> for User {
    type Output<'obj>
        = Result<&'obj str, Error>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log(format!("User.name x{}", parents.len()));
        Ok(parents
            .iter()
            .map(|u| {
                if ctx.app().fail_name && u.id == 2 {
                    Err(Error::new("name failed"))
                } else {
                    Ok(u.name.as_str())
                }
            })
            .collect())
    }
}
impl Resolver<schema::User::email, App> for User {
    type Output<'obj>
        = Result<Option<&'obj str>, Error>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log(format!("User.email x{}", parents.len()));
        Ok(parents
            .iter()
            .map(|u| {
                if ctx.app().fail_email && u.id == 1 {
                    Err(Error::new("email failed"))
                } else {
                    Ok(u.email.as_deref())
                }
            })
            .collect())
    }
}
impl Resolver<schema::User::posts, App> for User {
    type Output<'obj>
        = Vec<Post<'obj>>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<schema::User::posts>,
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log(format!(
            "User.posts(first: {:?}) x{}",
            args.first,
            parents.len()
        ));
        Ok(parents
            .iter()
            .map(|u| {
                (0..args.first.unwrap_or(10).min(2) as u32)
                    .map(|i| Post {
                        id: u.id * 10 + i,
                        title: format!("{} post {i}", u.name),
                        author: u,
                    })
                    .collect()
            })
            .collect())
    }
}
impl Resolver<schema::Post::id, App> for Post<'_> {
    type Output<'obj>
        = u32
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log(format!("Post.id x{}", parents.len()));
        Ok(parents.iter().map(|p| p.id).collect())
    }
}
impl Resolver<schema::Post::title, App> for Post<'_> {
    type Output<'obj>
        = &'obj String
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log(format!("Post.title x{}", parents.len()));
        Ok(parents.iter().map(|p| &p.title).collect())
    }
}
impl Resolver<schema::Post::author, App> for Post<'_> {
    type Output<'obj>
        = Result<&'obj User, Error>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log(format!("Post.author x{}", parents.len()));
        Ok(parents
            .iter()
            .map(|p| {
                if ctx.app().fail_author {
                    Err(Error::new("author failed"))
                } else {
                    Ok(p.author)
                }
            })
            .collect())
    }
}
impl Resolver<schema::Mutation::rename, App> for MutationRoot {
    type Output<'obj>
        = Option<User>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        args: &'obj Args<schema::Mutation::rename>,
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app()
            .log(format!("Mutation.rename({}, {})", args.id, args.name));
        Ok(parents
            .iter()
            .map(|_| {
                Some(User {
                    id: args.id.parse().unwrap_or(0),
                    name: args.name.clone(),
                    email: None,
                })
            })
            .collect())
    }
}
impl Resolver<schema::Mutation::fail, App> for MutationRoot {
    type Output<'obj>
        = Result<String, Error>
    where
        Self: 'obj;
    async fn resolve<'obj, 'call>(
        parents: &'call [&'obj Self],
        _: &'obj (),
        ctx: &'obj Context<'obj, App>,
    ) -> Result<Vec<Self::Output<'obj>>, Error>
    where
        'obj: 'call,
    {
        ctx.app().log("Mutation.fail");
        Ok(parents.iter().map(|_| Err(Error::new("boom"))).collect())
    }
}

type S = schema::Schema<App, QueryRoot, MutationRoot>;

fn schema() -> S {
    schema::Schema::<App>::builder()
        .query::<QueryRoot>()
        .mutation::<MutationRoot>()
        .build()
        .unwrap()
}
