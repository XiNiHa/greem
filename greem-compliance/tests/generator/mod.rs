#![allow(dead_code)]
use proptest::prelude::*;

#[derive(Clone, Copy, Debug)]
pub enum Ty {
    Query,
    User,
    Post,
    Node,
    Search,
    Mutation,
}

/// A generated selection set for `ty`, valid against the property schema.
pub fn selection(ty: Ty, depth: u32, incremental: bool) -> BoxedStrategy<String> {
    let leaf = |names: &[&'static str]| -> BoxedStrategy<String> {
        let names: Vec<&'static str> = names.to_vec();
        prop::sample::select(names)
            .prop_map(|s| s.to_owned())
            .boxed()
    };
    let deferrable = move |inner: BoxedStrategy<String>| -> BoxedStrategy<String> {
        if incremental && depth > 0 {
            (
                inner,
                prop::bool::weighted(0.3),
                prop::option::of("[a-z]{1,4}"),
            )
                .prop_map(|(sel, defer, label)| {
                    if defer {
                        match label {
                            Some(label) => format!("... @defer(label: \"{label}\") {{ {sel} }}"),
                            None => format!("... @defer {{ {sel} }}"),
                        }
                    } else {
                        sel
                    }
                })
                .boxed()
        } else {
            inner
        }
    };
    let directive = || -> BoxedStrategy<String> {
        prop_oneof![
            4 => Just(String::new()),
            1 => Just(" @skip(if: $flag)".to_owned()),
            1 => Just(" @include(if: $flag)".to_owned()),
            1 => Just(" @skip(if: false)".to_owned()),
        ]
        .boxed()
    };
    // Directives sit between the arguments and the selection set.
    let with_directive = move |s: BoxedStrategy<String>| {
        (s, directive())
            .prop_map(|(f, d)| {
                let brace = f.find('{').unwrap_or(f.len());
                let args_end = if f.starts_with("...") {
                    0
                } else {
                    match f.find('(').filter(|&open| open < brace) {
                        Some(open) => {
                            let mut depth = 0;
                            let mut end = open;
                            for (i, c) in f[open..].char_indices() {
                                match c {
                                    '(' => depth += 1,
                                    ')' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            end = open + i + 1;
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            end
                        }
                        None => 0,
                    }
                };
                match f[args_end..].find(" {") {
                    Some(i) => format!("{}{d}{}", &f[..args_end + i], &f[args_end + i..]),
                    None => format!("{f}{d}"),
                }
            })
            .boxed()
    };
    let fields: BoxedStrategy<String> = match ty {
        Ty::Query => {
            // Root lists stream too: `nodes` and `search` mix item types, so
            // their items' subtrees finish at different barriers.
            let stream = move || -> BoxedStrategy<String> {
                if incremental {
                    prop::option::weighted(0.3, 0..=2u32)
                        .prop_map(|n| match n {
                            Some(n) => format!(" @stream(initialCount: {n})"),
                            None => String::new(),
                        })
                        .boxed()
                } else {
                    Just(String::new()).boxed()
                }
            };
            let users = (
                0..=4i32,
                stream(),
                selection(Ty::User, depth + 1, incremental),
            )
                .prop_map(|(n, st, s)| format!("users(first: {n}){st} {{ {s} }}"));
            let user = ("[0-3]|x", selection(Ty::User, depth + 1, incremental))
                .prop_map(|(id, s)| format!("user(id: \"{id}\") {{ {s} }}"));
            let node = (
                prop::sample::select(vec!["0", "1", "post:0:0", "post:1:1", "nope"]),
                selection(Ty::Node, depth + 1, incremental),
            )
                .prop_map(|(id, s)| format!("node(id: \"{id}\") {{ {s} }}"));
            let nodes =
                (stream(), selection(Ty::Node, depth + 1, incremental)).prop_map(|(st, s)| {
                    format!("nodes(ids: [\"0\", \"post:0:0\", \"zzz\", \"1\"]){st} {{ {s} }}")
                });
            let search = (
                prop::option::of("user[01]"),
                stream(),
                selection(Ty::Search, depth + 1, incremental),
            )
                .prop_map(|(t, st, s)| match t {
                    Some(t) => format!("search(term: \"{t}\"){st} {{ {s} }}"),
                    None => format!("search{st} {{ {s} }}"),
                });
            let echo = prop::sample::select(vec![
                "echo(input: {name: \"n\"}) { name email role }",
                "echo(input: {name: null, role: ADMIN}) { name email role }",
                "echo(input: {}) { name email role }",
                "echo(input: $patch) { name email role }",
            ])
            .prop_map(|s| s.to_owned());
            let json = prop::sample::select(vec![
                "json(value: {a: [1, 2.5, \"x\", null], b: true})",
                "json",
                "json(value: 3)",
            ])
            .prop_map(|s| s.to_owned());
            prop_oneof![
                users,
                user,
                node,
                nodes,
                search,
                Just("matrix".to_owned()),
                echo,
                json,
                Just("__typename".to_owned())
            ]
            .boxed()
        }
        Ty::User => {
            let scalar = leaf(&["id", "uuid", "name", "email", "role", "score", "__typename"]);
            if depth >= 3 {
                scalar
            } else {
                let posts = (0..=3i32, selection(Ty::Post, depth + 1, incremental))
                    .prop_map(|(n, s)| format!("posts(first: {n}) {{ {s} }}"));
                let drafts = (
                    prop::option::of(0..=2u32),
                    selection(Ty::Post, depth + 1, incremental),
                )
                    .prop_map(move |(stream, s)| match stream {
                        Some(n) if incremental => {
                            format!("drafts @stream(initialCount: {n}) {{ {s} }}")
                        }
                        _ => format!("drafts {{ {s} }}"),
                    });
                let friends = selection(Ty::User, depth + 1, incremental)
                    .prop_map(|s| format!("friends {{ {s} }}"));
                prop_oneof![3 => scalar, 1 => posts, 1 => drafts, 1 => friends].boxed()
            }
        }
        Ty::Post => {
            let scalar = leaf(&["id", "title", "tags", "__typename"]);
            if depth >= 3 {
                scalar
            } else {
                let owner = selection(Ty::User, depth + 1, incremental)
                    .prop_map(|s| format!("owner {{ {s} }}"));
                let author = selection(Ty::User, depth + 1, incremental)
                    .prop_map(|s| format!("author {{ {s} }}"));
                prop_oneof![3 => scalar, 1 => owner, 1 => author].boxed()
            }
        }
        Ty::Node => {
            let user = selection(Ty::User, depth + 1, incremental)
                .prop_map(|s| format!("... on User {{ {s} }}"));
            let post = selection(Ty::Post, depth + 1, incremental)
                .prop_map(|s| format!("... on Post {{ {s} }}"));
            let resource = selection(Ty::User, depth + 1, incremental)
                .prop_map(|s| format!("... on Resource {{ id owner {{ {s} }} }}"));
            prop_oneof![
                Just("id".to_owned()),
                Just("__typename".to_owned()),
                user,
                post,
                resource
            ]
            .boxed()
        }
        Ty::Search => {
            let user = selection(Ty::User, depth + 1, incremental)
                .prop_map(|s| format!("... on User {{ {s} }}"));
            let post = selection(Ty::Post, depth + 1, incremental)
                .prop_map(|s| format!("... on Post {{ {s} }}"));
            prop_oneof![Just("__typename".to_owned()), user, post].boxed()
        }
        Ty::Mutation => {
            let rename = ("[0-2]", selection(Ty::User, depth + 1, incremental))
                .prop_map(|(id, s)| format!("rename(id: \"{id}\", name: \"z\") {{ {s} }}"));
            let patch =
                ("[0-2]", selection(Ty::User, depth + 1, incremental)).prop_map(|(id, s)| {
                    format!("patch(id: \"{id}\", patch: {{name: \"p\"}}) {{ {s} }}")
                });
            prop_oneof![3 => rename, 2 => patch, 1 => Just("fail".to_owned())].boxed()
        }
    };
    let field = with_directive(fields);
    let set = prop::collection::vec(deferrable(field), 1..4).prop_map(|fields| {
        // Fields with arguments get unique aliases so merged selections stay valid.
        fields
            .into_iter()
            .enumerate()
            .map(|(i, f)| {
                let name_len = f
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .count();
                if f.starts_with("...") || !f[name_len..].starts_with('(') {
                    f
                } else {
                    format!("f{i}_{}: {f}", &f[..name_len])
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    });
    set.boxed()
}

/// Declares only the variables the body uses (an unused variable is a validation error).
pub fn declare(body: &str) -> String {
    let mut vars = Vec::new();
    if body.contains("$flag") {
        vars.push("$flag: Boolean!");
    }
    if body.contains("$patch") {
        vars.push("$patch: UserPatch!");
    }
    if vars.is_empty() {
        String::new()
    } else {
        format!("({})", vars.join(", "))
    }
}

/// `@defer` labels must be unique per document.
pub fn unique_labels(doc: String) -> String {
    let mut out = String::with_capacity(doc.len());
    let mut rest = doc.as_str();
    let mut n = 0;
    while let Some(i) = rest.find("label: \"") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 8..];
        let end = after.find('"').unwrap();
        out.push_str(&format!("label: \"l{n}{}\"", &after[..end]));
        n += 1;
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn document(incremental: bool) -> impl Strategy<Value = String> {
    prop_oneof![
        5 => selection(Ty::Query, 0, incremental).prop_map(|s| unique_labels(format!("query Q{} {{ {s} }}", declare(&s)))),
        1 => selection(Ty::Mutation, 0, incremental).prop_map(|body| unique_labels(format!("mutation M{} {{ {body} }}", declare(&body)))),
    ]
}
