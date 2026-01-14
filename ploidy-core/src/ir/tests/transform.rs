//! IR transformation tests.

use serde_json::Number;

use crate::{
    ir::{
        InlineIrType, InlineIrTypePathRoot, InlineIrTypePathSegment, IrEnumVariant, IrStructField,
        IrStructFieldName, IrStructFieldNameHint, IrTaggedVariant, IrType, IrTypeName,
        IrUntaggedVariant, IrUntaggedVariantNameHint, PrimitiveIrType, SchemaIrType,
        transform::transform,
    },
    parse::{Document, Schema},
    tests::assert_matches,
};

// MARK: Enums

#[test]
fn test_enum_string_variants() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        enum: [active, inactive, pending]
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Status"), &schema);

    let enum_ = match result {
        IrType::Schema(SchemaIrType::Enum("Status", enum_)) => enum_,
        other => panic!("expected enum `Status`; got `{other:?}`"),
    };
    assert_matches!(
        &*enum_.variants,
        [
            IrEnumVariant::String("active"),
            IrEnumVariant::String("inactive"),
            IrEnumVariant::String("pending"),
        ],
    );
}

#[test]
fn test_enum_number_variants() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        enum: [1, 2, 3]
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Priority"), &schema);

    let enum_ = match result {
        IrType::Schema(SchemaIrType::Enum("Priority", enum_)) => enum_,
        other => panic!("expected enum `Priority`; got `{other:?}`"),
    };
    assert_matches!(
        &*enum_.variants,
        [
            IrEnumVariant::Number(n1),
            IrEnumVariant::Number(n2),
            IrEnumVariant::Number(n3),
        ] if n1 == &Number::from(1) && n2 == &Number::from(2) && n3 == &Number::from(3),
    );
}

#[test]
fn test_enum_bool_variants() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        enum: [true, false]
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Flag"), &schema);

    let enum_ = match result {
        IrType::Schema(SchemaIrType::Enum("Flag", enum_)) => enum_,
        other => panic!("expected enum `Flag`; got `{other:?}`"),
    };
    assert_matches!(
        &*enum_.variants,
        [IrEnumVariant::Bool(true), IrEnumVariant::Bool(false)],
    );
}

#[test]
fn test_enum_mixed_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        enum: [text, 42, true]
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Mixed"), &schema);

    let enum_ = match result {
        IrType::Schema(SchemaIrType::Enum("Mixed", enum_)) => enum_,
        other => panic!("expected enum `Mixed`; got `{other:?}`"),
    };
    assert_matches!(
        &*enum_.variants,
        [
            IrEnumVariant::String("text"),
            IrEnumVariant::Number(n),
            IrEnumVariant::Bool(true),
        ] if n == &Number::from(42),
    );
}

// MARK: Primitives

#[test]
fn test_primitive_string_formats() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();

    // `string` with `date-time` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: date-time
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("Timestamp"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::DateTime));

    // `string` with `date` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: date
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("Date"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Date));

    // `string` with `uri` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: uri
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("Url"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Url));

    // `string` with `uuid` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: uuid
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("Id"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Uuid));

    // `string` with `byte` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: byte
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("Data"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Bytes));

    // `string` without format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("Text"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::String));
}

#[test]
fn test_primitive_integer_formats() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();

    // `integer` with `int32` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
        format: int32
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("Count"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::I32));

    // `integer` with `int64` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
        format: int64
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("BigCount"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::I64));

    // `integer` with `unix-time` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
        format: unix-time
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("Timestamp"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::DateTime));

    // `integer` without format defaults to `int32`.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("DefaultInt"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::I32));
}

#[test]
fn test_primitive_number_formats() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();

    // `number` with `float` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
        format: float
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("Price"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::F32));

    // `number` with `double` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
        format: double
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("BigPrice"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::F64));

    // `number` with `unix-time` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
        format: unix-time
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("FloatTime"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::DateTime));

    // `number` without format defaults to `double`.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
    "})
    .unwrap();
    let result = transform(&doc, IrTypeName::Schema("DefaultNumber"), &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::F64));
}

