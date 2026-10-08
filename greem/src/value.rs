use crate::error::{Error, InputError};
use serde::Serialize;
use std::borrow::Cow;

/// A resolved leaf value, borrowing string data from the objects it came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Value<'a> {
    Null,
    Bool(bool),
    Int(i64),
    /// An integer above `i64::MAX`, as JSON-like custom scalars can hold.
    UInt(u64),
    Float(f64),
    Str(Cow<'a, str>),
    List(Vec<Value<'a>>),
    Object(Vec<(Cow<'a, str>, Value<'a>)>),
}

impl Serialize for Value<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Int(i) => serializer.serialize_i64(*i),
            Value::UInt(u) => serializer.serialize_u64(*u),
            Value::Float(f) => serializer.serialize_f64(*f),
            Value::Str(s) => serializer.serialize_str(s),
            Value::List(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Value::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key.as_ref(), value)?;
                }
                map.end()
            }
        }
    }
}

impl Value<'_> {
    /// Copies borrowed strings so the value no longer borrows its source.
    pub fn into_owned(self) -> Value<'static> {
        match self {
            Value::Null => Value::Null,
            Value::Bool(b) => Value::Bool(b),
            Value::Int(i) => Value::Int(i),
            Value::UInt(u) => Value::UInt(u),
            Value::Float(f) => Value::Float(f),
            Value::Str(s) => Value::Str(Cow::Owned(s.into_owned())),
            Value::List(items) => Value::List(items.into_iter().map(Value::into_owned).collect()),
            Value::Object(fields) => Value::Object(
                fields
                    .into_iter()
                    .map(|(k, v)| (Cow::Owned(k.into_owned()), v.into_owned()))
                    .collect(),
            ),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    pub fn from_json(json: &serde_json::Value) -> Value<'static> {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => match (n.as_i64(), n.as_u64()) {
                (Some(i), _) => Value::Int(i),
                (None, Some(u)) => Value::UInt(u),
                (None, None) => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
            },
            serde_json::Value::String(s) => Value::Str(Cow::Owned(s.clone())),
            serde_json::Value::Array(items) => {
                Value::List(items.iter().map(Value::from_json).collect())
            }
            serde_json::Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| (Cow::Owned(k.clone()), Value::from_json(v)))
                    .collect(),
            ),
        }
    }
}

/// A coerced input value: an argument, a variable or a nested input object field.
///
/// `Enum` is distinct from `String` so a generated enum rejects a string
/// literal where the schema says enum.
#[derive(Clone, Debug, PartialEq)]
pub enum InputValue {
    Null,
    Bool(bool),
    Int(i64),
    /// An integer above `i64::MAX`, as JSON-like custom scalars can receive.
    UInt(u64),
    Float(f64),
    String(String),
    Enum(String),
    List(Vec<InputValue>),
    Object(Vec<(String, InputValue)>),
}

impl InputValue {
    pub fn field(&self, name: &str) -> Option<&InputValue> {
        match self {
            InputValue::Object(fields) => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            InputValue::Null => "null",
            InputValue::Bool(_) => "boolean",
            InputValue::Int(_) | InputValue::UInt(_) => "integer",
            InputValue::Float(_) => "float",
            InputValue::String(_) => "string",
            InputValue::Enum(_) => "enum value",
            InputValue::List(_) => "list",
            InputValue::Object(_) => "input object",
        }
    }
}

/// A generated GraphQL enum: its values, their names and the reverse lookup.
/// These live on a trait rather than as inherent items so that a value named
/// `VALUES`, `name` or `from_name` still compiles; call them through the trait.
pub trait Enum: Copy + 'static {
    const VALUES: &'static [Self];
    fn name(self) -> &'static str;
    fn from_name(name: &str) -> Option<Self>;
}

/// Conversion from a coerced [`InputValue`] into an argument or input type.
pub trait FromInput: Sized {
    fn from_input(value: &InputValue) -> Result<Self, InputError>;

    /// The value used when an input object field or argument is absent.
    fn from_absent() -> Result<Self, InputError> {
        Err(InputError::new("value is required"))
    }
}

/// Reads an input object field, distinguishing an absent field from `null`.
pub fn read_field<T: FromInput>(object: &InputValue, name: &str) -> Result<T, InputError> {
    match object.field(name) {
        Some(value) => T::from_input(value).map_err(|e| e.at(name)),
        None => T::from_absent().map_err(|e| e.at(name)),
    }
}

