# Design Document

## Core Concepts

- Schema-first. Users will run codegen in `build.rs` to get Rust type definitions from SDL schema.

## API Design

### `schema/schema.graphql`

```graphql
type Query {
    node(id: ID!): Node
}

type User implements Node {
    id: ID!
    name: String!
    friends(first: Int, last: Int, after: String, before: String): UserFriendsResolveResult
}

union UserFriendsResolveResult = FriendsConnection | Error

type Error {
    code: String!
    message: String
}
```

### `build.rs`

```rs
fn main() -> Result<(), anyhow::Error> {
    greem_build::codegen(
        greem_build::config::ConfigBuilder::default()
            .schema(vec!["./schema/*.graphql"])
            .output_directory("./src/schema")
            .build()?,
    )?;

    Ok(())
}
```

Running this will generate the `schema` module.

### `src/models/query.rs`

```rs
pub struct QueryModel;

#[async_trait]
impl crate::schema::resolvers::Query for QueryModel {
    async fn node(
        &self,
        id: greem::scalars::ID,
        _: greem::Context,
    ) -> greem::Result<crate::schema::types::Node> {
        Ok(crate::schema::types::Node::User(Box::new(UserModel::new(
            id,
        ))))
    }
}
```

### `src/models/user.rs`

```rs
pub struct UserModel {
    id: greem::scalars::ID,
}

impl UserModel {
    pub fn new(id: greem::scalars::ID) -> Self {
        Self { id }
    }
}

#[async_trait]
impl schema::resolvers::Node for UserModel {
    async fn id(&self, _: greem::Context) -> greem::Result<greem::scalars::ID> {
      Ok(self.id.clone())
    }
}

#[async_trait]
impl schema::resolvers::User for UserModel {
  async fn name(&self, ctx: greem::Context) -> greem::Result<String> {
    let user = ctx.get::<UserLoader>().load(self.id).await?;
    Ok(user.name.clone())
  }

  async fn friends(
    &self,
    first: Option<i32>,
    last: Option<i32>,
    after: Option<String>,
    before: Option<String>,
    ctx: greem::Context,
  ) -> greem::Result<crate::schema::types::UserFriendsResolveResult> {
    let result = ctx.get::<UserFriendsLoader>()
      .load(self.id, first, last, after, before)
      .await;

    Ok(match result {
      Ok(connection) => crate::schema::types::UserFriendsResolveResult::FriendsConnection(
        Box::new(connection)
      ),
      Err(err) => crate::schema::types::UserFriendsResolveResult::Error(Box::new(
        schema::types::records::Error {
          code: err.code(),
          message: err.message(),
        },
      )),
    })
  }
}
```

### `src/schema/mod.ts`

```rs
pub mod resolvers {
    #[::async_trait::async_trait]
    pub trait Query {
        async fn node(
            &self,
            id: ::greem::scalars::ID,
            ctx: ::greem::Context,
        ) -> ::greem::Result<super::types::Node>;
    }

    #[::async_trait::async_trait]
    pub trait Node {
        async fn id(&self, ctx: ::greem::Context) -> ::greem::Result<::greem::scalars::ID>;
    }

    #[::async_trait::async_trait]
    pub trait User {
        async fn name(&self, ctx: ::greem::Context) -> ::greem::Result<::std::string::String>;
        async fn friends(
            &self,
            first: ::std::option::Option<i32>,
            last: ::std::option::Option<i32>,
            after: ::std::option::Option<::std::string::String>,
            before: ::std::option::Option<::std::string::String>,
            ctx: ::greem::Context,
        ) -> ::greem::Result<super::types::UserFriendsResolveResult>;
    }

    #[::async_trait::async_trait]
    pub trait Error {
        async fn code(&self, ctx: ::greem::Context) -> ::greem::Result<::std::string::String>;
        async fn message(
            &self,
            ctx: ::greem::Context,
        ) -> ::greem::Result<::std::option::Option<::std::string::String>>;
    }

    #[::async_trait::async_trait]
    pub trait FriendsConnection {/* ... */}
}

pub mod types {
    pub enum Node {
        User(Box<dyn super::resolvers::User>),
    }

    pub enum UserFriendsResolveResult {
        FriendsConnection(Box<dyn super::resolvers::FriendsConnection>),
        Error(Box<dyn super::resolvers::Error>),
    }

    pub mod records {
        pub struct Error {
            pub code: String,
            pub message: String,
        }

        #[::async_trait::async_trait]
        impl super::super::resolvers::Error for Error {
            async fn code(&self, ctx: ::greem::Context) -> ::greem::Result<::std::string::String> {
                Ok(self.code.clone())
            }
            async fn message(
                &self,
                ctx: ::greem::Context,
            ) -> ::greem::Result<::std::option::Option<::std::string::String>> {
                Ok(Some(self.message.clone()))
            }
        }
    }
}

```
