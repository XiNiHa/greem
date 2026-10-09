//! The execution tree: one node per response key, merged selections,
//! `@skip`/`@include`/`@defer`/`@stream` applied, built per request before
//! any resolver runs.

use crate::error::{Error, GraphQLError, Location};
use crate::value::InputValue;
use apollo_compiler::Node;
use apollo_compiler::ast::{self, Type};
use apollo_compiler::executable::{self, Field, Operation, Selection, SelectionSet};
use apollo_compiler::response::{JsonMap, JsonValue};
use apollo_compiler::schema::ExtendedType;
use apollo_compiler::validation::Valid;
use apollo_compiler::{ExecutableDocument, Name, Schema as ApolloSchema};
use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub type NodeId = u32;
pub type UsageId = u32;

/// A parsed and validated request document, shared between requests.
pub struct Document {
    pub(crate) doc: Valid<ExecutableDocument>,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Document")
    }
}

/// A request-level failure that aborts before execution.
#[derive(Debug)]
pub struct Abort(pub(crate) Vec<GraphQLError>);

impl Abort {
    pub(crate) fn one(message: impl Into<String>, locations: Vec<Location>) -> Self {
        Abort(vec![GraphQLError::request(message, locations)])
    }
}

#[derive(Clone, Debug)]
pub struct DeferUsage {
    pub id: UsageId,
    pub node: NodeId,
    pub label: Option<String>,
    pub parent: Option<UsageId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamInfo {
    pub initial_count: u32,
    pub label: Option<String>,
}

/// `@stream` arguments as written, defaults applied.
#[derive(PartialEq)]
struct StreamArguments {
    initial_count: Option<InputValue>,
    label: Option<InputValue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Normal,
    Typename,
    Introspection,
}

#[derive(Clone)]
pub(crate) struct Occurrence {
    field: Node<Field>,
    usages: Vec<UsageId>,
}

/// One entry of a collected (grouped) field set at a (node, object type).
#[derive(Clone)]
pub struct SelectedField {
    pub key: String,
    pub name: String,
    pub kind: FieldKind,
    pub child: Option<NodeId>,
    pub composite: bool,
    /// Sorted defer usages this field is collected under; empty means immediate.
    pub usages: Vec<UsageId>,
    pub stream: Option<StreamInfo>,
    pub spans: Vec<Location>,
    pub(crate) definition: Node<ast::FieldDefinition>,
    pub(crate) occurrences: Vec<Occurrence>,
}

pub struct Collected {
    pub fields: Vec<SelectedField>,
    pub introduced: Vec<UsageId>,
}

enum Source {
    Root,
    Fields(Vec<Occurrence>),
}

pub struct TreeNode {
    pub key: String,
    pub name: String,
    pub parent: Option<NodeId>,
    pub depth: u32,
    pub stream: Option<StreamInfo>,
    /// The defer usages the field producing this node's objects is delivered
    /// under; its fields are grouped relative to them.
    pub base: Vec<UsageId>,
    source: Source,
}

pub struct Tree {
    pub(crate) schema: Arc<Valid<ApolloSchema>>,
    pub(crate) doc: Arc<Document>,
    pub(crate) operation: Node<Operation>,
    pub(crate) variables: Valid<JsonMap>,
    pub(crate) nodes: Vec<TreeNode>,
    pub(crate) usages: Vec<DeferUsage>,
    pub(crate) incremental: bool,
    pub(crate) max_depth: u32,
    collected: HashMap<(NodeId, String), Arc<Collected>>,
}

/// The RFC's HasNoOverlappingStreams, which apollo-compiler does not check:
/// in every selection set of the document, two selections of one response
/// name cannot merge when either carries `@stream`, however it is written.
/// Like the rest of validation this ignores `@skip`, `@include`, variables
/// and type conditions: a document is valid or not before any request.
pub(crate) fn overlapping_streams(
    doc: &ExecutableDocument,
    schema: &ApolloSchema,
) -> Vec<GraphQLError> {
    let mut check = OverlapCheck {
        doc,
        schema,
        errors: Vec::new(),
        checked: HashSet::new(),
    };
    for operation in doc.operations.iter() {
        check.sets(&[&operation.selection_set]);
    }
    for fragment in doc.fragments.values() {
        check.sets(&[&fragment.selection_set]);
    }
    check.errors
}

struct OverlapCheck<'d> {
    doc: &'d ExecutableDocument,
    schema: &'d ApolloSchema,
    errors: Vec<GraphQLError>,
    /// Selection sets, and merged pairs of them, already checked.
    checked: HashSet<Vec<*const SelectionSet>>,
}

