use std::{collections::HashMap, fs, path::PathBuf};

use graphql_parser::schema::*;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

pub mod config;
pub mod error;

use crate::error::*;

pub fn codegen(config: config::Config) -> Result<(), GreemBuildError> {
    let targets = resolve_targets(config.schema);
    println!("{:?}", targets);
    println!(
        "{:?}",
        PathBuf::try_from(config.output_directory).map_err(|_| GreemBuildError::InvalidConfig)?
    );

    let individuals = targets
        .par_iter()
        .filter_map(|target| {
            let content = fs::read_to_string(target).ok()?;
            let schema =
                graphql_parser::parse_schema::<String>(Box::leak(Box::new(content))).ok()?;

            Some(schema.definitions)
        })
        .flatten()
        .collect::<Vec<_>>();
    let combined = combine_definitions(individuals)?;

    Ok(())
}

fn resolve_targets(schema_specifiers: Vec<&'static str>) -> Vec<PathBuf> {
    schema_specifiers
        .into_iter()
        .filter_map(|specifier| {
            glob::glob(specifier)
                .map(|entries| entries.filter_map(Result::ok).collect::<Vec<_>>())
                .ok()
        })
        .flatten()
        .collect()
}

fn combine_definitions(
    definitions: Vec<Definition<String>>,
) -> Result<Vec<Definition<String>>, GreemBuildError> {
    let mut schema_def = None;
    let mut hashmap = HashMap::<String, Definition<String>>::new();

    for definition in &definitions {
        match definition {
            Definition::SchemaDefinition(def) => match schema_def {
                None => schema_def = Some(def),
                Some(_) => {
                    return Err(GreemBuildError::MultipleDefinitionFound(
                        CollidedDefinition::Schema,
                    ))
                }
            },
            Definition::TypeDefinition(def) => match def {
                TypeDefinition::Scalar(def) => match hashmap.contains_key(&def.name) {
                    false => {
                        hashmap.insert(def.name.clone(), definition.clone());
                    }
                    true => {
                        return Err(GreemBuildError::MultipleDefinitionFound(
                            CollidedDefinition::Scalar(def.name.clone()),
                        ))
                    }
                },
                TypeDefinition::Object(def) => match hashmap.get_mut(&def.name) {
                    Some(present_def) => match present_def {
                        Definition::TypeDefinition(TypeDefinition::Object(present_def)) => {
                            let combined_description =
                                combine_description(&present_def.description, &def.description);
                            if combined_description.is_some() {
                                present_def.description = combined_description;
                            }

                            let combined_directives =
                                combine_nameable_vectors(&present_def.directives, &def.directives)?;
                            if let Some(combined_directives) = combined_directives {
                                present_def.directives = combined_directives;
                            }

                            for interface in def.implements_interfaces.iter() {
                                if !present_def.implements_interfaces.contains(&interface) {
                                    present_def.implements_interfaces.push(interface.clone());
                                }
                            }

                            let combined_fields =
                                combine_nameable_vectors(&present_def.fields, &def.fields)?;
                            if let Some(combined_fields) = combined_fields {
                                present_def.fields = combined_fields;
                            }
                        }
                        _ => {
                            return Err(GreemBuildError::NameCollision(
                                def.name.clone(),
                                NameCollisionReason::DifferentType,
                            ))
                        }
                    },
                    None => {
                        hashmap.insert(def.name.clone(), definition.clone());
                    }
                },
                TypeDefinition::Interface(_) => todo!(),
                TypeDefinition::Union(_) => todo!(),
                TypeDefinition::Enum(_) => todo!(),
                TypeDefinition::InputObject(_) => todo!(),
            },
            Definition::TypeExtension(_) => todo!(),
            Definition::DirectiveDefinition(_) => todo!(),
        }
    }

    todo!()
}

fn combine_description(present: &Option<String>, cand: &Option<String>) -> Option<String> {
    match (present, cand) {
        (Some(present), Some(cand)) => Some(format!("{}\n{}", present, cand)),
        (None, Some(cand)) => Some(cand.clone()),
        (_, None) => None,
    }
}

fn combine_nameable_vectors<T: Nameable + PartialEq + Clone>(
    present: &Vec<T>,
    cand: &Vec<T>,
) -> Result<Option<Vec<T>>, GreemBuildError> {
    if cand.len() == 0 {
        return Ok(None);
    }

    let mut combined = HashMap::new();

    for item in present.iter().chain(cand.iter()) {
        match combined.get(item.name()) {
            Some(present_item) => {
                if present_item != item {
                    return Err(GreemBuildError::NameCollision(
                        item.name().clone(),
                        NameCollisionReason::DifferentContent,
                    ));
                }
            }
            None => {
                combined.insert(item.name().clone(), item.clone());
            }
        }
    }

    Ok(Some(combined.into_values().collect()))
}

trait Nameable {
    fn name(&self) -> &String;
}

impl Nameable for Directive<'_, String> {
    fn name(&self) -> &String {
        &self.name
    }
}

impl Nameable for Field<'_, String> {
    fn name(&self) -> &String {
        &self.name
    }
}
