use std::collections::BTreeSet;

use indexmap::IndexMap;
use itertools::Itertools;
use ploidy_pointer::JsonPointee;

use crate::parse::{ComponentRef, Document, RefOrSchema, Schema};

/// Returns an iterator over all the fields of this schema,
/// including inherited fields.
pub fn all_fields<'a>(
    doc: &'a Document,
    schema: &'a Schema,
) -> impl Iterator<Item = (&'a str, IrSchemaField<'a>)> {
    let ancestors = Ancestors::new(doc, schema).collect_vec();

    let discriminators: BTreeSet<_> = std::iter::once(schema)
        .chain(ancestors.iter().copied())
        .filter_map(|s| s.discriminator.as_ref())
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
                    flattened: false,
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
                    flattened: false,
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
    pub flattened: bool,
}

/// An iterator over all the ancestors of a schema, in linear order.
pub struct Ancestors<'a> {
    doc: &'a Document,
    stack: Vec<&'a RefOrSchema>,
    visited: BTreeSet<&'a ComponentRef>,
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
                        .resolve(r.path.pointer().clone())
                        .ok()
                        .and_then(|p| p.downcast_ref::<Schema>())
                    else {
                        continue;
                    };
                    schema
                }
                RefOrSchema::Other(schema) => schema.as_ref(),
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

    use itertools::Itertools;

    use crate::tests::assert_matches;

    #[test]
    fn test_multi_level_inheritance() {
        // Entity -> NamedEntity -> User chain.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Entity:
                  properties:
                    id:
                      type: string
                NamedEntity:
                  allOf:
                    - $ref: '#/components/schemas/Entity'
                  properties:
                    name:
                      type: string
                User:
                  allOf:
                    - $ref: '#/components/schemas/NamedEntity'
                  properties:
                    email:
                      type: string
        "})
        .unwrap();

        let user_schema = &doc.components.as_ref().unwrap().schemas["User"];
        let all_fields = all_fields(&doc, user_schema).collect_vec();

        assert_matches!(
            &*all_fields,
            [
                ("name", IrSchemaField::Inherited(_)),
                ("id", IrSchemaField::Inherited(_)),
                ("email", IrSchemaField::Own(_))
            ]
        );
    }

    #[test]
    fn test_diamond_inheritance_no_duplicate() {
        // Product -> [NamedEntity, Entity], NamedEntity -> Entity;
        // `Entity` should only appear once.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Entity:
                  properties:
                    id:
                      type: string
                NamedEntity:
                  allOf:
                    - $ref: '#/components/schemas/Entity'
                  properties:
                    name:
                      type: string
                Product:
                  allOf:
                    - $ref: '#/components/schemas/NamedEntity'
                    - $ref: '#/components/schemas/Entity'
                  properties:
                    price:
                      type: integer
        "})
        .unwrap();

        let product_schema = &doc.components.as_ref().unwrap().schemas["Product"];
        let all_fields = all_fields(&doc, product_schema).collect_vec();

        assert_matches!(
            &*all_fields,
            [
                ("name", IrSchemaField::Inherited(_)),
                ("id", IrSchemaField::Inherited(_)),
                ("price", IrSchemaField::Own(_))
            ]
        );
    }

    #[test]
    fn test_single_parent_inheritance() {
        // Simple case: Child -> Parent with `allOf`.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Parent:
                  properties:
                    parent_field:
                      type: string
                Child:
                  allOf:
                    - $ref: '#/components/schemas/Parent'
                  properties:
                    child_field:
                      type: integer
        "})
        .unwrap();

        let child_schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let all_fields = all_fields(&doc, child_schema).collect_vec();

        assert_matches!(
            &*all_fields,
            [
                ("parent_field", IrSchemaField::Inherited(_)),
                ("child_field", IrSchemaField::Own(_))
            ]
        );
    }

    #[test]
    fn test_field_override_in_child_schema() {
        // Child redefines a field from parent; the overridden field
        // should be "own", not "inherited".
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Parent:
                  properties:
                    name:
                      type: string
                Child:
                  allOf:
                    - $ref: '#/components/schemas/Parent'
                  properties:
                    name:
                      type: integer
        "})
        .unwrap();

        let child_schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let all_fields = all_fields(&doc, child_schema).collect_vec();

        assert_matches!(&*all_fields, [("name", IrSchemaField::Own(_))]);
    }

    #[test]
    fn test_required_field_inheritance() {
        // Fields marked as required in the parent maintain their
        // required status when inherited.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Parent:
                  properties:
                    id:
                      type: string
                    optional_field:
                      type: string
                  required:
                    - id
                Child:
                  allOf:
                    - $ref: '#/components/schemas/Parent'
                  properties:
                    child_required:
                      type: string
                  required:
                    - child_required
        "})
        .unwrap();

        let child_schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let all_fields = all_fields(&doc, child_schema).collect_vec();

        // Check inherited required field.
        let id_field = all_fields
            .iter()
            .find(|(n, _)| *n == "id")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(id_field.required);

        // Check inherited optional field.
        let optional_field = all_fields
            .iter()
            .find(|(n, _)| *n == "optional_field")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(!optional_field.required);

        // Check own required field.
        let child_required = all_fields
            .iter()
            .find(|(n, _)| *n == "child_required")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(child_required.required);
    }

    #[test]
    fn test_empty_parent_schema_handling() {
        // Inheriting from a parent with no properties should work correctly.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                EmptyParent:
                  type: object
                Child:
                  allOf:
                    - $ref: '#/components/schemas/EmptyParent'
                  properties:
                    child_field:
                      type: string
        "})
        .unwrap();

        let child_schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let all_fields = all_fields(&doc, child_schema).collect_vec();

        assert_matches!(&*all_fields, [("child_field", IrSchemaField::Own(_))]);
    }

    #[test]
    fn test_inline_allof_handling() {
        // `allOf` with inline schemas should be processed correctly.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Child:
                  allOf:
                    - properties:
                        inline_field:
                          type: string
                  properties:
                    child_field:
                      type: integer
        "})
        .unwrap();

        let child_schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let all_fields = all_fields(&doc, child_schema).collect_vec();

        assert_matches!(
            &*all_fields,
            [
                ("inline_field", IrSchemaField::Inherited(_)),
                ("child_field", IrSchemaField::Own(_))
            ]
        );
    }

    #[test]
    fn test_own_field_required_status() {
        // Own fields respect the local `required` array,
        // independent of the parent.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Parent:
                  properties:
                    parent_field:
                      type: string
                  required:
                    - parent_field
                Child:
                  allOf:
                    - $ref: '#/components/schemas/Parent'
                  properties:
                    own_required:
                      type: string
                    own_optional:
                      type: string
                  required:
                    - own_required
        "})
        .unwrap();

        let child_schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let all_fields = all_fields(&doc, child_schema).collect_vec();

        let own_required = all_fields
            .iter()
            .find(|(n, _)| *n == "own_required")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(own_required.required);

        let own_optional = all_fields
            .iter()
            .find(|(n, _)| *n == "own_optional")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(!own_optional.required);
    }

    #[test]
    fn test_discriminator_field_detection() {
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Parent:
                  properties:
                    type:
                      type: string
                    name:
                      type: string
                  discriminator:
                    propertyName: type
                Child:
                  allOf:
                    - $ref: '#/components/schemas/Parent'
                  properties:
                    child_field:
                      type: string
        "})
        .unwrap();

        let child_schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let all_fields = all_fields(&doc, child_schema).collect_vec();

        // The `type` field should be marked as the discriminator.
        let type_field = all_fields
            .iter()
            .find(|(n, _)| *n == "type")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(type_field.discriminator);

        // The `name` field should not be marked as a discriminator.
        let name_field = all_fields
            .iter()
            .find(|(n, _)| *n == "name")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(!name_field.discriminator);
    }

    #[test]
    fn test_multiple_discriminators() {
        // Schemas with multiple discriminators from different parents.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Parent1:
                  properties:
                    type1:
                      type: string
                  discriminator:
                    propertyName: type1
                Parent2:
                  properties:
                    type2:
                      type: string
                  discriminator:
                    propertyName: type2
                Child:
                  allOf:
                    - $ref: '#/components/schemas/Parent1'
                    - $ref: '#/components/schemas/Parent2'
                  properties:
                    name:
                      type: string
        "})
        .unwrap();

        let child_schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let all_fields = all_fields(&doc, child_schema).collect_vec();

        // Both discriminator fields should be marked.
        let type1_field = all_fields
            .iter()
            .find(|(n, _)| *n == "type1")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(type1_field.discriminator);

        let type2_field = all_fields
            .iter()
            .find(|(n, _)| *n == "type2")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(type2_field.discriminator);

        // The `name` field should not be marked as a discriminator.
        let name_field = all_fields
            .iter()
            .find(|(n, _)| *n == "name")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(!name_field.discriminator);
    }
}