// MARK: Arrays

#[test]
fn test_array_with_ref_items() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Item:
              type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: array
        items:
          $ref: '#/components/schemas/Item'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Items"), &schema);

    let items = match result {
        IrType::Array(items) => items,
        other => panic!("expected array; got `{other:?}`"),
    };
    assert_matches!(*items, IrType::Ref(_));
}

#[test]
fn test_array_with_inline_items() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: array
        items:
          type: string
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Strings"), &schema);

    let items = match result {
        IrType::Array(items) => items,
        other => panic!("expected array; got `{other:?}`"),
    };
    assert_matches!(*items, IrType::Primitive(PrimitiveIrType::String));
}

// MARK: Maps

#[test]
fn test_map_with_additional_properties_ref() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Value:
              type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        additionalProperties:
          $ref: '#/components/schemas/Value'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("StringMap"), &schema);

    let value = match result {
        IrType::Map(value) => value,
        other => panic!("expected map; got `{other:?}`"),
    };
    assert_matches!(*value, IrType::Ref(_));
}

#[test]
fn test_map_with_additional_properties_inline() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        additionalProperties:
          type: integer
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("IntMap"), &schema);

    let value = match result {
        IrType::Map(value) => value,
        other => panic!("expected map; got `{other:?}`"),
    };
    assert_matches!(*value, IrType::Primitive(PrimitiveIrType::I32));
}

#[test]
fn test_map_with_additional_properties_true() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties: {}
        additionalProperties: true
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("DynamicMap"), &schema);

    let value = match result {
        IrType::Map(value) => value,
        other => panic!("expected map; got `{other:?}`"),
    };
    assert_matches!(*value, IrType::Any);
}

// MARK: `try_struct()`

#[test]
fn test_struct_with_own_properties() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          name:
            type: string
          age:
            type: integer
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Person"), &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Person", struct_)) => struct_,
        other => panic!("expected struct `Person`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Name("name"),
                ty: IrType::Primitive(PrimitiveIrType::String),
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("age"),
                ty: IrType::Primitive(PrimitiveIrType::I32),
                ..
            },
        ],
    );
}

#[test]
fn test_struct_with_additional_properties() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          name:
            type: string
        additionalProperties:
          type: string
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("DynamicObj"), &schema);

    // When `additionalProperties` is present, it should fall through
    // to `other()`, which creates a `Map`.
    assert_matches!(result, IrType::Map(_));
}

#[test]
fn test_struct_with_required_fields() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          name:
            type: string
          email:
            type: string
        required:
          - name
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("User"), &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("User", struct_)) => struct_,
        other => panic!("expected struct `User`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Name("name"),
                required: true,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("email"),
                required: false,
                ..
            },
        ],
    );
}

#[test]
fn test_struct_with_nullable_field_ref() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            NullableString:
              type: string
              nullable: true
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          value:
            $ref: '#/components/schemas/NullableString'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Container"), &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Container", struct_)) => struct_,
        other => panic!("expected struct `Container`; got `{other:?}`"),
    };
    let [
        IrStructField {
            name: IrStructFieldName::Name("value"),
            ty: IrType::Nullable(inner),
            ..
        },
    ] = &*struct_.fields
    else {
        panic!("expected single nullable field; got `{:?}`", struct_.fields);
    };
    assert_matches!(&**inner, IrType::Ref(_));
}

#[test]
fn test_struct_with_nullable_field_inline() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          value:
            type: string
            nullable: true
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Container"), &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Container", struct_)) => struct_,
        other => panic!("expected struct `Container`; got `{other:?}`"),
    };
    let [
        IrStructField {
            name: IrStructFieldName::Name("value"),
            ty: IrType::Nullable(inner),
            ..
        },
    ] = &*struct_.fields
    else {
        panic!("expected single nullable field; got `{:?}`", struct_.fields);
    };
    assert_matches!(&**inner, IrType::Primitive(PrimitiveIrType::String));
}

