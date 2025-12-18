use std::collections::BTreeSet;

use indexmap::IndexMap;

use crate::parse::{Document, RefOrSchema, Schema, SchemaRefPath};

/// Yields all the fields of this schema, including inherited fields.
pub fn all_fields<'a>(
    doc: &'a Document,
    schema: &'a Schema,
) -> impl Iterator<Item = (&'a str, IrSchemaField<'a>)> {
    let ancestors = Vec::from_iter(Ancestors::new(doc, schema));

    let discriminators: BTreeSet<_> = ancestors
        .iter()
        .filter_map(|ancestor| ancestor.discriminator.as_ref())
        .map(|d| d.property_name.as_str())
        .collect();

    let own: IndexMap<_, _> = schema
        .properties
        .iter()
        .flatten()
        .map(|(name, property)| {
            (
                name.as_str(),
                IrSchemaField::Own(IrSchemaFieldInfo {
                    schema: property,
                    required: schema.required.contains(name),
                    discriminator: discriminators.contains(name.as_str()),
                }),
            )
        })
        .collect();

    let mut inherited = IndexMap::new();
    for ancestor in ancestors {
        let properties = ancestor
            .properties
            .iter()
            .flatten()
            .filter(|(name, _)| !own.contains_key(name.as_str()));

        for (name, property) in properties {
            inherited.entry(name.as_str()).or_insert_with(|| {
                IrSchemaField::Inherited(IrSchemaFieldInfo {
                    schema: property,
                    required: ancestor.required.contains(name),
                    discriminator: discriminators.contains(name.as_str()),
                })
            });
        }
    }

    itertools::chain!(inherited, own)
}

#[derive(Clone, Copy, Debug)]
pub enum IrSchemaField<'a> {
    Inherited(IrSchemaFieldInfo<'a>),
    Own(IrSchemaFieldInfo<'a>),
}

impl<'a> IrSchemaField<'a> {
    #[inline]
    pub fn info(self) -> IrSchemaFieldInfo<'a> {
        let (Self::Inherited(info) | Self::Own(info)) = self;
        info
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IrSchemaFieldInfo<'a> {
    pub schema: &'a RefOrSchema,
    pub required: bool,
    pub discriminator: bool,
}

/// Yields all ancestors of a schema in linear order.
pub struct Ancestors<'a> {
    doc: &'a Document,
    stack: Vec<&'a RefOrSchema>,
    visited: BTreeSet<&'a SchemaRefPath>,
}

impl<'a> Ancestors<'a> {
    #[inline]
    pub fn new(doc: &'a Document, schema: &'a Schema) -> Self {
        let stack = match &schema.all_of {
            // Push parents in reverse, so that the iterator will pop and
            // visit them in left-to-right order.
            Some(all_of) => all_of.iter().rev().collect(),
            None => vec![],
        };
        Self {
            doc,
            stack,
            visited: BTreeSet::new(),
        }
    }
}