impl<'d> OverlapCheck<'d> {
    /// Checks the fields of `sets` taken together, as FieldsInSetCanMerge
    /// does for one set or for the merged sets of two fields that merge.
    fn sets(&mut self, sets: &[&'d SelectionSet]) {
        let mut key: Vec<*const SelectionSet> = sets.iter().map(|s| *s as *const _).collect();
        key.sort_unstable();
        if !self.checked.insert(key) {
            return;
        }
        let mut fields: IndexMap<&str, Vec<(&'d Node<Field>, &'d Name)>> = IndexMap::new();
        let mut visited = Vec::new();
        for set in sets {
            self.gather(set, &set.ty, &mut fields, &mut visited);
        }
        for (key, occurrences) in &fields {
            if occurrences.len() > 1
                && let Some((streamed, _)) = occurrences
                    .iter()
                    .find(|(f, _)| f.directives.get("stream").is_some())
            {
                self.errors.push(GraphQLError::request(
                    format!(
                        "fields `{key}` conflict because they have overlapping stream directives"
                    ),
                    span_location(streamed.location(), self.doc),
                ));
            }
        }
        for occurrences in fields.values() {
            for (i, (a, parent_a)) in occurrences.iter().enumerate() {
                self.sets(&[&a.selection_set]);
                for (b, parent_b) in &occurrences[i + 1..] {
                    if parent_a == parent_b
                        || !self.is_object(parent_a)
                        || !self.is_object(parent_b)
                    {
                        self.sets(&[&a.selection_set, &b.selection_set]);
                    }
                }
            }
        }
    }

    fn is_object(&self, ty: &Name) -> bool {
        self.schema.types.get(ty).is_some_and(|t| t.is_object())
    }

    /// The fields of `set` by response name, visiting fragments, each with
    /// the type its selection set is written against.
    fn gather(
        &self,
        set: &'d SelectionSet,
        parent: &'d Name,
        fields: &mut IndexMap<&'d str, Vec<(&'d Node<Field>, &'d Name)>>,
        visited: &mut Vec<&'d Name>,
    ) {
        for selection in &set.selections {
            match selection {
                Selection::Field(field) => fields
                    .entry(field.response_key().as_str())
                    .or_default()
                    .push((field, parent)),
                Selection::InlineFragment(fragment) => {
                    let parent = fragment.type_condition.as_ref().unwrap_or(parent);
                    self.gather(&fragment.selection_set, parent, fields, visited);
                }
                Selection::FragmentSpread(spread) => {
                    if visited.contains(&&spread.fragment_name) {
                        continue;
                    }
                    visited.push(&spread.fragment_name);
                    if let Some(fragment) = self.doc.fragments.get(&spread.fragment_name) {
                        let parent = &fragment.selection_set.ty;
                        self.gather(&fragment.selection_set, parent, fields, visited);
                    }
                }
            }
        }
    }
}

pub(crate) fn span_location(
    node_location: Option<apollo_compiler::parser::SourceSpan>,
    doc: &ExecutableDocument,
) -> Vec<Location> {
    node_location
        .and_then(|span| span.line_column(&doc.sources))
        .map(|lc| {
            vec![Location {
                line: lc.line,
                column: lc.column,
            }]
        })
        .unwrap_or_default()
}

impl Tree {
    pub(crate) fn new(
        schema: Arc<Valid<ApolloSchema>>,
        doc: Arc<Document>,
        operation: Node<Operation>,
        variables: Valid<JsonMap>,
        incremental: bool,
        max_depth: u32,
    ) -> Self {
        let root = TreeNode {
            key: String::new(),
            name: String::new(),
            parent: None,
            depth: 0,
            stream: None,
            base: Vec::new(),
            source: Source::Root,
        };
        Self {
            schema,
            doc,
            operation,
            variables,
            nodes: vec![root],
            usages: Vec::new(),
            incremental,
            max_depth,
            collected: HashMap::new(),
        }
    }

    pub fn root(&self) -> NodeId {
        0
    }

    pub fn node(&self, id: NodeId) -> &TreeNode {
        &self.nodes[id as usize]
    }

    pub fn usage(&self, id: UsageId) -> &DeferUsage {
        &self.usages[id as usize]
    }

    pub fn is_mutation(&self) -> bool {
        self.operation.operation_type == ast::OperationType::Mutation
    }