#[test]
fn test_struct_ref_field_description() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Id:
              type: string
              description: An identifier
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          id:
            $ref: '#/components/schemas/Id'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Entity"), &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Entity", struct_)) => struct_,
        other => panic!("expected struct `Entity`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [IrStructField {
            name: IrStructFieldName::Name("id"),
            description: Some("An identifier"),
            ..
        }],
    );
}

#[test]
fn test_struct_inline_field_description() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          name:
            type: string
            description: A user's name
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("User"), &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("User", struct_)) => struct_,
        other => panic!("expected struct `User`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [IrStructField {
            name: IrStructFieldName::Name("name"),
            description: Some("A user's name"),
            ..
        }],
    );
}

// MARK: `try_tagged()`

#[test]
fn test_tagged_with_mapping() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Dog:
              type: object
              properties:
                bark:
                  type: string
            Cat:
              type: object
              properties:
                meow:
                  type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf:
          - $ref: '#/components/schemas/Dog'
          - $ref: '#/components/schemas/Cat'
        discriminator:
          propertyName: type
          mapping:
            dog: '#/components/schemas/Dog'
            cat: '#/components/schemas/Cat'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Animal"), &schema);

    let tagged = match result {
        IrType::Schema(SchemaIrType::Tagged("Animal", tagged)) => tagged,
        other => panic!("expected tagged union `Animal`; got `{other:?}`"),
    };
    assert_eq!(tagged.tag, "type");
    let [
        dog_variant @ IrTaggedVariant { name: "Dog", .. },
        cat_variant @ IrTaggedVariant { name: "Cat", .. },
    ] = &*tagged.variants
    else {
        panic!(
            "expected and `Cat` variants `Dog`; got `{:?}`",
            tagged.variants,
        );
    };
    assert_matches!(&*dog_variant.aliases, ["dog"]);
    assert_eq!(&*cat_variant.aliases, ["cat"]);
}

#[test]
fn test_tagged_filters_non_refs() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Dog:
              type: object
              properties:
                bark:
                  type: string
    "})
    .unwrap();
    // Include both a reference and an inline schema.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf:
          - $ref: '#/components/schemas/Dog'
          - type: object
            properties:
              inline:
                type: string
        discriminator:
          propertyName: type
          mapping:
            dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Animal"), &schema);

    // Should only have the `Dog` variant; inline schema is filtered out.
    let tagged = match result {
        IrType::Schema(SchemaIrType::Tagged("Animal", tagged)) => tagged,
        other => panic!("expected tagged union `Animal`; got `{other:?}`"),
    };
    assert_matches!(&*tagged.variants, [IrTaggedVariant { name: "Dog", .. }]);
}

#[test]
fn test_tagged_multiple_aliases() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Success:
              type: object
              properties:
                data:
                  type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf:
          - $ref: '#/components/schemas/Success'
        discriminator:
          propertyName: status
          mapping:
            good: '#/components/schemas/Success'
            ok: '#/components/schemas/Success'
            success: '#/components/schemas/Success'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Result"), &schema);

    let tagged = match result {
        IrType::Schema(SchemaIrType::Tagged("Result", tagged)) => tagged,
        other => panic!("expected tagged union `Result`; got `{other:?}`"),
    };
    let [
        IrTaggedVariant {
            name: "Success",
            aliases,
            ..
        },
    ] = &*tagged.variants
    else {
        panic!("expected variant `Success`; got `{:?}`", tagged.variants);
    };
    assert_matches!(&**aliases, ["good", "ok", "success"]);
}

#[test]
fn test_tagged_missing_variant() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Dog:
              type: object
              properties:
                bark:
                  type: string
            Cat:
              type: object
              properties:
                meow:
                  type: string
    "})
    .unwrap();
    // Only `Dog` is in the mapping; `Cat` isn't.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf:
          - $ref: '#/components/schemas/Dog'
          - $ref: '#/components/schemas/Cat'
        discriminator:
          propertyName: type
          mapping:
            dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Animal"), &schema);

    // Only `Dog` should be included, since `Cat` isn't in the mapping.
    let tagged = match result {
        IrType::Schema(SchemaIrType::Tagged("Animal", tagged)) => tagged,
        other => panic!("expected tagged union `Animal`; got `{other:?}`"),
    };
    assert_matches!(&*tagged.variants, [IrTaggedVariant { name: "Dog", .. }]);
}

