use indexmap::IndexMap;
use itertools::Itertools;
use ploidy_pointer::JsonPointee;
use rustc_hash::FxHashSet;

use crate::parse::{RefOrSchema, Schema};

use super::transform::TransformContext;

/// Returns all the fields of this schema, including inherited fields.
///
/// The `skip_refs` parameter contains references to skip when traversing
/// `allOf` chains. This is used to break cycles when a schema's field
/// contains an inline schema that references back to an ancestor.
pub fn all_fields<'a>(
    context: &TransformContext<'a>,
    schema: &'a Schema,
) -> (TransformContext<'a>, Vec<(&'a str, IrSchemaField<'a>)>) {
    let (context, ancestors) = collect_ancestors(context, schema);

    // Now, determine which fields are own, and which are inherited.
    // Discriminators can be inherited, too, and can be duplicated
    // in both `properties` and `discriminator`.

    let discriminators: FxHashSet<_> = std::iter::once(schema)
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

    let fields = itertools::chain!(inherited, own).collect();
    (context, fields)
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

/// Collects all ancestors of a schema by traversing their `allOf` references.
///
/// Returns (new context, ancestors), where the new context has the
/// updated set of references followed during traversal; and the list of ancestors
/// contains reached schemas in linearized order.
fn collect_ancestors<'a>(
    context: &TransformContext<'a>,
    schema: &'a Schema,
) -> (TransformContext<'a>, Vec<&'a Schema>) {
    let mut ancestors = Vec::new();
    let mut stack = schema.all_of.iter().flatten().rev().collect_vec();
    let mut visited = FxHashSet::default();

    while let Some(item) = stack.pop() {
        let schema = match item {
            RefOrSchema::Ref(r) => {
                if context.skip_refs.contains(&r.path) {
                    // Reference is being processed by a transform up the stack;
                    // skip to break the cycle.
                    continue;
                }
                if !visited.insert(&r.path) {
                    // Reference already visited during this linearization;
                    // skip to break the cycle.
                    continue;
                }
                let Some(schema) = context
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
            stack.extend(all_of.iter().rev());
        }
        ancestors.push(schema);
    }

    (context.with_followed(&visited), ancestors)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{parse::Document, tests::assert_matches};

    // MARK: Inheritance

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

        let schema = &doc.components.as_ref().unwrap().schemas["User"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        assert_matches!(
            &*fields,
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

        let schema = &doc.components.as_ref().unwrap().schemas["Product"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        assert_matches!(
            &*fields,
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

        let schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        assert_matches!(
            &*fields,
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

        let schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        assert_matches!(&*fields, [("name", IrSchemaField::Own(_))]);
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

        let schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        assert_matches!(&*fields, [("child_field", IrSchemaField::Own(_))]);
    }

    #[test]
    fn test_inline_all_of_handling() {
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

        let schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        assert_matches!(
            &*fields,
            [
                ("inline_field", IrSchemaField::Inherited(_)),
                ("child_field", IrSchemaField::Own(_))
            ]
        );
    }

    // MARK: Required fields

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

        let schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        // Check inherited required field.
        let id_field = fields
            .iter()
            .find(|(n, _)| *n == "id")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(id_field.required);

        // Check inherited optional field.
        let optional_field = fields
            .iter()
            .find(|(n, _)| *n == "optional_field")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(!optional_field.required);

        // Check own required field.
        let child_required = fields
            .iter()
            .find(|(n, _)| *n == "child_required")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(child_required.required);
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

        let schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        let own_required = fields
            .iter()
            .find(|(n, _)| *n == "own_required")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(own_required.required);

        let own_optional = fields
            .iter()
            .find(|(n, _)| *n == "own_optional")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(!own_optional.required);
    }

    // MARK: Discriminators

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

        let schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        // The `type` field should be marked as the discriminator.
        let type_field = fields
            .iter()
            .find(|(n, _)| *n == "type")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(type_field.discriminator);

        // The `name` field should not be marked as a discriminator.
        let name_field = fields
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

        let schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        // Both discriminator fields should be marked.
        let type1_field = fields
            .iter()
            .find(|(n, _)| *n == "type1")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(type1_field.discriminator);

        let type2_field = fields
            .iter()
            .find(|(n, _)| *n == "type2")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(type2_field.discriminator);

        // The `name` field should not be marked as a discriminator.
        let name_field = fields
            .iter()
            .find(|(n, _)| *n == "name")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(!name_field.discriminator);
    }

    #[test]
    fn test_own_discriminator_field() {
        // The schema's own discriminator should also be detected.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Schema:
                  properties:
                    kind:
                      type: string
                    name:
                      type: string
                  discriminator:
                    propertyName: kind
        "})
        .unwrap();

        let schema = &doc.components.as_ref().unwrap().schemas["Schema"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        // The `kind` field should be marked as the discriminator.
        let kind_field = fields
            .iter()
            .find(|(n, _)| *n == "kind")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(kind_field.discriminator);

        // The `name` field should not be marked as a discriminator.
        let name_field = fields
            .iter()
            .find(|(n, _)| *n == "name")
            .map(|(_, f)| f.info())
            .unwrap();
        assert!(!name_field.discriminator);
    }

    // MARK: Cycle detection

    #[test]
    fn test_skip_refs_breaks_cycle() {
        // When a reference is in `skip_refs`, it should be skipped
        // to avoid infinite recursion.
        let doc = Document::from_yaml(indoc::indoc! {"
            openapi: 3.0.0
            info:
              title: Test
              version: 1.0.0
            components:
              schemas:
                Node:
                  allOf:
                    - $ref: '#/components/schemas/Node'
                  properties:
                    value:
                      type: string
        "})
        .unwrap();

        let schema = &doc.components.as_ref().unwrap().schemas["Node"];
        let (_, fields) = all_fields(&TransformContext::new(&doc), schema);

        // The self-reference should be skipped; only own fields remain.
        assert_matches!(&*fields, [("value", IrSchemaField::Own(_))]);
    }

    #[test]
    fn test_context_updated_with_followed_refs() {
        // The returned context should include the references that were followed.
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
                      type: string
        "})
        .unwrap();

        let schema = &doc.components.as_ref().unwrap().schemas["Child"];
        let (new_context, _) = all_fields(&TransformContext::new(&doc), schema);

        // The context should now include the `Parent` reference.
        assert_eq!(new_context.skip_refs.len(), 1);
    }
}