/// Reads an input object field through explicit readers (custom scalar positions).
pub fn read_field_with<T>(
    object: &InputValue,
    name: &str,
    present: impl FnOnce(&InputValue) -> Result<T, InputError>,
    absent: impl FnOnce() -> Result<T, InputError>,
) -> Result<T, InputError> {
    match object.field(name) {
        Some(value) => present(value).map_err(|e| e.at(name)),
        None => absent().map_err(|e| e.at(name)),
    }
}

impl InputValue {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            InputValue::Null => serde_json::Value::Null,
            InputValue::Bool(b) => serde_json::Value::Bool(*b),
            InputValue::Int(i) => serde_json::Value::from(*i),
            InputValue::UInt(u) => serde_json::Value::from(*u),
            InputValue::Float(f) => serde_json::Value::from(*f),
            InputValue::String(s) | InputValue::Enum(s) => serde_json::Value::String(s.clone()),
            InputValue::List(items) => {
                serde_json::Value::Array(items.iter().map(InputValue::to_json).collect())
            }
            InputValue::Object(fields) => serde_json::Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_json()))
                    .collect(),
            ),
        }
    }
}

/// A nullable input that remembers whether it was absent, `null` or present.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Maybe<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

impl<T: FromInput> FromInput for Maybe<T> {
    fn from_input(value: &InputValue) -> Result<Self, InputError> {
        match value {
            InputValue::Null => Ok(Maybe::Null),
            other => T::from_input(other).map(Maybe::Value),
        }
    }

    fn from_absent() -> Result<Self, InputError> {
        Ok(Maybe::Absent)
    }
}

impl<T: FromInput> FromInput for Option<T> {
    fn from_input(value: &InputValue) -> Result<Self, InputError> {
        match value {
            InputValue::Null => Ok(None),
            other => T::from_input(other).map(Some),
        }
    }

    fn from_absent() -> Result<Self, InputError> {
        Ok(None)
    }
}

impl<T: FromInput> FromInput for Vec<T> {
    fn from_input(value: &InputValue) -> Result<Self, InputError> {
        match value {
            InputValue::List(items) => items
                .iter()
                .enumerate()
                .map(|(i, item)| T::from_input(item).map_err(|e| e.at(&i.to_string())))
                .collect(),
            // A nullable list is read through `Option`; here null is not a
            // one-item list, whatever the item type accepts.
            InputValue::Null => Err(InputError::new("expected a list, found null")),
            other => T::from_input(other).map(|v| vec![v]),
        }
    }
}

impl<T: FromInput> FromInput for Box<T> {
    fn from_input(value: &InputValue) -> Result<Self, InputError> {
        T::from_input(value).map(Box::new)
    }

    fn from_absent() -> Result<Self, InputError> {
        T::from_absent().map(Box::new)
    }
}

impl FromInput for () {
    fn from_input(_: &InputValue) -> Result<Self, InputError> {
        Ok(())
    }

    fn from_absent() -> Result<Self, InputError> {
        Ok(())
    }
}

impl FromInput for bool {
    fn from_input(value: &InputValue) -> Result<Self, InputError> {
        match value {
            InputValue::Bool(b) => Ok(*b),
            other => Err(InputError::new(format!(
                "expected Boolean, found {}",
                other.kind()
            ))),
        }
    }
}

impl FromInput for i32 {
    fn from_input(value: &InputValue) -> Result<Self, InputError> {
        match value {
            InputValue::Int(i) => i32::try_from(*i)
                .map_err(|_| InputError::new(format!("Int value {i} is out of range"))),
            InputValue::UInt(u) => Err(InputError::new(format!("Int value {u} is out of range"))),
            other => Err(InputError::new(format!(
                "expected Int, found {}",
                other.kind()
            ))),
        }
    }
}

impl FromInput for f64 {
    fn from_input(value: &InputValue) -> Result<Self, InputError> {
        match value {
            InputValue::Float(f) => Ok(*f),
            InputValue::Int(i) => Ok(*i as f64),
            InputValue::UInt(u) => Ok(*u as f64),
            other => Err(InputError::new(format!(
                "expected Float, found {}",
                other.kind()
            ))),
        }
    }
}