#[test]
fn test_tagged_description() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Dog:
              type: object
              properties:
                bark:
                  type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        description: A tagged union of animals
        oneOf:
          - $ref: '#/components/schemas/Dog'
        discriminator:
          propertyName: type
          mapping:
            dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Animal"), &schema);

    let tagged = match result {
        IrType::Schema(SchemaIrType::Tagged("Animal", tagged)) => tagged,
        other => panic!("expected tagged union `Animal`; got `{other:?}`"),
    };
    assert_eq!(tagged.description, Some("A tagged union of animals"));
    assert_matches!(
        &*tagged.variants,
        [IrTaggedVariant { name: "Dog", aliases, .. }] if aliases == &["dog"],
    );
}

// MARK: `try_untagged()`

#[test]
fn test_untagged_basic() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            String:
              type: string
            Number:
              type: integer
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf:
          - $ref: '#/components/schemas/String'
          - $ref: '#/components/schemas/Number'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("StringOrNumber"), &schema);

    let untagged = match result {
        IrType::Schema(SchemaIrType::Untagged("StringOrNumber", untagged)) => untagged,
        other => panic!("expected untagged union `StringOrNumber`; got `{other:?}`"),
    };
    assert_matches!(
        &*untagged.variants,
        [
            IrUntaggedVariant::Some(IrUntaggedVariantNameHint::Index(1), IrType::Ref(_)),
            IrUntaggedVariant::Some(IrUntaggedVariantNameHint::Index(2), IrType::Ref(_)),
        ],
    );
}

#[test]
fn test_untagged_empty_simplifies() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf: []
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Empty"), &schema);

    assert_matches!(result, IrType::Any);
}

#[test]
fn test_untagged_single_null_simplifies() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf:
          - type: 'null'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("JustNull"), &schema);

    assert_matches!(result, IrType::Any);
}

#[test]
fn test_untagged_single_variant_unwraps() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            String:
              type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf:
          - $ref: '#/components/schemas/String'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("JustString"), &schema);

    assert_matches!(result, IrType::Ref(_));
}

#[test]
fn test_untagged_variant_numbering() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            A:
              type: string
            B:
              type: string
            C:
              type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf:
          - $ref: '#/components/schemas/A'
          - $ref: '#/components/schemas/B'
          - $ref: '#/components/schemas/C'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("ABC"), &schema);

    // Variants should have 1-based indices in their name hints.
    let untagged = match result {
        IrType::Schema(SchemaIrType::Untagged("ABC", untagged)) => untagged,
        other => panic!("expected untagged union `ABC`; got `{other:?}`"),
    };
    assert_matches!(
        &*untagged.variants,
        [
            IrUntaggedVariant::Some(IrUntaggedVariantNameHint::Index(1), _),
            IrUntaggedVariant::Some(IrUntaggedVariantNameHint::Index(2), _),
            IrUntaggedVariant::Some(IrUntaggedVariantNameHint::Index(3), _),
        ],
    );
}

#[test]
fn test_untagged_null_detection() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf:
          - type: 'null'
          - type: string
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("StringOrNull"), &schema);

    assert_matches!(result, IrType::Nullable(_));
}

// MARK: `try_any_of()`

#[test]
fn test_any_of_fields_marked_flattened_not_required() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Address:
              type: object
              properties:
                street:
                  type: string
            Email:
              type: object
              properties:
                email:
                  type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        anyOf:
          - $ref: '#/components/schemas/Address'
          - $ref: '#/components/schemas/Email'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Contact"), &schema);

    // Both fields should be flattened.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Contact", struct_)) => struct_,
        other => panic!("expected struct `Contact`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Name("Address"),
                flattened: true,
                required: false,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("Email"),
                flattened: true,
                required: false,
                ..
            },
        ],
    );
}

