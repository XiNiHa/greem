//! The area for graphql-js v17.0.2 `src/execution/__tests__/variables-test.ts`:
//! its schema, world and resolvers. Every field echoes its coerced `input`
//! as JSON (upstream's `inspect(args.input)`), so a case reads the argument
//! the resolver received; the world carries nothing but the harness.

use crate::harness::{Area, Harness, HasHarness};
use greem::{Args, Context, Enum, InputError, InputValue, Roots, Scalar, SchemaBuilder, Value};
use serde::Serialize;
use serde_json::json;

pub mod schema {
    greem::include_schema!("graphql_js_variables.rs");
}

#[derive(Clone, Debug, Default)]
pub struct World {
    pub harness: Harness,
}

impl HasHarness for World {
    fn harness(&self) -> &Harness {
        &self.harness
    }
}

impl Area for World {
    type Info = schema::__private::Info;
    type Query = QueryRoot;
    type Mutation = greem::NoMutation;

    fn builder() -> SchemaBuilder<Self::Info, Self, Self::Query, Self::Mutation> {
        schema::Schema::<World>::builder().query::<QueryRoot>()
    }

    fn roots() -> Roots<QueryRoot, greem::NoMutation> {
        Roots::query(QueryRoot)
    }
}

pub type Schema = schema::Schema<World, QueryRoot, greem::NoMutation>;

/// Upstream's `TestFaultyScalar`: every input is rejected.
impl Scalar for schema::types::FaultyScalar {
    type Value = String;
    fn to_value(value: &Self::Value) -> Value<'_> {
        Value::Str(value.as_str().into())
    }
    fn from_input(_: &InputValue) -> Result<Self::Value, InputError> {
        Err(InputError::new("FaultyScalarErrorMessage"))
    }
}

/// Upstream's `TestComplexScalar`: `"ExternalValue"` reads as `"InternalValue"`.
impl Scalar for schema::types::ComplexScalar {
    type Value = String;
    fn to_value(value: &Self::Value) -> Value<'_> {
        Value::Str(value.as_str().into())
    }
    fn from_input(value: &InputValue) -> Result<Self::Value, InputError> {
        match value {
            InputValue::String(s) if s == "ExternalValue" => Ok("InternalValue".to_owned()),
            other => Err(InputError::new(format!(
                "expected \"ExternalValue\", found {}",
                other.kind()
            ))),
        }
    }
}

/// Upstream's `TestJSONScalar`: the input, literal or variable, kept as is.
impl Scalar for schema::types::JSONScalar {
    type Value = serde_json::Value;
    fn to_value(value: &Self::Value) -> Value<'_> {
        Value::from_json(value)
    }
    fn from_input(value: &InputValue) -> Result<Self::Value, InputError> {
        Ok(value.to_json())
    }
}

/// The coerced input as a JSON string. An absent nullable argument reads as
/// `None`, like an explicit null: greem's arguments are not absent-aware.
fn echo(value: impl Serialize) -> Option<String> {
    Some(serde_json::to_string(&value).unwrap())
}

/// Every field of the input object, in schema order; an absent field is null.
fn test_input_object(o: &schema::TestInputObject) -> serde_json::Value {
    json!({"a": o.a, "b": o.b, "c": o.c, "d": o.d, "e": o.e})
}

pub struct QueryRoot;
pub struct Nested;