    pub(crate) fn usage_is_ancestor(&self, ancestor: UsageId, mut usage: UsageId) -> bool {
        while let Some(parent) = self.usages[usage as usize].parent {
            if parent == ancestor {
                return true;
            }
            usage = parent;
        }
        false
    }

    /// Collects the grouped field set for `typename` at `node`, creating child
    /// nodes for new response keys and checking the depth limit.
    pub(crate) fn collect(
        &mut self,
        node: NodeId,
        typename: &str,
    ) -> Result<Arc<Collected>, Abort> {
        let key = (node, typename.to_owned());
        if let Some(c) = self.collected.get(&key) {
            return Ok(c.clone());
        }
        let mut groups: IndexMap<String, Vec<Occurrence>> = IndexMap::new();
        let mut introduced = Vec::new();
        match &self.nodes[node as usize].source {
            Source::Root => {
                let set = self.operation.selection_set.clone();
                self.collect_set(
                    &set,
                    typename,
                    &[],
                    &mut groups,
                    &mut introduced,
                    node,
                    &mut Vec::new(),
                )?;
            }
            Source::Fields(occurrences) => {
                // One visited set for every occurrence merged into this node,
                // as CollectSubfields shares it across the merged fields.
                let mut visited = Vec::new();
                for occ in occurrences.clone() {
                    // Each occurrence keeps the usages it was collected under,
                    // so a nested defer sits under the fragment that reached
                    // it, not under another one that merely selects the field.
                    self.collect_set(
                        &occ.field.selection_set,
                        typename,
                        &occ.usages,
                        &mut groups,
                        &mut introduced,
                        node,
                        &mut visited,
                    )?;
                }
            }
        }
        let parent_depth = self.nodes[node as usize].depth;
        let base = self.nodes[node as usize].base.clone();
        let mut fields = Vec::with_capacity(groups.len());
        for (key, occurrences) in groups {
            let first = &occurrences[0].field;
            let name = first.name.to_string();
            let kind = match name.as_str() {
                "__typename" => FieldKind::Typename,
                "__schema" | "__type" if node == 0 => FieldKind::Introspection,
                _ => FieldKind::Normal,
            };
            // The RFC's BuildExecutionPlan: a field whose filtered usage set
            // is its parent's runs with the parent's set; any other set is a
            // deferred set of its own, keyed by the fragments that select
            // the field, which may all be ancestors already delivering here.
            let set = self.field_usages(&occurrences);
            let usages = if set == base { Vec::new() } else { set.clone() };
            let directive = if kind == FieldKind::Normal {
                self.stream_arguments(first)?
            } else {
                None
            };
            // Disabled ignores the directive: its arguments are only
            // validated when it takes effect.
            let stream = match directive {
                Some((_, arguments)) if self.incremental => Some(self.stream_info(arguments)?),
                _ => None,
            };
            let composite = matches!(
                self.schema
                    .types
                    .get(first.definition.ty.inner_named_type()),
                Some(ExtendedType::Object(_) | ExtendedType::Interface(_) | ExtendedType::Union(_))
            );
            let spans = occurrences
                .iter()
                .flat_map(|o| span_location(o.field.location(), &self.doc.doc))
                .collect();
            // Every selection counts toward the depth, `__typename` under a
            // deferred fragment included; only normal fields get a node.
            let mut depth = parent_depth
                + usages
                    .iter()
                    .map(|&u| self.defer_levels(u, &base).max(1))
                    .max()
                    .unwrap_or(0);
            if kind == FieldKind::Normal {
                if composite {
                    depth += 1;
                }
                if stream.is_some() {
                    depth += 1;
                }
            }
            if depth > self.max_depth {
                return Err(Abort::one(
                    format!(
                        "operation depth {depth} exceeds the maximum execution depth of {}",
                        self.max_depth
                    ),
                    spans,
                ));
            }
            let child = if kind == FieldKind::Normal {
                let id = self.nodes.len() as NodeId;
                self.nodes.push(TreeNode {
                    key: key.clone(),
                    name: name.clone(),
                    parent: Some(node),
                    depth,
                    stream: stream.clone(),
                    base: set,
                    source: Source::Fields(occurrences.clone()),
                });
                Some(id)
            } else {
                None
            };
            // Arguments coerce against the concrete object's definition: its
            // defaults may differ from the interface's.
            let definition = match self.schema.type_field(typename, &name) {
                Ok(definition) => definition.node.clone(),
                Err(_) => first.definition.clone(),
            };
            fields.push(SelectedField {
                key,
                name,
                kind,
                child,
                composite,
                usages,
                stream,
                spans,
                definition,
                occurrences,
            });
        }
        let collected = Arc::new(Collected { fields, introduced });
        self.collected.insert(key, collected.clone());
        Ok(collected)
    }