#[test]
fn test_any_of_ref_uses_type_name() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Address:
              type: object
              properties:
                street:
                  type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        anyOf:
          - $ref: '#/components/schemas/Address'
          - $ref: '#/components/schemas/Address'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Contact"), &schema);

    // Both fields should be named `Address`, since they reference the same
    // type.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Contact", struct_)) => struct_,
        other => panic!("expected struct `Contact`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Name("Address"),
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("Address"),
                ..
            },
        ],
    );
}

#[test]
fn test_any_of_inline_uses_index_hint() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        anyOf:
          - type: object
            properties:
              a:
                type: string
          - type: object
            properties:
              b:
                type: string
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Mixed"), &schema);

    // Both inline schemas should have index hints.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Mixed", struct_)) => struct_,
        other => panic!("expected struct `Mixed`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Hint(IrStructFieldNameHint::Index(1)),
                flattened: true,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Hint(IrStructFieldNameHint::Index(2)),
                flattened: true,
                ..
            },
        ],
    );
}

#[test]
fn test_any_of_with_properties() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Extra1:
              type: object
              properties:
                extra1:
                  type: string
            Extra2:
              type: object
              properties:
                extra2:
                  type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        anyOf:
          - $ref: '#/components/schemas/Extra1'
          - $ref: '#/components/schemas/Extra3'
        properties:
          Extra2:
            type: string
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Combined"), &schema);

    // `Extra2` is an own field; `Extra1` and `Extra3` are flattened.
    // Own fields should precede the flattened fields.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Combined", struct_)) => struct_,
        other => panic!("expected struct `Combined`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Name("Extra2"),
                flattened: false,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("Extra1"),
                flattened: true,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("Extra3"),
                flattened: true,
                ..
            },
        ],
    );
}

#[test]
fn test_any_of_nullable_refs() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            NullableString1:
              type: string
              nullable: true
            NullableString2:
              type: string
              nullable: true
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        anyOf:
          - $ref: '#/components/schemas/NullableString1'
          - $ref: '#/components/schemas/NullableString2'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Container"), &schema);

    // Both nullable schemas should be flattened.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Container", struct_)) => struct_,
        other => panic!("expected struct `Container`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Name("NullableString1"),
                flattened: true,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("NullableString2"),
                flattened: true,
                ..
            },
        ],
    );
}

#[test]
fn test_any_of_with_allof() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Base:
              type: object
              properties:
                id:
                  type: string
            Extra1:
              type: object
              properties:
                extra1:
                  type: string
            Extra2:
              type: object
              properties:
                extra2:
                  type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        allOf:
          - $ref: '#/components/schemas/Base'
        anyOf:
          - $ref: '#/components/schemas/Extra1'
          - $ref: '#/components/schemas/Extra2'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Combined"), &schema);

    // Should have both inherited and flattened fields.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Combined", struct_)) => struct_,
        other => panic!("expected struct `Combined`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Name("id"),
                inherited: true,
                flattened: false,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("Extra1"),
                inherited: false,
                flattened: true,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("Extra2"),
                inherited: false,
                flattened: true,
                ..
            },
        ],
    );
}

// MARK: Edge cases

#[test]
fn test_boolean_primitive_transformation() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: boolean
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Flag"), &schema);

    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Bool));
}

#[test]
fn test_unhandled_string_format_falls_back_to_string() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    // Use a format that is not explicitly handled for strings.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: currency
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("CustomType"), &schema);

    assert_matches!(result, IrType::Primitive(PrimitiveIrType::String));
}

#[test]
fn test_binary_format_maps_to_bytes() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: binary
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Data"), &schema);

    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Bytes));
}

#[test]
fn test_empty_type_array_produces_any() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: []
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("NoType"), &schema);

    assert_matches!(result, IrType::Any);
}

#[test]
fn test_array_without_items_produces_array_of_any() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: array
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("ArrayAny"), &schema);

    let items = match result {
        IrType::Array(items) => items,
        other => panic!("expected array; got `{other:?}`"),
    };
    assert_matches!(*items, IrType::Any);
}

