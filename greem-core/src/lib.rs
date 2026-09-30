//! What `greem-build` and `greem-macros` share: the mapping from GraphQL
//! names to Rust identifiers, used when generating a schema module and when
//! `#[greem::object]` names the generated markers, so the two cannot disagree.

use proc_macro2::{Ident, Span};

const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "else", "enum", "extern", "false", "fn", "for", "if",
    "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static",
    "struct", "trait", "true", "type", "unsafe", "use", "where", "while", "async", "await", "dyn",
    "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof", "unsized",
    "virtual", "yield", "try", "gen",
];
const RESERVED: &[&str] = &[
    "self",
    "Self",
    "super",
    "crate",
    "types",
    "__private",
    "Schema",
];

/// Exact GraphQL spelling as a Rust identifier: keywords become raw
/// identifiers; names that cannot be raw (`self`, `Self`, `super`, `crate`),
/// names the generated module uses (`types`, `__private`, `Schema`) and names
/// already ending in `_` get a trailing underscore, which keeps the mapping
/// injective.
pub fn ident(name: &str, span: Span) -> Ident {
    if RESERVED.contains(&name) || name.ends_with('_') {
        Ident::new(&format!("{name}_"), span)
    } else if KEYWORDS.contains(&name) {
        Ident::new_raw(name, span)
    } else {
        Ident::new(name, span)
    }
}

#[cfg(test)]
mod tests {
    use super::ident;
    use proc_macro2::Span;

    #[test]
    fn mapping() {
        let map = |n: &str| ident(n, Span::call_site()).to_string();
        assert_eq!(map("user"), "user");
        assert_eq!(map("type"), "r#type");
        assert_eq!(map("gen"), "r#gen");
        assert_eq!(map("self"), "self_");
        assert_eq!(map("Schema"), "Schema_");
        assert_eq!(map("Schema_"), "Schema__");
        assert_eq!(map("types"), "types_");
        assert_eq!(map("__private"), "__private_");
    }
}