    /// How many deferred fragments nest between `usage` and the node's base.
    fn defer_levels(&self, usage: UsageId, base: &[UsageId]) -> u32 {
        let mut levels = 0;
        let mut cursor = Some(usage);
        while let Some(u) = cursor {
            if base.contains(&u) {
                break;
            }
            levels += 1;
            cursor = self.usages[u as usize].parent;
        }
        levels
    }

    fn field_usages(&self, occurrences: &[Occurrence]) -> Vec<UsageId> {
        if occurrences.iter().any(|o| o.usages.is_empty()) {
            return Vec::new();
        }
        let mut set: Vec<UsageId> = occurrences
            .iter()
            .filter_map(|o| o.usages.last().copied())
            .collect();
        set.sort_unstable();
        set.dedup();
        let pruned: Vec<UsageId> = set
            .iter()
            .copied()
            .filter(|&u| !set.iter().any(|&a| a != u && self.usage_is_ancestor(a, u)))
            .collect();
        pruned
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_set(
        &mut self,
        set: &SelectionSet,
        typename: &str,
        usage_path: &[UsageId],
        groups: &mut IndexMap<String, Vec<Occurrence>>,
        introduced: &mut Vec<UsageId>,
        node: NodeId,
        visited: &mut Vec<(Name, Vec<UsageId>)>,
    ) -> Result<(), Abort> {
        for selection in &set.selections {
            if !self.included(selection.directives())? {
                continue;
            }
            match selection {
                Selection::Field(field) => {
                    groups
                        .entry(field.response_key().to_string())
                        .or_default()
                        .push(Occurrence {
                            field: field.clone(),
                            usages: usage_path.to_vec(),
                        });
                }
                Selection::InlineFragment(fragment) => {
                    if let Some(condition) = &fragment.type_condition
                        && !self.type_condition_matches(condition, typename)
                    {
                        continue;
                    }
                    let path =
                        self.defer_path(&fragment.directives, usage_path, node, introduced)?;
                    let inner = fragment.selection_set.clone();
                    self.collect_set(&inner, typename, &path, groups, introduced, node, visited)?;
                }
                Selection::FragmentSpread(spread) => {
                    let Some(fragment) = self.doc.doc.fragments.get(&spread.fragment_name).cloned()
                    else {
                        continue;
                    };
                    if !self.type_condition_matches(fragment.type_condition(), typename) {
                        continue;
                    }
                    let path = self.defer_path(&spread.directives, usage_path, node, introduced)?;
                    // A spread that is not itself deferred is collected once
                    // per node and delivery context: the usages it is reached
                    // under here, and the usage the occurrence itself sits
                    // under, which its nested defers depend on. A deferred
                    // spread is its own usage every time.
                    if path.len() == usage_path.len() {
                        let marker = (spread.fragment_name.clone(), usage_path.to_vec());
                        if visited.contains(&marker) {
                            continue;
                        }
                        visited.push(marker);
                    }
                    self.collect_set(
                        &fragment.selection_set,
                        typename,
                        &path,
                        groups,
                        introduced,
                        node,
                        visited,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn defer_path(
        &mut self,
        directives: &executable::DirectiveList,
        usage_path: &[UsageId],
        node: NodeId,
        introduced: &mut Vec<UsageId>,
    ) -> Result<Vec<UsageId>, Abort> {
        let Some(defer) = directives.get("defer") else {
            return Ok(usage_path.to_vec());
        };
        if !self.incremental {
            return Ok(usage_path.to_vec());
        }
        if !self.incremental_condition(defer)? {
            return Ok(usage_path.to_vec());
        }
        let label = match self.directive_arg(defer, "label")? {
            Some(InputValue::String(s)) => Some(s),
            _ => None,
        };
        let id = self.usages.len() as UsageId;
        self.usages.push(DeferUsage {
            id,
            node,
            label,
            parent: usage_path.last().copied(),
        });
        introduced.push(id);
        let mut path = usage_path.to_vec();
        path.push(id);
        Ok(path)
    }

    /// The field's `@stream` with its arguments resolved but not validated:
    /// what merged fields must agree on whether or not incremental delivery
    /// is enabled. `None` when absent or switched off by `if`.
    #[allow(clippy::type_complexity)]
    fn stream_arguments<'f>(
        &self,
        field: &'f Node<Field>,
    ) -> Result<Option<(&'f Node<executable::Directive>, StreamArguments)>, Abort> {
        let Some(stream) = field.directives.get("stream") else {
            return Ok(None);
        };
        let enabled = match self.directive_arg(stream, "if")? {
            None | Some(InputValue::Bool(true)) => true,
            // Null is an error once the directive takes effect.
            Some(InputValue::Null) => self.incremental && self.incremental_condition(stream)?,
            Some(_) => false,
        };
        if !enabled {
            return Ok(None);
        }
        Ok(Some((
            stream,
            StreamArguments {
                initial_count: self.directive_arg(stream, "initialCount")?,
                label: self.directive_arg(stream, "label")?,
            },
        )))
    }

    /// A `@stream` that takes effect. A negative `initialCount` is not a
    /// request failure: it is raised as an execution error when the field's
    /// arguments are coerced, and streams nothing initially until then.
    fn stream_info(&self, arguments: StreamArguments) -> Result<StreamInfo, Abort> {
        let initial_count = match arguments.initial_count {
            Some(InputValue::Int(i)) if i >= 0 => i as u32,
            _ => 0,
        };
        let label = match arguments.label {
            Some(InputValue::String(s)) => Some(s),
            _ => None,
        };
        Ok(StreamInfo {
            initial_count,
            label,
        })
    }

    fn included(&self, directives: &executable::DirectiveList) -> Result<bool, Abort> {
        if let Some(skip) = directives.get("skip")
            && let Some(cond) = skip.specified_argument_by_name("if")
            && self.bool_value(cond)?
        {
            return Ok(false);
        }
        if let Some(include) = directives.get("include")
            && let Some(cond) = include.specified_argument_by_name("if")
            && !self.bool_value(cond)?
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// `if` of `@defer`/`@stream` (`Boolean! = true`) as argument coercion
    /// sees it: unspecified, or a variable the request did not provide, is
    /// the default; an explicit null is an error.
    fn incremental_condition(
        &self,
        directive: &Node<executable::Directive>,
    ) -> Result<bool, Abort> {
        match self.directive_arg(directive, "if")? {
            None => Ok(true),
            Some(InputValue::Bool(b)) => Ok(b),
            Some(InputValue::Null) => Err(Abort::one(
                format!(
                    "Argument \"if\" of non-null type \"Boolean!\" must not be null on @{}",
                    directive.name
                ),
                span_location(directive.location(), &self.doc.doc),
            )),
            Some(_) => Ok(false),
        }
    }

    /// A directive argument as argument coercion sees it: unspecified, or
    /// bound to a variable the request did not provide, takes the default
    /// from the directive's definition in the schema.
    fn directive_arg(
        &self,
        directive: &Node<executable::Directive>,
        name: &str,
    ) -> Result<Option<InputValue>, Abort> {
        if let Some(value) = directive.specified_argument_by_name(name)
            && let Some(resolved) = self.resolve_value(value)?
        {
            return Ok(Some(resolved));
        }
        Ok(self
            .schema
            .directive_definitions
            .get(directive.name.as_str())
            .and_then(|definition| definition.argument_by_name(name))
            .and_then(|argument| argument.default_value.as_ref())
            .map(|default| literal_to_input(&self.schema, None, default, &self.variables)))
    }

    fn bool_value(&self, value: &Node<ast::Value>) -> Result<bool, Abort> {
        match self.resolve_value(value)? {
            Some(InputValue::Bool(b)) => Ok(b),
            _ => Ok(false),
        }
    }

    fn resolve_value(&self, value: &Node<ast::Value>) -> Result<Option<InputValue>, Abort> {
        match value.as_ref() {
            ast::Value::Variable(name) => Ok(self
                .variables
                .get(name.as_str())
                .map(|v| json_to_input(&self.schema, None, v))),
            other => Ok(Some(literal_to_input(
                &self.schema,
                None,
                other,
                &self.variables,
            ))),
        }
    }

    fn type_condition_matches(&self, condition: &Name, typename: &str) -> bool {
        if condition.as_str() == typename {
            return true;
        }
        match self.schema.types.get(condition.as_str()) {
            Some(ExtendedType::Interface(_)) => match self.schema.types.get(typename) {
                Some(ExtendedType::Object(object)) => {
                    object.implements_interfaces.contains(condition.as_str())
                }
                Some(ExtendedType::Interface(interface)) => {
                    interface.implements_interfaces.contains(condition.as_str())
                }
                _ => false,
            },
            Some(ExtendedType::Union(union)) => union.members.contains(typename),
            _ => false,
        }
    }

    /// Spec `CoerceArgumentValues` over the first occurrence of a selected
    /// field, after the arguments of a `@stream` that takes effect.
    pub(crate) fn coerce_arguments(&self, field: &SelectedField) -> Result<InputValue, Error> {
        let occurrence = &field.occurrences[0].field;
        if field.stream.is_some()
            && let Ok(Some((_, arguments))) = self.stream_arguments(occurrence)
            && matches!(arguments.initial_count, Some(InputValue::Int(i)) if i < 0)
        {
            return Err(Error::new("initialCount must not be negative"));
        }
        let mut out = Vec::new();
        for definition in &field.definition.arguments {
            let name = definition.name.as_str();
            let provided = occurrence
                .arguments
                .iter()
                .find(|a| a.name.as_str() == name);
            let value = match provided {
                Some(argument) => match argument.value.as_ref() {
                    ast::Value::Variable(var) => match self.variables.get(var.as_str()) {
                        Some(json) => Some(json_to_input(&self.schema, Some(&definition.ty), json)),
                        None => match &definition.default_value {
                            Some(default) => Some(literal_to_input(
                                &self.schema,
                                Some(&definition.ty),
                                default,
                                &self.variables,
                            )),
                            None if definition.ty.is_non_null() => {
                                return Err(Error::framework(
                                    format!(
                                        "argument `{name}` of type `{}` was provided the variable `${var}` which was not provided a runtime value",
                                        definition.ty
                                    ),
                                    "BAD_USER_INPUT",
                                ));
                            }
                            None => None,
                        },
                    },
                    literal => Some(literal_to_input(
                        &self.schema,
                        Some(&definition.ty),
                        literal,
                        &self.variables,
                    )),
                },
                None => definition.default_value.as_ref().map(|default| {
                    literal_to_input(&self.schema, Some(&definition.ty), default, &self.variables)
                }),
            };
            if let Some(value) = value {
                if definition.ty.is_non_null() && value == InputValue::Null {
                    return Err(Error::framework(
                        format!(
                            "argument `{name}` of non-null type `{}` must not be null",
                            definition.ty
                        ),
                        "BAD_USER_INPUT",
                    ));
                }
                out.push((name.to_owned(), value));
            }
        }
        Ok(InputValue::Object(out))
    }
}

/// Converts a coerced variable (JSON) into an [`InputValue`] guided by the
/// schema type, so enum positions become `InputValue::Enum`.
pub(crate) fn json_to_input(
    schema: &ApolloSchema,
    ty: Option<&Type>,
    json: &JsonValue,
) -> InputValue {
    match json {
        JsonValue::Null => InputValue::Null,
        JsonValue::Bool(b) => InputValue::Bool(*b),
        JsonValue::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => InputValue::Int(i),
            (None, Some(u)) => InputValue::UInt(u),
            (None, None) => InputValue::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        JsonValue::String(s) => {
            let named = ty.map(|t| t.inner_named_type().as_str());
            match named.and_then(|n| schema.types.get(n)) {
                Some(ExtendedType::Enum(_)) if !ty.is_some_and(Type::is_list) => {
                    InputValue::Enum(s.as_str().to_owned())
                }
                _ => InputValue::String(s.as_str().to_owned()),
            }
        }
        JsonValue::Array(items) => {
            let item_ty = ty.and_then(|t| {
                if t.is_list() {
                    Some(t.item_type())
                } else {
                    None
                }
            });
            InputValue::List(
                items
                    .iter()
                    .map(|item| json_to_input(schema, item_ty, item))
                    .collect(),
            )
        }
        JsonValue::Object(fields) => {
            let named = ty.map(|t| t.inner_named_type().as_str());
            let input_object = match named.and_then(|n| schema.types.get(n)) {
                Some(ExtendedType::InputObject(io)) => Some(io),
                _ => None,
            };
            InputValue::Object(
                fields
                    .iter()
                    .map(|(k, v)| {
                        let field_ty = input_object
                            .and_then(|io| io.fields.get(k.as_str()))
                            .map(|f| f.ty.as_ref());
                        (k.as_str().to_owned(), json_to_input(schema, field_ty, v))
                    })
                    .collect(),
            )
        }
    }
}

pub(crate) fn literal_to_input(
    schema: &ApolloSchema,
    ty: Option<&Type>,
    value: &ast::Value,
    variables: &JsonMap,
) -> InputValue {
    match value {
        ast::Value::Null => InputValue::Null,
        ast::Value::Enum(name) => InputValue::Enum(name.as_str().to_owned()),
        ast::Value::Variable(name) => match variables.get(name.as_str()) {
            Some(json) => json_to_input(schema, ty, json),
            None => InputValue::Null,
        },
        ast::Value::String(s) => InputValue::String(s.clone()),
        ast::Value::Float(f) => InputValue::Float(f.try_to_f64().unwrap_or(f64::NAN)),
        ast::Value::Int(i) => match (i.as_str().parse::<i64>(), i.as_str().parse::<u64>()) {
            (Ok(v), _) => InputValue::Int(v),
            (Err(_), Ok(v)) => InputValue::UInt(v),
            (Err(_), Err(_)) => InputValue::Float(i.as_str().parse().unwrap_or(f64::NAN)),
        },
        ast::Value::Boolean(b) => InputValue::Bool(*b),
        ast::Value::List(items) => {
            let item_ty = ty.and_then(|t| {
                if t.is_list() {
                    Some(t.item_type())
                } else {
                    None
                }
            });
            InputValue::List(
                items
                    .iter()
                    .map(|item| literal_to_input(schema, item_ty, item, variables))
                    .collect(),
            )
        }
        ast::Value::Object(fields) => {
            let named = ty.map(|t| t.inner_named_type().as_str());
            let input_object = match named.and_then(|n| schema.types.get(n)) {
                Some(ExtendedType::InputObject(io)) => Some(io),
                _ => None,
            };
            let mut out = Vec::new();
            for (name, value) in fields {
                let field_ty = input_object
                    .and_then(|io| io.fields.get(name.as_str()))
                    .map(|f| f.ty.as_ref());
                if let ast::Value::Variable(var) = value.as_ref()
                    && !variables.contains_key(var.as_str())
                {
                    continue;
                }
                out.push((
                    name.as_str().to_owned(),
                    literal_to_input(schema, field_ty, value, variables),
                ));
            }
            if let Some(io) = input_object {
                for (name, def) in &io.fields {
                    if !out.iter().any(|(k, _)| k == name.as_str())
                        && let Some(default) = &def.default_value
                    {
                        out.push((
                            name.as_str().to_owned(),
                            literal_to_input(schema, Some(&def.ty), default, variables),
                        ));
                    }
                }
            }
            InputValue::Object(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_coerce_against_the_concrete_type() {
        let sdl = "interface Node { value(x: Int = 1): Int }\n\
            type User implements Node { value(x: Int = 2): Int }\n\
            type Query { node: Node }";
        let schema = Arc::new(ApolloSchema::parse_and_validate(sdl, "s.graphql").unwrap());
        let doc =
            ExecutableDocument::parse_and_validate(&schema, "{ node { value } }", "q.graphql")
                .unwrap();
        let operation = doc.operations.get(None).unwrap().clone();
        let variables =
            apollo_compiler::request::coerce_variable_values(&schema, &operation, &JsonMap::new())
                .unwrap();
        let mut tree = Tree::new(
            schema,
            Arc::new(Document { doc }),
            operation,
            variables,
            false,
            32,
        );
        let node = tree.collect(0, "Query").unwrap().fields[0].child.unwrap();
        let user = tree.collect(node, "User").unwrap();
        // The selection was written against `Node`; the default is `User`'s.
        assert_eq!(
            tree.coerce_arguments(&user.fields[0]).ok(),
            Some(InputValue::Object(vec![(
                "x".to_owned(),
                InputValue::Int(2)
            )]))
        );
    }

    /// A helper for the grouping tests: the tree of `query` over `sdl` with
    /// incremental delivery on.
    fn tree_of(sdl: &str, query: &str) -> Tree {
        let sdl = format!(
            "directive @defer(if: Boolean! = true, label: String) on FRAGMENT_SPREAD | INLINE_FRAGMENT\n\
            directive @stream(if: Boolean! = true, label: String, initialCount: Int = 0) on FIELD\n{sdl}"
        );
        let schema = Arc::new(ApolloSchema::parse_and_validate(sdl, "s.graphql").unwrap());
        let doc = ExecutableDocument::parse_and_validate(&schema, query, "q.graphql").unwrap();
        let operation = doc.operations.get(None).unwrap().clone();
        let variables =
            apollo_compiler::request::coerce_variable_values(&schema, &operation, &JsonMap::new())
                .unwrap();
        Tree::new(
            schema,
            Arc::new(Document { doc }),
            operation,
            variables,
            true,
            32,
        )
    }

    fn usages_of(collected: &Collected) -> Vec<(&str, Vec<UsageId>)> {
        collected
            .fields
            .iter()
            .map(|f| (f.key.as_str(), f.usages.clone()))
            .collect()
    }

    #[test]
    fn fields_are_grouped_by_their_own_usage_set_not_their_parents() {
        // The RFC's BuildExecutionPlan: `hero` is shared by both fragments,
        // and each subfield only one of them selects is a set of its own
        // keyed by that fragment, not part of the shared set.
        let mut tree = tree_of(
            "type Query { hero: Hero } type Hero { id: ID name: String both: Int }",
            "{ ... @defer { hero { id both } } ... @defer { hero { name both } } }",
        );
        let root = tree.collect(0, "Query").unwrap();
        assert_eq!(root.introduced, vec![0, 1]);
        assert_eq!(usages_of(&root), vec![("hero", vec![0, 1])]);
        let hero = root.fields[0].child.unwrap();
        let hero = tree.collect(hero, "Hero").unwrap();
        assert_eq!(
            usages_of(&hero),
            vec![("id", vec![0]), ("both", vec![]), ("name", vec![1])]
        );
    }

    #[test]
    fn a_field_under_one_fragment_only_is_immediate_relative_to_it() {
        let mut tree = tree_of(
            "type Query { hero: Hero } type Hero { id: ID name: String }",
            "{ hero { id } ... @defer { hero { name ... @defer { id } } } }",
        );
        let root = tree.collect(0, "Query").unwrap();
        assert_eq!(usages_of(&root), vec![("hero", vec![])]);
        let hero = root.fields[0].child.unwrap();
        let hero = tree.collect(hero, "Hero").unwrap();
        // `id` is immediate; `name` is the outer fragment's own set; the
        // inner fragment selects only `id`, which the initial set already
        // delivers, so it introduces a usage with no set.
        assert_eq!(usages_of(&hero), vec![("id", vec![]), ("name", vec![0])]);
        assert_eq!(hero.introduced, vec![1]);
    }

    #[test]
    fn incremental_directive_defaults_come_from_the_schema() {
        let sdl = "directive @defer(if: Boolean! = false, label: String = \"deferred-default\") on FRAGMENT_SPREAD | INLINE_FRAGMENT\n\
            directive @stream(if: Boolean! = true, label: String = \"stream-default\", initialCount: Int = 2) on FIELD\n\
            type Query { a: [Int] b: Int }";
        let schema = Arc::new(ApolloSchema::parse_and_validate(sdl, "s.graphql").unwrap());
        let doc = ExecutableDocument::parse_and_validate(
            &schema,
            "query($c: Boolean, $n: Int) { a @stream(initialCount: $n) ... @defer { b } ... @defer(if: $c) { b } ... @defer(if: true) { b } }",
            "q.graphql",
        )
        .unwrap();
        let operation = doc.operations.get(None).unwrap().clone();
        let variables =
            apollo_compiler::request::coerce_variable_values(&schema, &operation, &JsonMap::new())
                .unwrap();
        let mut tree = Tree::new(
            schema,
            Arc::new(Document { doc }),
            operation.clone(),
            variables,
            true,
            32,
        );
        let selections = &operation.selection_set.selections;
        let Selection::Field(a) = &selections[0] else {
            panic!("field");
        };
        // $n unprovided and no label: the definition defaults.
        let (_, arguments) = tree.stream_arguments(a).unwrap().unwrap();
        let info = tree.stream_info(arguments).unwrap();
        assert_eq!(info.initial_count, 2);
        assert_eq!(info.label.as_deref(), Some("stream-default"));
        let mut introduced = Vec::new();
        for (i, deferred) in [(1, false), (2, false), (3, true)] {
            let Selection::InlineFragment(fragment) = &selections[i] else {
                panic!("fragment");
            };
            let path = tree
                .defer_path(&fragment.directives, &[], 0, &mut introduced)
                .unwrap();
            assert_eq!(!path.is_empty(), deferred, "selection {i}");
        }
        assert_eq!(tree.usages.len(), 1);
        assert_eq!(tree.usages[0].label.as_deref(), Some("deferred-default"));
    }
}