#[test]
fn test_object_with_empty_properties_produces_struct() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties: {}
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("EmptyObject"), &schema);

    // An `object` schema without properties should become an empty struct.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("EmptyObject", struct_)) => struct_,
        other => panic!("expected struct `EmptyObject`; got `{other:?}`"),
    };
    assert_matches!(&*struct_.fields, []);
}

#[test]
fn test_schema_without_type_or_properties_produces_any() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        {}
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Empty"), &schema);

    // A schema with no `type` and no `properties` should become `Any`.
    assert_matches!(result, IrType::Any);
}

#[test]
fn test_type_and_null_in_type_array_creates_nullable() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: [string, 'null']
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("StringOrNull"), &schema);

    // As a special case, `string` + `null` should produce `Nullable(String)`,
    // not an untagged union.
    let inner = match result {
        IrType::Nullable(inner) => inner,
        other => panic!("expected nullable; got `{other:?}`"),
    };
    assert_matches!(*inner, IrType::Primitive(PrimitiveIrType::String));
}

#[test]
fn test_multiple_types_string_and_integer_untagged() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: [string, integer]
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("StringOrInt"), &schema);

    let untagged = match result {
        IrType::Schema(SchemaIrType::Untagged("StringOrInt", untagged)) => untagged,
        other => panic!("expected untagged union `StringOrInt`; got `{other:?}`"),
    };
    assert_matches!(
        &*untagged.variants,
        [
            IrUntaggedVariant::Some(
                IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::String),
                IrType::Primitive(PrimitiveIrType::String),
            ),
            IrUntaggedVariant::Some(
                IrUntaggedVariantNameHint::Primitive(PrimitiveIrType::I32),
                IrType::Primitive(PrimitiveIrType::I32),
            ),
        ],
    );
}

#[test]
fn test_deeply_nested_inline_types() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          items:
            type: array
            items:
              type: object
              properties:
                field:
                  type: string
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Outer"), &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Outer", struct_)) => struct_,
        other => panic!("expected struct `Outer`; got `{other:?}`"),
    };
    let [
        IrStructField {
            name: IrStructFieldName::Name("items"),
            ty: IrType::Array(items),
            ..
        },
    ] = &*struct_.fields
    else {
        panic!("expected named array; got `{:?}`", struct_.fields);
    };
    let (path, inner_struct) = match &**items {
        IrType::Inline(InlineIrType::Struct(path, inner_struct)) => (path, inner_struct),
        other => panic!("expected inline struct; got `{other:?}`"),
    };
    // Check that the path has correct segments.
    assert_matches!(path.root, InlineIrTypePathRoot::Type("Outer"));
    assert_matches!(
        &*path.segments,
        [
            InlineIrTypePathSegment::Field(IrStructFieldName::Name("items")),
            InlineIrTypePathSegment::ArrayItem,
        ],
    );
    // Verify that the inner struct has the expected field.
    assert_matches!(
        &*inner_struct.fields,
        [IrStructField {
            name: IrStructFieldName::Name("field"),
            ty: IrType::Primitive(PrimitiveIrType::String),
            ..
        },]
    );
}

#[test]
fn test_enum_with_only_null_json_values_produces_empty_enum() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: 'null'
        enum: [null]
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("NullEnum"), &schema);

    // `null` values are filtered out from enum variants, producing an enum
    // with zero variants.
    let enum_ = match result {
        IrType::Schema(SchemaIrType::Enum("NullEnum", enum_)) => enum_,
        other => panic!("expected enum `NullEnum`; got `{other:?}`"),
    };
    assert_matches!(&*enum_.variants, []);
}

#[test]
fn test_additional_properties_false_creates_struct() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          name:
            type: string
        additionalProperties: false
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("StrictObject"), &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("StrictObject", struct_)) => struct_,
        other => panic!("expected struct `StrictObject`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [IrStructField {
            name: IrStructFieldName::Name("name"),
            ty: IrType::Primitive(PrimitiveIrType::String),
            ..
        }],
    );
}

