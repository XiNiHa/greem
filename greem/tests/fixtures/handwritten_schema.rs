// The hand-written copy of what `greem-build` emits for the test schema:
// the runtime, the proc macro and the reference executor all test against it.
// Included with `include!` so a contract change is edited once.
#[allow(non_snake_case, non_camel_case_types, dead_code, unused_imports)]
mod schema {
    use greem::__private as rt;
    use greem::{Field, List, Nullable, Outputs, Resolver, Shape};

    pub const SDL: &str = r#"
        directive @defer(if: Boolean! = true, label: String) on FRAGMENT_SPREAD | INLINE_FRAGMENT
        directive @stream(if: Boolean! = true, label: String, initialCount: Int = 0) on FIELD
        type Query { users: [User!]! user(id: ID!): User node(id: ID!): Node search: [SearchResult!]! ints: [[Int]!] }
        interface Node { id: ID! }
        type User implements Node { id: ID! name: String! email: String posts(first: Int = 10): [Post!]! }
        type Post implements Node { id: ID! title: String! author: User! }
        union SearchResult = User | Post
        type Mutation { rename(id: ID!, name: String!): User fail: String! }
    "#;

    pub mod types {
        pub struct Query;
        pub struct Mutation;
        pub struct Node;
        pub struct User;
        pub struct Post;
        pub struct SearchResult;
    }

    pub mod Query {
        use super::*;
        pub struct users;
        impl Field for users {
            type Args = ();
            type Type = List<types::User>;
            const NAME: &'static str = "users";
            const SHAPE: Shape = Shape::new(&[false], false);
        }
        pub struct user;
        impl Field for user {
            type Args = rt_args::Query_user;
            type Type = Nullable<types::User>;
            const NAME: &'static str = "user";
            const SHAPE: Shape = Shape::new(&[], true);
        }
        pub struct node;
        impl Field for node {
            type Args = rt_args::Query_node;
            type Type = Nullable<types::Node>;
            const NAME: &'static str = "node";
            const SHAPE: Shape = Shape::new(&[], true);
        }
        pub struct search;
        impl Field for search {
            type Args = ();
            type Type = List<types::SearchResult>;
            const NAME: &'static str = "search";
            const SHAPE: Shape = Shape::new(&[false], false);
        }
        pub struct ints;
        impl Field for ints {
            type Args = ();
            type Type = Nullable<List<List<Nullable<greem::scalars::Int>>>>;
            const NAME: &'static str = "ints";
            const SHAPE: Shape = Shape::new(&[true, false], true);
        }
    }
    pub mod Mutation {
        use super::*;
        pub struct rename;
        impl Field for rename {
            type Args = rt_args::Mutation_rename;
            type Type = Nullable<types::User>;
            const NAME: &'static str = "rename";
            const SHAPE: Shape = Shape::new(&[], true);
        }
        pub struct fail;
        impl Field for fail {
            type Args = ();
            type Type = greem::scalars::String;
            const NAME: &'static str = "fail";
            const SHAPE: Shape = Shape::new(&[], false);
        }
    }
    pub mod User {
        use super::*;
        pub struct id;
        impl Field for id {
            type Args = ();
            type Type = greem::scalars::ID;
            const NAME: &'static str = "id";
            const SHAPE: Shape = Shape::new(&[], false);
        }
        pub struct name;
        impl Field for name {
            type Args = ();
            type Type = greem::scalars::String;
            const NAME: &'static str = "name";
            const SHAPE: Shape = Shape::new(&[], false);
        }
        pub struct email;
        impl Field for email {
            type Args = ();
            type Type = Nullable<greem::scalars::String>;
            const NAME: &'static str = "email";
            const SHAPE: Shape = Shape::new(&[], true);
        }
        pub struct posts;
        impl Field for posts {
            type Args = rt_args::User_posts;
            type Type = List<types::Post>;
            const NAME: &'static str = "posts";
            const SHAPE: Shape = Shape::new(&[false], false);
        }
    }
    pub mod Post {
        use super::*;
        pub struct id;
        impl Field for id {
            type Args = ();
            type Type = greem::scalars::ID;
            const NAME: &'static str = "id";
            const SHAPE: Shape = Shape::new(&[], false);
        }
        pub struct title;
        impl Field for title {
            type Args = ();
            type Type = greem::scalars::String;
            const NAME: &'static str = "title";
            const SHAPE: Shape = Shape::new(&[], false);
        }
        pub struct author;
        impl Field for author {
            type Args = ();
            type Type = types::User;
            const NAME: &'static str = "author";
            const SHAPE: Shape = Shape::new(&[], false);
        }
    }