impl FromInput for String {
    fn from_input(value: &InputValue) -> Result<Self, InputError> {
        match value {
            InputValue::String(s) => Ok(s.clone()),
            InputValue::Int(i) => Ok(i.to_string()),
            InputValue::UInt(u) => Ok(u.to_string()),
            other => Err(InputError::new(format!(
                "expected String, found {}",
                other.kind()
            ))),
        }
    }
}

impl FromInput for InputValue {
    fn from_input(value: &InputValue) -> Result<Self, InputError> {
        Ok(value.clone())
    }
}

/// The Rust representation of a custom scalar, implemented on its generated tag.
pub trait Scalar {
    /// The Rust representation. Generated argument and input-object structs
    /// derive `Clone` and `Debug`, so the representation provides both.
    type Value: Clone + std::fmt::Debug + Send + Sync;
    fn to_value(value: &Self::Value) -> Value<'_>;
    fn from_input(value: &InputValue) -> Result<Self::Value, InputError>;
}

/// Reads a custom scalar input through its tag's [`Scalar`] impl.
pub fn scalar_from_input<S: Scalar>(value: &InputValue) -> Result<S::Value, InputError> {
    match value {
        InputValue::Null => Err(InputError::new("expected a scalar value, found null")),
        other => S::from_input(other),
    }
}

/// A Rust value that produces the leaf [`Value`] for the scalar or enum tag `Tag`.
pub trait ToLeaf<Tag> {
    fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
    where
        Self: 'a;

    /// The error [`to_leaf`](Self::to_leaf) would return, without consuming the value.
    fn leaf_error(&self) -> Option<Error> {
        None
    }

    /// Whether [`to_leaf`](Self::to_leaf) would return null: possible for a
    /// custom scalar, and an error where the position is non-null.
    fn leaf_is_null(&self) -> bool {
        false
    }
}

impl<Tag, T: ToLeaf<Tag>> ToLeaf<Tag> for Result<T, Error> {
    fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
    where
        Self: 'a,
    {
        self.and_then(T::to_leaf)
    }

    fn leaf_error(&self) -> Option<Error> {
        match self {
            Ok(value) => value.leaf_error(),
            Err(error) => Some(error.clone()),
        }
    }

    fn leaf_is_null(&self) -> bool {
        self.as_ref().is_ok_and(T::leaf_is_null)
    }
}

impl<'r, Tag, T> ToLeaf<Tag> for &'r Result<T, Error>
where
    &'r T: ToLeaf<Tag>,
{
    fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
    where
        Self: 'a,
    {
        match self {
            Ok(value) => value.to_leaf(),
            Err(error) => Err(error.clone()),
        }
    }

    fn leaf_error(&self) -> Option<Error> {
        match self {
            Ok(value) => <&'r T as ToLeaf<Tag>>::leaf_error(&value),
            Err(error) => Some(error.clone()),
        }
    }

    fn leaf_is_null(&self) -> bool {
        self.as_ref()
            .is_ok_and(|value| <&'r T as ToLeaf<Tag>>::leaf_is_null(&value))
    }
}

/// A borrow of a borrowed leaf completes like the borrow it points to.
impl<'x, Tag, T: ?Sized> ToLeaf<Tag> for &&'x T
where
    &'x T: ToLeaf<Tag>,
{
    fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
    where
        Self: 'a,
    {
        (*self).to_leaf()
    }

    fn leaf_error(&self) -> Option<Error> {
        <&'x T as ToLeaf<Tag>>::leaf_error(*self)
    }

    fn leaf_is_null(&self) -> bool {
        <&'x T as ToLeaf<Tag>>::leaf_is_null(*self)
    }
}

/// Tags for the built-in scalars, used as `Field::Type` at leaf positions.
pub mod scalars {
    pub struct Int;
    pub struct Float;
    pub struct String;
    pub struct Boolean;
    pub struct ID;
}

/// A leaf completed to null where the schema says non-null: a custom scalar
/// whose value is JSON null, which the type system cannot rule out.
pub fn null_at_non_null() -> Error {
    Error::framework(
        "Cannot return null at a non-null position",
        "NULL_AT_NON_NULL",
    )
}

fn int_out_of_range(value: impl std::fmt::Display) -> Error {
    Error::framework(format!("Int cannot represent {value}"), "INT_OUT_OF_RANGE")
}

macro_rules! exact_int {
    ($($t:ty),*) => {$(
        impl ToLeaf<scalars::Int> for $t {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> where Self: 'a { Ok(Value::Int(self as i64)) }
        }
        impl<'x> ToLeaf<scalars::Int> for &'x $t {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> where Self: 'a { Ok(Value::Int(*self as i64)) }
        }
        impl ToLeaf<scalars::ID> for $t {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> where Self: 'a { Ok(Value::Str(Cow::Owned(self.to_string()))) }
        }
        impl<'x> ToLeaf<scalars::ID> for &'x $t {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> where Self: 'a { Ok(Value::Str(Cow::Owned(self.to_string()))) }
        }
    )*};
}
exact_int!(i8, i16, i32, u8, u16);

macro_rules! checked_int {
    ($($t:ty),*) => {$(
        impl ToLeaf<scalars::Int> for $t {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> where Self: 'a {
                i32::try_from(self).map(|v| Value::Int(v as i64)).map_err(|_| int_out_of_range(self))
            }
            fn leaf_error(&self) -> Option<Error> { <$t as ToLeaf<scalars::Int>>::to_leaf(*self).err() }
        }
        impl<'x> ToLeaf<scalars::Int> for &'x $t {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> where Self: 'a { <$t as ToLeaf<scalars::Int>>::to_leaf(*self) }
            fn leaf_error(&self) -> Option<Error> { <$t as ToLeaf<scalars::Int>>::to_leaf(**self).err() }
        }
        impl ToLeaf<scalars::ID> for $t {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> where Self: 'a { Ok(Value::Str(Cow::Owned(self.to_string()))) }
        }
        impl<'x> ToLeaf<scalars::ID> for &'x $t {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> where Self: 'a { Ok(Value::Str(Cow::Owned(self.to_string()))) }
        }
    )*};
}
checked_int!(i64, u32, u64, isize, usize);