// MARK: Inline type paths

#[test]
fn test_array_inline_path_construction() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: array
        items:
          type: object
          properties:
            field:
              type: string
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Container"), &schema);

    let items = match result {
        IrType::Array(items) => items,
        other => panic!("expected named array; got `{other:?}`"),
    };
    let path = match *items {
        IrType::Inline(InlineIrType::Struct(path, _)) => path,
        other => panic!("expected inline struct; got `{other:?}`"),
    };
    assert_matches!(path.root, InlineIrTypePathRoot::Type("Container"));
    assert_matches!(&*path.segments, [InlineIrTypePathSegment::ArrayItem]);
}

#[test]
fn test_map_inline_path_construction() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        additionalProperties:
          type: object
          properties:
            field:
              type: string
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Dictionary"), &schema);

    let value = match result {
        IrType::Map(value) => value,
        other => panic!("expected named map; got `{other:?}`"),
    };
    let path = match *value {
        IrType::Inline(InlineIrType::Struct(path, _)) => path,
        other => panic!("expected inline struct; got `{other:?}`"),
    };
    assert_matches!(path.root, InlineIrTypePathRoot::Type("Dictionary"));
    assert_matches!(&*path.segments, [InlineIrTypePathSegment::MapValue]);
}

#[test]
fn test_struct_inline_path_construction() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          nested:
            type: object
            properties:
              inner:
                type: string
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Outer"), &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Outer", struct_)) => struct_,
        other => panic!("expected struct `Outer`; got `{other:?}`"),
    };
    let [
        IrStructField {
            name: IrStructFieldName::Name("nested"),
            ty: IrType::Inline(InlineIrType::Struct(path, _)),
            ..
        },
    ] = &*struct_.fields
    else {
        panic!(
            "expected single inline struct field; got `{:?}`",
            struct_.fields,
        );
    };
    assert_matches!(path.root, InlineIrTypePathRoot::Type("Outer"));
    assert_matches!(
        &*path.segments,
        [InlineIrTypePathSegment::Field(IrStructFieldName::Name(
            "nested"
        ))],
    );
}

// MARK: Inline tagged unions

#[test]
fn test_inline_tagged_union_in_struct_field() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Cat:
              type: object
              properties:
                meow:
                  type: string
            Dog:
              type: object
              properties:
                bark:
                  type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          animal:
            oneOf:
              - $ref: '#/components/schemas/Cat'
              - $ref: '#/components/schemas/Dog'
            discriminator:
              propertyName: kind
              mapping:
                cat: '#/components/schemas/Cat'
                dog: '#/components/schemas/Dog'
    "})
    .unwrap();

    let result = transform(&doc, IrTypeName::Schema("Container"), &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct("Container", struct_)) => struct_,
        other => panic!("expected struct `Container`; got `{other:?}`"),
    };

    let [
        IrStructField {
            name: IrStructFieldName::Name("animal"),
            ty: IrType::Inline(InlineIrType::Tagged(path, tagged)),
            ..
        },
    ] = &*struct_.fields
    else {
        panic!(
            "expected single inline tagged union field; got `{:?}`",
            struct_.fields,
        );
    };

    // Verify the path.
    assert_matches!(path.root, InlineIrTypePathRoot::Type("Container"));
    assert_matches!(
        &*path.segments,
        [InlineIrTypePathSegment::Field(IrStructFieldName::Name(
            "animal"
        ))],
    );

    // Verify the tag.
    assert_eq!(tagged.tag, "kind");

    // Verify the variants.
    let [
        cat_variant @ IrTaggedVariant { name: "Cat", .. },
        dog_variant @ IrTaggedVariant { name: "Dog", .. },
    ] = &*tagged.variants
    else {
        panic!(
            "expected `Cat` and `Dog` variants; got `{:?}`",
            tagged.variants
        );
    };
    assert_eq!(&*cat_variant.aliases, ["cat"]);
    assert_eq!(&*dog_variant.aliases, ["dog"]);
}