impl<'a> Iterator for Ancestors<'a> {
    type Item = &'a Schema;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(item) = self.stack.pop() {
            let schema = match item {
                RefOrSchema::Ref(r) => {
                    if !self.visited.insert(&r.path) {
                        // Skip cycles.
                        continue;
                    }
                    let Some(schema) = self
                        .doc
                        .components
                        .as_ref()
                        .and_then(|c| c.schemas.get(r.path.as_str()))
                    else {
                        continue;
                    };
                    schema
                }
                RefOrSchema::Schema(schema) => schema.as_ref(),
            };
            if let Some(all_of) = &schema.all_of {
                self.stack.extend(all_of.iter().rev());
            }
            return Some(schema);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Components, RefOrSchema, Schema, Ty};
    use indexmap::IndexMap;
    use itertools::Itertools;

    fn make_string_field() -> RefOrSchema {
        RefOrSchema::Schema(Box::new(Schema {
            ty: vec![Ty::String],
            ..Default::default()
        }))
    }

    fn make_int_field() -> RefOrSchema {
        RefOrSchema::Schema(Box::new(Schema {
            ty: vec![Ty::Integer],
            ..Default::default()
        }))
    }

    fn make_doc(schemas: IndexMap<String, Schema>) -> Document {
        Document {
            openapi: "3.0.0".to_string(),
            info: crate::parse::Info {
                title: "Test".to_string(),
                version: "1.0".to_string(),
                description: None,
            },
            paths: IndexMap::new(),
            components: Some(Components { schemas }),
        }
    }

    fn make_ref(name: &str) -> crate::parse::Ref {
        crate::parse::Ref {
            path: format!("#/components/schemas/{name}").parse().unwrap(),
        }
    }

    #[test]
    fn test_multi_level_inheritance() {
        // Entity -> NamedEntity -> User chain
        let mut schemas = IndexMap::new();

        let mut entity_props = IndexMap::new();
        entity_props.insert("id".to_string(), make_string_field());
        schemas.insert(
            "Entity".to_string(),
            Schema {
                properties: Some(entity_props),
                ..Default::default()
            },
        );

        let mut named_entity_props = IndexMap::new();
        named_entity_props.insert("name".to_string(), make_string_field());
        schemas.insert(
            "NamedEntity".to_string(),
            Schema {
                all_of: Some(vec![RefOrSchema::Ref(make_ref("Entity"))]),
                properties: Some(named_entity_props),
                ..Default::default()
            },
        );

        let mut user_props = IndexMap::new();
        user_props.insert("email".to_string(), make_string_field());
        schemas.insert(
            "User".to_string(),
            Schema {
                all_of: Some(vec![RefOrSchema::Ref(make_ref("NamedEntity"))]),
                properties: Some(user_props),
                ..Default::default()
            },
        );

        let doc = make_doc(schemas);
        let user_schema = doc
            .components
            .as_ref()
            .unwrap()
            .schemas
            .get("User")
            .unwrap();
        let all_fields = all_fields(&doc, user_schema).collect_vec();

        // Should have 3 fields total: id (inherited), name (inherited), email (own)
        assert_eq!(all_fields.len(), 3);

        // Check inherited fields
        let inherited: Vec<_> = all_fields
            .iter()
            .filter_map(|(name, f)| match f {
                IrSchemaField::Inherited(info) => Some((name, info)),
                _ => None,
            })
            .collect();
        assert_eq!(inherited.len(), 2);
        let inherited_names: Vec<&str> = inherited.iter().map(|(n, _)| **n).collect();
        assert!(inherited_names.contains(&"id"));
        assert!(inherited_names.contains(&"name"));

        // Check own fields
        let own: Vec<_> = all_fields
            .iter()
            .filter_map(|(name, f)| match f {
                IrSchemaField::Own(info) => Some((name, info)),
                _ => None,
            })
            .collect();
        assert_eq!(own.len(), 1);
        assert_eq!(*own[0].0, "email");
    }

    #[test]
    fn test_diamond_inheritance_no_duplicate() {
        // Product -> [NamedEntity, Entity], NamedEntity -> Entity
        // Entity should only appear once
        let mut schemas = IndexMap::new();

        let mut entity_props = IndexMap::new();
        entity_props.insert("id".to_string(), make_string_field());
        schemas.insert(
            "Entity".to_string(),
            Schema {
                properties: Some(entity_props),
                ..Default::default()
            },
        );

        let mut named_entity_props = IndexMap::new();
        named_entity_props.insert("name".to_string(), make_string_field());
        schemas.insert(
            "NamedEntity".to_string(),
            Schema {
                all_of: Some(vec![RefOrSchema::Ref(make_ref("Entity"))]),
                properties: Some(named_entity_props),
                ..Default::default()
            },
        );

        let mut product_props = IndexMap::new();
        product_props.insert("price".to_string(), make_int_field());
        schemas.insert(
            "Product".to_string(),
            Schema {
                all_of: Some(vec![
                    RefOrSchema::Ref(make_ref("NamedEntity")),
                    RefOrSchema::Ref(make_ref("Entity")),
                ]),
                properties: Some(product_props),
                ..Default::default()
            },
        );

        let doc = make_doc(schemas);
        let product = doc
            .components
            .as_ref()
            .unwrap()
            .schemas
            .get("Product")
            .unwrap();
        let all_fields = all_fields(&doc, product).collect_vec();

        // Should have 3 fields total: id (inherited), name (inherited), price (own)
        assert_eq!(all_fields.len(), 3);

        // Should inherit: name, id (Entity only once)
        let inherited: Vec<_> = all_fields
            .iter()
            .filter_map(|(name, f)| match f {
                IrSchemaField::Inherited(info) => Some((name, info)),
                _ => None,
            })
            .collect();
        assert_eq!(inherited.len(), 2);
        let inherited_names: Vec<&str> = inherited.iter().map(|(n, _)| **n).collect();
        assert!(inherited_names.contains(&"id"));
        assert!(inherited_names.contains(&"name"));

        // Own: price
        let own: Vec<_> = all_fields
            .iter()
            .filter_map(|(name, f)| match f {
                IrSchemaField::Own(info) => Some((name, info)),
                _ => None,
            })
            .collect();
        assert_eq!(own.len(), 1);
        assert_eq!(*own[0].0, "price");
    }
}
