//! Folding an incremental run into its merged response: every payload's
//! incremental entries applied at their paths, streamed items appended, errors
//! collected. `pending`, `completed` and ids only route the entries; the spec
//! leaves batching and order to the executor, so only this result is compared.

use serde_json::{Map, Value};
use std::collections::HashMap;

/// The merged response of one run. A single non-incremental response merges
/// to itself.
pub fn merge(payloads: Vec<Value>) -> Value {
    let mut paths: HashMap<String, Vec<Value>> = HashMap::new();
    let mut data = Value::Null;
    let mut errors = Vec::new();
    for mut payload in payloads {
        let Value::Object(payload) = &mut payload else {
            panic!("payload is not an object: {payload}");
        };
        if let Some(initial) = payload.remove("data") {
            data = initial;
        }
        errors.extend(take_array(payload, "errors"));
        for pending in take_array(payload, "pending") {
            let id = pending["id"].as_str().expect("pending id").to_owned();
            let path = pending["path"].as_array().expect("pending path").clone();
            paths.insert(id, path);
        }
        for mut entry in take_array(payload, "incremental") {
            let Value::Object(entry) = &mut entry else {
                panic!("incremental entry is not an object: {entry}");
            };
            let id = entry["id"].as_str().expect("incremental id");
            let mut path = paths
                .get(id)
                .unwrap_or_else(|| panic!("incremental entry for unannounced id {id}"))
                .clone();
            path.extend(take_array(entry, "subPath"));
            let target = at_path(&mut data, &path);
            if let Some(items) = entry.remove("items") {
                let list = target.as_array_mut().expect("streamed position is a list");
                list.extend(items.as_array().expect("items is a list").iter().cloned());
            }
            if let Some(deferred) = entry.remove("data") {
                merge_into(target, deferred);
            }
            errors.extend(take_array(entry, "errors"));
        }
        for mut completed in take_array(payload, "completed") {
            if let Value::Object(completed) = &mut completed {
                errors.extend(take_array(completed, "errors"));
            }
        }
    }
    let mut response = Map::new();
    response.insert("data".into(), data);
    if !errors.is_empty() {
        response.insert("errors".into(), Value::Array(errors));
    }
    Value::Object(response)
}

fn take_array(object: &mut Map<String, Value>, key: &str) -> Vec<Value> {
    match object.remove(key) {
        Some(Value::Array(items)) => items,
        Some(Value::Null) | None => Vec::new(),
        Some(other) => panic!("{key} is not a list: {other}"),
    }
}

fn at_path<'a>(root: &'a mut Value, path: &[Value]) -> &'a mut Value {
    path.iter().fold(root, |value, segment| match segment {
        Value::String(key) => value
            .as_object_mut()
            .unwrap_or_else(|| panic!("path segment {key:?} into a non-object"))
            .get_mut(key)
            .unwrap_or_else(|| panic!("path segment {key:?} missing")),
        Value::Number(index) => value
            .as_array_mut()
            .unwrap_or_else(|| panic!("path segment {index} into a non-list"))
            .get_mut(index.as_u64().expect("list index") as usize)
            .unwrap_or_else(|| panic!("path segment {index} out of range")),
        other => panic!("path segment is neither key nor index: {other}"),
    })
}

/// Deferred data merges field by field; lists merge item by item since a
/// deferred selection adds fields to objects that were already delivered.
fn merge_into(target: &mut Value, incoming: Value) {
    match (target, incoming) {
        (Value::Object(target), Value::Object(incoming)) => {
            for (key, value) in incoming {
                match target.get_mut(&key) {
                    Some(existing) => merge_into(existing, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (Value::Array(target), Value::Array(incoming)) => {
            for (index, value) in incoming.into_iter().enumerate() {
                match target.get_mut(index) {
                    Some(existing) => merge_into(existing, value),
                    None => target.push(value),
                }
            }
        }
        (target, incoming) => *target = incoming,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn a_single_response_is_its_own_merged_response() {
        let response = json!({"data": {"users": [{"id": "0"}]}});
        assert_eq!(merge(vec![response.clone()]), response);
    }

    #[test]
    fn deferred_data_lands_at_its_pending_path() {
        let payloads = vec![
            json!({
                "data": {"users": [{"id": "0"}, {"id": "1"}]},
                "pending": [{"id": "a", "path": ["users", 0]}, {"id": "b", "path": ["users", 1]}],
                "hasNext": true
            }),
            json!({
                "incremental": [{"id": "b", "data": {"name": "user1"}}],
                "completed": [{"id": "b"}],
                "hasNext": true
            }),
            json!({
                "incremental": [{"id": "a", "data": {"name": "user0"}}],
                "completed": [{"id": "a"}],
                "hasNext": false
            }),
        ];
        assert_eq!(
            merge(payloads),
            json!({"data": {"users": [{"id": "0", "name": "user0"}, {"id": "1", "name": "user1"}]}})
        );
    }

    #[test]
    fn sub_path_and_nested_objects_merge_deeply() {
        let payloads = vec![
            json!({
                "data": {"users": [{"id": "0", "posts": [{"id": "0-0"}, {"id": "0-1"}]}]},
                "pending": [{"id": "a", "path": ["users", 0]}],
                "hasNext": true
            }),
            json!({
                "incremental": [
                    {"id": "a", "data": {"posts": [{"title": "first"}, {"title": "second"}]}},
                    {"id": "a", "subPath": ["posts", 1], "data": {"tags": ["t1"]}}
                ],
                "completed": [{"id": "a"}],
                "hasNext": false
            }),
        ];
        assert_eq!(
            merge(payloads),
            json!({"data": {"users": [{"id": "0", "posts": [
                {"id": "0-0", "title": "first"},
                {"id": "0-1", "title": "second", "tags": ["t1"]}
            ]}]}})
        );
    }

    #[test]
    fn streamed_items_append_in_order_after_the_initial_ones() {
        let payloads = vec![
            json!({
                "data": {"users": [{"id": "0"}]},
                "pending": [{"id": "s", "path": ["users"]}],
                "hasNext": true
            }),
            json!({
                "incremental": [{"id": "s", "items": [{"id": "1"}, {"id": "2"}]}],
                "hasNext": true
            }),
            json!({
                "incremental": [{"id": "s", "items": [{"id": "3"}]}],
                "completed": [{"id": "s"}],
                "hasNext": false
            }),
        ];
        assert_eq!(
            merge(payloads),
            json!({"data": {"users": [{"id": "0"}, {"id": "1"}, {"id": "2"}, {"id": "3"}]}})
        );
    }

    #[test]
    fn groups_announced_later_and_errors_anywhere_are_honored() {
        let payloads = vec![
            json!({
                "data": {"hero": {"name": "R2"}},
                "errors": [{"message": "initial"}],
                "pending": [{"id": "a", "path": ["hero"]}],
                "hasNext": true
            }),
            json!({
                "pending": [{"id": "b", "path": ["hero", "friends"]}],
                "incremental": [{"id": "a", "data": {"friends": []}, "errors": [{"message": "inside"}]}],
                "completed": [{"id": "a"}],
                "hasNext": true
            }),
            json!({
                "incremental": [{"id": "b", "items": [{"name": "C3"}]}],
                "completed": [{"id": "b", "errors": [{"message": "failed"}]}],
                "hasNext": false
            }),
        ];
        assert_eq!(
            merge(payloads),
            json!({
                "data": {"hero": {"name": "R2", "friends": [{"name": "C3"}]}},
                "errors": [
                    {"message": "initial"},
                    {"message": "inside"},
                    {"message": "failed"}
                ]
            })
        );
    }
}
