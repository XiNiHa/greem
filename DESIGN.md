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
            .output_directory("./__generated__")
            .build()?,
    )?;

    Ok(())
}
```

Running this will generate the `schema` module.

### `src/models/node.rs`

```rs
pub struct NodeModel;

impl schema::resolvers::Node for NodeModel {
  fn node(&self, id: greem::scalars::ID, _: Context) -> greem::Result<schema::types::Node> {
    Ok(schema::types::Node::User(Box::new(UserModel::new(id))))
  }
}
```

### `src/models/user.rs`

```rs
pub struct UserModel {
  id: greem::scalars::ID
}

impl UserModel {
  fn new(id: greem::scalars::ID) -> Self {
    Self { id }
  }
}

impl schema::resolvers::User for UserModel {
  fn id(&self, _: Context) -> greem::Result<greem::scalars::ID> {
    Ok(self.id)
  }

  async fn name(&self, ctx: Context) -> greem::Result<greem::scalars::String> {
    let user = ctx.dataloaders.load::<User>(self.id).await?;
    Ok(user.name.clone())
  }

  async fn friends(
    &self,
    first: Option<i32>,
    last: Option<i32>,
    after: Option<String>,
    before: Option<String>,
    ctx: Context,
  ) -> greem::Result<schema::types::UserFriendsResolveResult> {
    let result = ctx.some_magical_loader
      .load(self.id, first, last, after, before)
      .await;

    Ok(match result {
      Ok(connection) => schema::types::UserFriendsResolveResult::FriendsConnection(connection)
      Err(err) => schema::types::UserFriendsResolveResult::Error(
        schema::types::records::Error {
          code: err.code(),
          message: err.message(),
        }
      )
    })
  }
}
```