#[greem::object(schema = crate::graphql_js::variables::schema, type = "Query", context = World)]
impl QueryRoot {
    async fn field_with_enum_input(
        &self,
        args: &Args<schema::Query::fieldWithEnumInput>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("Query.fieldWithEnumInput", 1).await;
        echo(args.input.map(Enum::name))
    }
    async fn field_with_non_nullable_enum_input(
        &self,
        args: &Args<schema::Query::fieldWithNonNullableEnumInput>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app()
            .harness
            .trace("Query.fieldWithNonNullableEnumInput", 1)
            .await;
        echo(args.input.name())
    }
    async fn field_with_object_input(
        &self,
        args: &Args<schema::Query::fieldWithObjectInput>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app()
            .harness
            .trace("Query.fieldWithObjectInput", 1)
            .await;
        echo(args.input.as_ref().map(test_input_object))
    }
    async fn field_with_nullable_string_input(
        &self,
        args: &Args<schema::Query::fieldWithNullableStringInput>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app()
            .harness
            .trace("Query.fieldWithNullableStringInput", 1)
            .await;
        echo(&args.input)
    }
    async fn field_with_non_nullable_string_input(
        &self,
        args: &Args<schema::Query::fieldWithNonNullableStringInput>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app()
            .harness
            .trace("Query.fieldWithNonNullableStringInput", 1)
            .await;
        echo(&args.input)
    }
    async fn field_with_default_argument_value(
        &self,
        args: &Args<schema::Query::fieldWithDefaultArgumentValue>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app()
            .harness
            .trace("Query.fieldWithDefaultArgumentValue", 1)
            .await;
        echo(&args.input)
    }
    async fn field_with_non_nullable_string_input_and_default_argument_value(
        &self,
        args: &Args<schema::Query::fieldWithNonNullableStringInputAndDefaultArgumentValue>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app()
            .harness
            .trace(
                "Query.fieldWithNonNullableStringInputAndDefaultArgumentValue",
                1,
            )
            .await;
        echo(&args.input)
    }
    async fn field_with_nested_input_object(
        &self,
        args: &Args<schema::Query::fieldWithNestedInputObject>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app()
            .harness
            .trace("Query.fieldWithNestedInputObject", 1)
            .await;
        echo(
            args.input
                .as_ref()
                .map(|o| json!({"na": test_input_object(&o.na), "nb": o.nb})),
        )
    }
    #[greem(name = "fieldWithJSONScalarInput")]
    async fn field_with_json_scalar_input(
        &self,
        args: &Args<schema::Query::fieldWithJSONScalarInput>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app()
            .harness
            .trace("Query.fieldWithJSONScalarInput", 1)
            .await;
        echo(&args.input)
    }
    async fn field_with_prototype_named_argument(
        &self,
        args: &Args<schema::Query::fieldWithPrototypeNamedArgument>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app()
            .harness
            .trace("Query.fieldWithPrototypeNamedArgument", 1)
            .await;
        match &args.toString {
            None => Some("missing".to_owned()),
            value => echo(value),
        }
    }
    async fn list(&self, args: &Args<schema::Query::list>, ctx: &Context<World>) -> Option<String> {
        ctx.app().harness.trace("Query.list", 1).await;
        echo(&args.input)
    }
    async fn nested(
        &self,
        _args: &Args<schema::Query::nested>,
        ctx: &Context<World>,
    ) -> Option<Nested> {
        ctx.app().harness.trace("Query.nested", 1).await;
        Some(Nested)
    }
    async fn nn_list(
        &self,
        args: &Args<schema::Query::nnList>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("Query.nnList", 1).await;
        echo(&args.input)
    }
    #[greem(name = "listNN")]
    async fn list_nn(
        &self,
        args: &Args<schema::Query::listNN>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("Query.listNN", 1).await;
        echo(&args.input)
    }
    #[greem(name = "nnListNN")]
    async fn nn_list_nn(
        &self,
        args: &Args<schema::Query::nnListNN>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("Query.nnListNN", 1).await;
        echo(&args.input)
    }
}

#[greem::object(schema = crate::graphql_js::variables::schema, type = "NestedType", context = World)]
impl Nested {
    async fn echo(
        &self,
        args: &Args<schema::NestedType::echo>,
        ctx: &Context<World>,
    ) -> Option<String> {
        ctx.app().harness.trace("NestedType.echo", 1).await;
        echo(&args.input)
    }
}