/// JSON has no NaN or infinity: they are execution errors at the position.
fn finite_float<'a>(value: f64) -> Result<Value<'a>, Error> {
    if value.is_finite() {
        Ok(Value::Float(value))
    } else {
        Err(Error::framework(
            format!("Float cannot represent {value}"),
            "FLOAT_NOT_FINITE",
        ))
    }
}

macro_rules! float {
    ($($t:ty),*) => {$(
        impl ToLeaf<scalars::Float> for $t {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> where Self: 'a { finite_float(self as f64) }
            fn leaf_error(&self) -> Option<Error> { finite_float(*self as f64).err() }
        }
        impl<'x> ToLeaf<scalars::Float> for &'x $t {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error> where Self: 'a { finite_float(*self as f64) }
            fn leaf_error(&self) -> Option<Error> { finite_float(**self as f64).err() }
        }
    )*};
}
float!(f32, f64);

impl ToLeaf<scalars::Boolean> for bool {
    fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
    where
        Self: 'a,
    {
        Ok(Value::Bool(self))
    }
}
impl ToLeaf<scalars::Boolean> for &bool {
    fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
    where
        Self: 'a,
    {
        Ok(Value::Bool(*self))
    }
}

macro_rules! strings {
    ($tag:ty) => {
        impl ToLeaf<$tag> for String {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
            where
                Self: 'a,
            {
                Ok(Value::Str(Cow::Owned(self)))
            }
        }
        impl<'x> ToLeaf<$tag> for &'x String {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
            where
                Self: 'a,
            {
                Ok(Value::Str(Cow::Borrowed(self)))
            }
        }
        impl<'x> ToLeaf<$tag> for &'x str {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
            where
                Self: 'a,
            {
                Ok(Value::Str(Cow::Borrowed(self)))
            }
        }
        impl<'x> ToLeaf<$tag> for Cow<'x, str> {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
            where
                Self: 'a,
            {
                Ok(Value::Str(self))
            }
        }
        impl<'r, 'x> ToLeaf<$tag> for &'r Cow<'x, str> {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
            where
                Self: 'a,
            {
                Ok(Value::Str(Cow::Borrowed(self)))
            }
        }
        impl ToLeaf<$tag> for Box<str> {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
            where
                Self: 'a,
            {
                Ok(Value::Str(Cow::Owned(self.into())))
            }
        }
        impl<'x> ToLeaf<$tag> for &'x Box<str> {
            fn to_leaf<'a>(self) -> Result<Value<'a>, Error>
            where
                Self: 'a,
            {
                Ok(Value::Str(Cow::Borrowed(self)))
            }
        }
    };
}
strings!(scalars::String);
strings!(scalars::ID);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_outputs_are_range_checked() {
        assert_eq!(
            <i64 as ToLeaf<scalars::Int>>::to_leaf(7).unwrap(),
            Value::Int(7)
        );
        let error = <i64 as ToLeaf<scalars::Int>>::to_leaf(i64::MAX).unwrap_err();
        assert_eq!(error.extensions().unwrap()["code"], "INT_OUT_OF_RANGE");
        assert!(<u64 as ToLeaf<scalars::Int>>::to_leaf(u64::MAX).is_err());
        assert!(<usize as ToLeaf<scalars::Int>>::to_leaf(1 << 31).is_err());
        assert_eq!(
            <u16 as ToLeaf<scalars::Int>>::to_leaf(u16::MAX).unwrap(),
            Value::Int(65535)
        );
        assert_eq!(
            <u64 as ToLeaf<scalars::ID>>::to_leaf(u64::MAX).unwrap(),
            Value::Str("18446744073709551615".into())
        );
    }

    #[test]
    fn leaves_complete_through_a_borrow_of_any_lifetime() {
        fn through_any_borrow<T>(value: &T) -> Result<Value<'_>, Error>
        where
            for<'q> &'q T: ToLeaf<scalars::String>,
        {
            value.to_leaf()
        }
        let text = String::from("x");
        let x = Value::Str("x".into());
        assert_eq!(through_any_borrow::<&str>(&text.as_str()).unwrap(), x);
        assert_eq!(through_any_borrow::<&String>(&&text).unwrap(), x);
        assert_eq!(
            through_any_borrow::<Cow<str>>(&Cow::Borrowed(text.as_str())).unwrap(),
            x
        );
        assert_eq!(
            through_any_borrow::<Result<&str, Error>>(&Ok(text.as_str())).unwrap(),
            x
        );
        let failed: Result<String, Error> = Err(Error::new("failed"));
        assert!(through_any_borrow::<Result<String, Error>>(&failed).is_err());
        assert!(ToLeaf::<scalars::String>::leaf_error(&&failed).is_some());
    }

    #[test]
    fn non_finite_floats_are_errors() {
        assert_eq!(
            <f64 as ToLeaf<scalars::Float>>::to_leaf(1.5).unwrap(),
            Value::Float(1.5)
        );
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let error = <f64 as ToLeaf<scalars::Float>>::to_leaf(bad).unwrap_err();
            assert_eq!(error.extensions().unwrap()["code"], "FLOAT_NOT_FINITE");
        }
        assert!(<f32 as ToLeaf<scalars::Float>>::to_leaf(f32::NAN).is_err());
    }

    #[test]
    fn int_inputs_are_range_checked_and_ids_accept_integers() {
        assert_eq!(i32::from_input(&InputValue::Int(5)).unwrap(), 5);
        assert!(i32::from_input(&InputValue::Int(1 << 40)).is_err());
        assert_eq!(String::from_input(&InputValue::Int(5)).unwrap(), "5");
        assert!(bool::from_input(&InputValue::String("true".into())).is_err());
        assert!(Vec::<i32>::from_input(&InputValue::Int(1)).unwrap() == vec![1]);
        // Null at a non-null list is an error even when the items are nullable.
        assert!(Vec::<Option<i32>>::from_input(&InputValue::Null).is_err());
        assert_eq!(
            Option::<Vec<Option<i32>>>::from_input(&InputValue::Null).unwrap(),
            None
        );
        assert!(ToLeaf::<scalars::Float>::leaf_error(&f64::NAN).is_some());
        assert!(ToLeaf::<scalars::Float>::leaf_error(&1.5f64).is_none());
        assert!(ToLeaf::<scalars::Int>::leaf_error(&(1i64 << 40)).is_some());
        // Integers above i64::MAX stay integers through JSON in both directions.
        let big = serde_json::json!(u64::MAX);
        assert_eq!(Value::from_json(&big).to_json(), big);
        assert_eq!(InputValue::UInt(u64::MAX).to_json(), big);
        assert!(i32::from_input(&InputValue::UInt(u64::MAX)).is_err());
        assert_eq!(
            f64::from_input(&InputValue::UInt(1 << 63)).unwrap(),
            (1u64 << 63) as f64
        );
    }
}