    pub mod rt_args {
        use greem::{FromInput, InputError, InputValue};
        pub struct Query_user {
            pub id: String,
        }
        impl FromInput for Query_user {
            fn from_input(v: &InputValue) -> Result<Self, InputError> {
                Ok(Self {
                    id: greem::__private::read_field(v, "id")?,
                })
            }
        }
        pub struct Query_node {
            pub id: String,
        }
        impl FromInput for Query_node {
            fn from_input(v: &InputValue) -> Result<Self, InputError> {
                Ok(Self {
                    id: greem::__private::read_field(v, "id")?,
                })
            }
        }
        pub struct User_posts {
            pub first: Option<i32>,
        }
        impl FromInput for User_posts {
            fn from_input(v: &InputValue) -> Result<Self, InputError> {
                Ok(Self {
                    first: greem::__private::read_field(v, "first")?,
                })
            }
        }
        pub struct Mutation_rename {
            pub id: String,
            pub name: String,
        }
        impl FromInput for Mutation_rename {
            fn from_input(v: &InputValue) -> Result<Self, InputError> {
                Ok(Self {
                    id: greem::__private::read_field(v, "id")?,
                    name: greem::__private::read_field(v, "name")?,
                })
            }
        }
    }

    macro_rules! object_tag {
        ($tag:ident, $plan:ident, $name:literal, $module:ident, [$($field:ident),*], $witness:ident) => {
            impl rt::InnerKind for types::$tag { const OBJECTS: bool = true; }
            impl<T, C> rt::Completes<T, C> for types::$tag
            where
                T: $(Resolver<$module::$field, C> +)* Send + Sync,
                C: Send + Sync + 'static,
            {
                const TYPENAME: &'static str = $name;

                fn walk<'a>(w: &mut rt::Walker<'_, C>, node: rt::NodeId, leaf: &rt::Leaf) -> Result<(), rt::Abort>
                where T: 'a, C: 'a,
                {
                    let entry = w.enter(node, leaf, $name)?;
                    let n = w.field_count(entry);
                    let mut fields = Vec::with_capacity(n);
                    for i in 0..n {
                        match w.field_name(entry, i) {
                            $(stringify!($field) => {
                                let args = w.coerce::<$module::$field>(entry, i);
                                if args.is_ok() { w.hints::<T, $module::$field>(entry, i); }
                                fields.push(plans::$tag::$field(args));
                            })*
                            _ => fields.push(plans::$tag::__other),
                        }
                    }
                    for i in 0..n {
                        match &fields[i] {
                            $(plans::$tag::$field(Ok(args)) => {
                                w.descend(entry, i, |w, child, leaf| {
                                    <<T as Resolver<$module::$field, C>>::Output<'a> as Outputs<<$module::$field as Field>::Type, C>>::__walk(w, child, leaf)
                                })?;
                                w.plan::<T, $module::$field>(entry, i, args);
                            })*
                            _ => {}
                        }
                    }
                    w.set_typed(entry, Box::new(plans::$plan(fields)));
                    Ok(())
                }

                fn parent_error(value: &T) -> ::core::option::Option<&::greem::Error> {
                    <T as Resolver<$module::$witness, C>>::parent_error(value)
                }

                fn complete<'a>(values: Vec<T>, positions: Vec<rt::Pos>, cc: &mut rt::Completion<'a, '_, C>)
                where T: 'a, C: 'a,
                {
                    let mut ok = Vec::with_capacity(values.len());
                    let mut ok_pos = Vec::with_capacity(values.len());
                    for (value, pos) in values.into_iter().zip(positions) {
                        match <T as Resolver<$module::$witness, C>>::parent_error(&value) {
                            Some(error) => cc.error(&pos, error.clone()),
                            None => { ok.push(value); ok_pos.push(pos); }
                        }
                    }
                    cc.object_scope::<T, types::$tag>(ok, ok_pos);
                }

                fn start_fields<'a>(cx: &rt::FieldsCx<'a, C>, parents: &[&'a T], set: usize) -> Vec<rt::FieldFuture<'a>>
                where T: 'a, C: 'a,
                {
                    let plan: &'a plans::$plan = cx.table.typed(cx.entry);
                    cx.header.sets[set].1.iter().map(|&i| match &plan.0[i as usize] {
                        $(plans::$tag::$field(args) => rt::field::<T, $module::$field, C>(cx, i, parents, args),)*
                        plans::$tag::__other => match cx.header.fields[i as usize].kind {
                            rt::FieldKind::Typename => rt::typename_field(cx, i),
                            _ => rt::introspection_field(cx, i),
                        },
                    }).collect()
                }

                fn reference<'v, 's: 'v>(value: T, rc: &rt::RefCompletion<'s, C>) -> rt::futures::future::BoxFuture<'v, rt::RefValue>
                where T: 'v, C: 'v,
                {
                    if let Some(error) = <T as Resolver<$module::$witness, C>>::parent_error(&value) {
                        return rc.error(error.clone());
                    }
                    let rc = rc.clone();
                    Box::pin(async move {
                        let value = value;
                        rt::reference_object(&value, &rc, $name, |value, entry, i, rc| {
                            let plan: &plans::$plan = rc.typed(entry);
                            match &plan.0[i as usize] {
                                $(plans::$tag::$field(args) => rt::reference_field::<T, $module::$field, C>(value, entry, i, args, rc),)*
                                plans::$tag::__other => Box::pin(async { Ok(rt::serde_json::Value::Null) }),
                            }
                        }).await
                    })
                }
            }
        };
    }

    pub mod plans {
        use super::*;
        pub enum Query {
            users(rt::FieldArgs<super::Query::users>),
            user(rt::FieldArgs<super::Query::user>),
            node(rt::FieldArgs<super::Query::node>),
            search(rt::FieldArgs<super::Query::search>),
            ints(rt::FieldArgs<super::Query::ints>),
            __other,
        }
        pub struct QueryPlan(pub Vec<Query>);
        pub enum Mutation {
            rename(rt::FieldArgs<super::Mutation::rename>),
            fail(rt::FieldArgs<super::Mutation::fail>),
            __other,
        }
        pub struct MutationPlan(pub Vec<Mutation>);
        pub enum User {
            id(rt::FieldArgs<super::User::id>),
            name(rt::FieldArgs<super::User::name>),
            email(rt::FieldArgs<super::User::email>),
            posts(rt::FieldArgs<super::User::posts>),
            __other,
        }
        pub struct UserPlan(pub Vec<User>);
        pub enum Post {
            id(rt::FieldArgs<super::Post::id>),
            title(rt::FieldArgs<super::Post::title>),
            author(rt::FieldArgs<super::Post::author>),
            __other,
        }
        pub struct PostPlan(pub Vec<Post>);
    }

    object_tag!(
        Query,
        QueryPlan,
        "Query",
        Query,
        [users, user, node, search, ints],
        users
    );
    object_tag!(
        Mutation,
        MutationPlan,
        "Mutation",
        Mutation,
        [rename, fail],
        rename
    );
    object_tag!(User, UserPlan, "User", User, [id, name, email, posts], id);
    object_tag!(Post, PostPlan, "Post", Post, [id, title, author], id);

    macro_rules! abstract_tag {
        ($tag:ident, $name:literal, [$(($step:literal, $member:ident)),*]) => {
            impl rt::InnerKind for types::$tag { const OBJECTS: bool = true; }
            $(
            impl<T, C> rt::Completes<greem::As<types::$member, T>, C> for types::$tag
            where T: Outputs<types::$member, C> + Send, C: Send + Sync + 'static,
            {
                fn walk<'a>(w: &mut rt::Walker<'_, C>, node: rt::NodeId, leaf: &rt::Leaf) -> Result<(), rt::Abort> where greem::As<types::$member, T>: 'a, C: 'a {
                    rt::walk_as::<types::$member, T, C>($step, w, node, leaf)
                }
                fn first_error(value: &greem::As<types::$member, T>, indices: &mut Vec<u32>, wanted: &dyn Fn(&[u32]) -> bool) -> Option<greem::Error> {
                    T::__first_error(&value.0, indices, wanted)
                }

                fn complete<'a>(values: Vec<greem::As<types::$member, T>>, positions: Vec<rt::Pos>, cc: &mut rt::Completion<'a, '_, C>) where greem::As<types::$member, T>: 'a, C: 'a {
                    rt::complete_as::<types::$member, T, C>($step, values, positions, cc)
                }
                fn reference<'v, 's: 'v>(value: greem::As<types::$member, T>, rc: &rt::RefCompletion<'s, C>) -> rt::futures::future::BoxFuture<'v, rt::RefValue> where greem::As<types::$member, T>: 'v, C: 'v {
                    T::__reference(value.into_inner(), &rc.with_leaf($step))
                }
            }
            )*
            impl<A, B, C> rt::Completes<greem::Either<A, B>, C> for types::$tag
            where types::$tag: rt::Completes<A, C> + rt::Completes<B, C>, A: Send, B: Send, C: Send + Sync + 'static,
            {
                fn walk<'a>(w: &mut rt::Walker<'_, C>, node: rt::NodeId, leaf: &rt::Leaf) -> Result<(), rt::Abort> where greem::Either<A, B>: 'a, C: 'a {
                    rt::walk_either::<types::$tag, A, B, C>(w, node, leaf)
                }
                fn first_error(value: &greem::Either<A, B>, indices: &mut Vec<u32>, wanted: &dyn Fn(&[u32]) -> bool) -> Option<greem::Error> {
                    match value {
                        greem::Either::A(a) => <types::$tag as rt::Completes<A, C>>::first_error(a, indices, wanted),
                        greem::Either::B(b) => <types::$tag as rt::Completes<B, C>>::first_error(b, indices, wanted),
                    }
                }

                fn complete<'a>(values: Vec<greem::Either<A, B>>, positions: Vec<rt::Pos>, cc: &mut rt::Completion<'a, '_, C>) where greem::Either<A, B>: 'a, C: 'a {
                    rt::complete_either::<types::$tag, A, B, C>(values, positions, cc)
                }
                fn reference<'v, 's: 'v>(value: greem::Either<A, B>, rc: &rt::RefCompletion<'s, C>) -> rt::futures::future::BoxFuture<'v, rt::RefValue> where greem::Either<A, B>: 'v, C: 'v {
                    match value {
                        greem::Either::A(a) => <types::$tag as rt::Completes<A, C>>::reference(a, &rc.with_leaf(0)),
                        greem::Either::B(b) => <types::$tag as rt::Completes<B, C>>::reference(b, &rc.with_leaf(1)),
                    }
                }
            }
            impl<X, C> rt::Completes<Result<X, greem::Error>, C> for types::$tag
            where X: Outputs<types::$tag, C> + Send, C: Send + Sync + 'static,
            {
                fn walk<'a>(w: &mut rt::Walker<'_, C>, node: rt::NodeId, leaf: &rt::Leaf) -> Result<(), rt::Abort> where Result<X, greem::Error>: 'a, C: 'a {
                    X::__walk(w, node, leaf)
                }
                fn first_error(value: &Result<X, greem::Error>, indices: &mut Vec<u32>, wanted: &dyn Fn(&[u32]) -> bool) -> Option<greem::Error> {
                    match value {
                        Ok(v) => X::__first_error(v, indices, wanted),
                        Err(e) => wanted(indices).then(|| e.clone()),
                    }
                }

                fn complete<'a>(values: Vec<Result<X, greem::Error>>, positions: Vec<rt::Pos>, cc: &mut rt::Completion<'a, '_, C>) where Result<X, greem::Error>: 'a, C: 'a {
                    let mut ok = Vec::new(); let mut ok_pos = Vec::new();
                    for (value, pos) in values.into_iter().zip(positions) { match value { Ok(v) => { ok.push(v); ok_pos.push(pos); } Err(e) => cc.error(&pos, e) } }
                    X::__complete(ok, ok_pos, cc)
                }
                fn reference<'v, 's: 'v>(value: Result<X, greem::Error>, rc: &rt::RefCompletion<'s, C>) -> rt::futures::future::BoxFuture<'v, rt::RefValue> where Result<X, greem::Error>: 'v, C: 'v {
                    match value { Ok(v) => X::__reference(v, rc), Err(e) => rc.error(e) }
                }
            }
        };
    }
    abstract_tag!(Node, "Node", [(0, User), (1, Post)]);
    abstract_tag!(SearchResult, "SearchResult", [(0, User), (1, Post)]);

    pub struct Info;
    impl rt::SchemaInfo for Info {
        const SDL: &'static str = SDL;
        const BUILD_VERSION: &'static str = greem::BUILD_VERSION;
        type Query = types::Query;
        type Mutation = types::Mutation;
    }
    pub type Schema<C = (), Q = (), M = greem::NoMutation> = greem::Schema<Info, C, Q, M>;
}
