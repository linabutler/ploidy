//! IR transformation tests.

use crate::{
    arena::Arena,
    ir::{
        Enum, EnumVariant, InlineTypePath, InlineTypePathRoot, InlineTypePathSegment,
        PrimitiveType, SchemaTypeInfo, SpecContainer, SpecInlineType, SpecInner, SpecSchemaType,
        SpecStructField, SpecTaggedVariant, SpecType, SpecUntaggedVariant, StructFieldName,
        StructFieldNameHint, UntaggedVariantNameHint,
        shape::{Struct, Tagged, Untagged},
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Status", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Enum(
            SchemaTypeInfo { name: "Status", .. },
            Enum {
                variants: [
                    EnumVariant::String("active"),
                    EnumVariant::String("inactive"),
                    EnumVariant::String("pending"),
                ],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Priority", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Enum(
            SchemaTypeInfo {
                name: "Priority",
                ..
            },
            Enum {
                variants: [
                    EnumVariant::I64(1),
                    EnumVariant::I64(2),
                    EnumVariant::I64(3)
                ],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Flag", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Enum(
            SchemaTypeInfo { name: "Flag", .. },
            Enum {
                variants: [EnumVariant::Bool(true), EnumVariant::Bool(false)],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Mixed", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Enum(
            SchemaTypeInfo { name: "Mixed", .. },
            Enum {
                variants: [
                    EnumVariant::String("text"),
                    EnumVariant::I64(42),
                    EnumVariant::Bool(true),
                ],
                ..
            },
        )),
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
    let arena = Arena::new();

    // `string` with `date-time` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: date-time
    "})
    .unwrap();
    let result = transform(&arena, &doc, "Timestamp", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::DateTime)),
    );

    // `string` with `date` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: date
    "})
    .unwrap();
    let result = transform(&arena, &doc, "Date", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::Date)),
    );

    // `string` with `uri` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: uri
    "})
    .unwrap();
    let result = transform(&arena, &doc, "Url", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::Url)),
    );

    // `string` with `uuid` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: uuid
    "})
    .unwrap();
    let result = transform(&arena, &doc, "Id", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::Uuid)),
    );

    // `string` with `byte` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: byte
    "})
    .unwrap();
    let result = transform(&arena, &doc, "Data", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::Bytes)),
    );

    // `string` with `binary` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: binary
    "})
    .unwrap();
    let result = transform(&arena, &doc, "RawData", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::Binary)),
    );

    // `string` without format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
    "})
    .unwrap();
    let result = transform(&arena, &doc, "Text", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::String)),
    );
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
    let arena = Arena::new();

    // `integer` with `int32` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
        format: int32
    "})
    .unwrap();
    let result = transform(&arena, &doc, "Count", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::I32)),
    );

    // `integer` with `int64` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
        format: int64
    "})
    .unwrap();
    let result = transform(&arena, &doc, "BigCount", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::I64)),
    );

    // `integer` with `unix-time` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
        format: unix-time
    "})
    .unwrap();
    let result = transform(&arena, &doc, "Timestamp", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::UnixTime)),
    );

    // `integer` without format defaults to `int32`.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
    "})
    .unwrap();
    let result = transform(&arena, &doc, "DefaultInt", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::I32)),
    );
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
    let arena = Arena::new();

    // `number` with `float` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
        format: float
    "})
    .unwrap();
    let result = transform(&arena, &doc, "Price", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::F32)),
    );

    // `number` with `double` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
        format: double
    "})
    .unwrap();
    let result = transform(&arena, &doc, "BigPrice", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::F64)),
    );

    // `number` with `unix-time` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
        format: unix-time
    "})
    .unwrap();
    let result = transform(&arena, &doc, "FloatTime", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::UnixTime)),
    );

    // `number` without format defaults to `double`.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
    "})
    .unwrap();
    let result = transform(&arena, &doc, "DefaultNumber", &schema);
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::F64)),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Items", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo { name: "Items", .. },
            SpecContainer::Array(SpecInner {
                ty: SpecType::Ref(_),
                ..
            }),
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Strings", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "Strings",
                ..
            },
            SpecContainer::Array(SpecInner {
                ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::String)),
                ..
            }),
        )),
    );
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
    let arena = Arena::new();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        required: [name, age]
        properties:
          name:
            type: string
          age:
            type: integer
    "})
    .unwrap();

    let result = transform(&arena, &doc, "Person", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Person", .. },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("name"),
                        ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::String)),
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("age"),
                        ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::I32)),
                        ..
                    },
                ],
                ..
            },
        )),
    );
}

#[test]
fn test_struct_with_additional_properties_ref() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Value:
              type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          name:
            type: string
        additionalProperties:
          $ref: '#/components/schemas/Value'
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Config", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Config", .. },
            Struct {
                fields: [
                    _,
                    SpecStructField {
                        name: StructFieldName::Hint(StructFieldNameHint::AdditionalProperties),
                        flattened: true,
                        required: true,
                        ty: SpecType::Inline(SpecInlineType::Container(
                            _,
                            SpecContainer::Map(SpecInner {
                                ty: SpecType::Ref(_),
                                ..
                            }),
                        )),
                        ..
                    },
                ],
                ..
            },
        )),
    );
}

#[test]
fn test_struct_with_additional_properties_inline() {
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
          type: object
          properties:
            inner:
              type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Config", &schema);

    // When `additionalProperties` is present alongside `properties`,
    // the result should be a struct with a flattened, inline map field.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Config", .. },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("name"),
                        flattened: false,
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Hint(StructFieldNameHint::AdditionalProperties),
                        flattened: true,
                        required: true,
                        ty: SpecType::Inline(SpecInlineType::Container(
                            InlineTypePath {
                                root: InlineTypePathRoot::Type("Config"),
                                segments: [InlineTypePathSegment::Field(StructFieldName::Hint(
                                    StructFieldNameHint::AdditionalProperties,
                                ))],
                            },
                            SpecContainer::Map(SpecInner {
                                ty: SpecType::Inline(SpecInlineType::Struct(
                                    InlineTypePath {
                                        root: InlineTypePathRoot::Type("Config"),
                                        segments: [
                                            InlineTypePathSegment::Field(StructFieldName::Hint(
                                                StructFieldNameHint::AdditionalProperties,
                                            )),
                                            InlineTypePathSegment::MapValue,
                                        ],
                                    },
                                    _,
                                )),
                                ..
                            }),
                        )),
                        ..
                    },
                ],
                ..
            },
        )),
    );
}

#[test]
fn test_struct_with_additional_properties_true() {
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "DynamicMap", &schema);

    // Empty `properties` with `additionalProperties: true` produces a
    // struct with a single flattened map field of type `Any`.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "DynamicMap",
                ..
            },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Hint(StructFieldNameHint::AdditionalProperties),
                    flattened: true,
                    required: true,
                    ty: SpecType::Inline(SpecInlineType::Container(
                        _,
                        SpecContainer::Map(SpecInner {
                            ty: SpecType::Inline(SpecInlineType::Any(_)),
                            ..
                        }),
                    )),
                    ..
                }],
                ..
            },
        )),
    );
}

#[test]
fn test_struct_without_properties_falls_through() {
    // A schema with only `additionalProperties` and no `properties`
    // falls through to `other()`, producing a map.
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
          type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "DynamicMap", &schema);

    assert_matches!(
        &result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "DynamicMap",
                ..
            },
            SpecContainer::Map(_),
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "User", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "User", .. },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("name"),
                        required: true,
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("email"),
                        required: false,
                        ..
                    },
                ],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Container", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "Container",
                ..
            },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("value"),
                    ty: SpecType::Inline(SpecInlineType::Container(
                        _,
                        SpecContainer::Optional(SpecInner {
                            ty: SpecType::Ref(_),
                            ..
                        }),
                    )),
                    ..
                }],
                ..
            },
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Container", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "Container",
                ..
            },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("value"),
                    ty: SpecType::Inline(SpecInlineType::Container(
                        _,
                        SpecContainer::Optional(SpecInner {
                            ty: SpecType::Inline(SpecInlineType::Primitive(
                                _,
                                PrimitiveType::String
                            )),
                            ..
                        }),
                    )),
                    ..
                }],
                ..
            },
        )),
    );
}

#[test]
fn test_struct_with_nullable_field_openapi_31_syntax() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        properties:
          value:
            type: [string, 'null']
        required:
          - value
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Container", &schema);

    // OpenAPI 3.1 `type: [T, 'null']` syntax should produce an `Optional(T)` field,
    // identical to OpenAPI 3.0 `nullable: true`.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "Container",
                ..
            },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("value"),
                    ty: SpecType::Inline(SpecInlineType::Container(
                        _,
                        SpecContainer::Optional(SpecInner {
                            ty: SpecType::Inline(SpecInlineType::Primitive(
                                _,
                                PrimitiveType::String
                            )),
                            ..
                        }),
                    )),
                    required: true,
                    ..
                }],
                ..
            },
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Entity", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Entity", .. },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("id"),
                    description: Some("An identifier"),
                    ..
                }],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "User", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "User", .. },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("name"),
                    description: Some("A user's name"),
                    ..
                }],
                ..
            },
        )),
    );
}

#[test]
fn test_struct_inline_all_of_becomes_parent() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        allOf:
          - type: object
            properties:
              name:
                type: string
          - type: object
            properties:
              age:
                type: integer
        properties:
          email:
            type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Person", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Person", .. },
            Struct {
                // The struct's own field is `email`; inherited fields
                // come from parents.
                fields: [SpecStructField {
                    name: StructFieldName::Name("email"),
                    ..
                }],
                // The inline `allOf` schemas become inline parent types.
                parents: [
                    SpecType::Inline(SpecInlineType::Struct(
                        InlineTypePath {
                            root: InlineTypePathRoot::Type("Person"),
                            segments: [InlineTypePathSegment::Parent(1)],
                        },
                        Struct {
                            fields: [SpecStructField {
                                name: StructFieldName::Name("name"),
                                ..
                            }],
                            ..
                        },
                    )),
                    SpecType::Inline(SpecInlineType::Struct(
                        InlineTypePath {
                            root: InlineTypePathRoot::Type("Person"),
                            segments: [InlineTypePathSegment::Parent(2)],
                        },
                        Struct {
                            fields: [SpecStructField {
                                name: StructFieldName::Name("age"),
                                ..
                            }],
                            ..
                        },
                    )),
                ],
                ..
            },
        )),
    );
}

#[test]
fn test_struct_mixed_all_of_ref_and_inline() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
        components:
          schemas:
            Base:
              type: object
              properties:
                id:
                  type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        allOf:
          - $ref: '#/components/schemas/Base'
          - type: object
            properties:
              name:
                type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Child", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Child", .. },
            Struct {
                // No own fields; all fields come from parents.
                fields: [],
                // Parents include both the named and inline schemas.
                parents: [
                    SpecType::Ref(r),
                    SpecType::Inline(SpecInlineType::Struct(
                        InlineTypePath {
                            root: InlineTypePathRoot::Type("Child"),
                            segments: [InlineTypePathSegment::Parent(2)],
                        },
                        Struct {
                            fields: [SpecStructField {
                                name: StructFieldName::Name("name"),
                                ..
                            }],
                            ..
                        },
                    )),
                ],
                ..
            },
        )) if r.name() == "Base",
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Animal", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Tagged(
            SchemaTypeInfo { name: "Animal", .. },
            Tagged {
                tag: "type",
                variants: [
                    SpecTaggedVariant {
                        name: "Dog",
                        aliases: ["dog"],
                        ..
                    },
                    SpecTaggedVariant {
                        name: "Cat",
                        aliases: ["cat"],
                        ..
                    },
                ],
                ..
            },
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Animal", &schema);

    // Inline schemas can't have discriminator mappings, so `Animal`
    // should lower to an untagged union with two variants.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Untagged(
            SchemaTypeInfo { name: "Animal", .. },
            Untagged {
                variants: [
                    SpecUntaggedVariant::Some(UntaggedVariantNameHint::Index(1), SpecType::Ref(_)),
                    SpecUntaggedVariant::Some(
                        UntaggedVariantNameHint::Index(2),
                        SpecType::Inline(_)
                    ),
                ],
                ..
            },
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Result", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Tagged(
            SchemaTypeInfo { name: "Result", .. },
            Tagged {
                variants: [SpecTaggedVariant {
                    name: "Success",
                    aliases: ["good", "ok", "success"],
                    ..
                }],
                ..
            },
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Animal", &schema);

    // `Cat` has no discriminator tag, so `Animal` should lower to
    // an untagged union with two variants.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Untagged(
            SchemaTypeInfo { name: "Animal", .. },
            Untagged {
                variants: [
                    SpecUntaggedVariant::Some(UntaggedVariantNameHint::Index(1), SpecType::Ref(_)),
                    SpecUntaggedVariant::Some(UntaggedVariantNameHint::Index(2), SpecType::Ref(_)),
                ],
                ..
            },
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Animal", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Tagged(
            SchemaTypeInfo { name: "Animal", .. },
            Tagged {
                description: Some("A tagged union of animals"),
                tag: "type",
                variants: [SpecTaggedVariant {
                    name: "Dog",
                    aliases: ["dog"],
                    ..
                }],
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "StringOrNumber", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Untagged(
            SchemaTypeInfo {
                name: "StringOrNumber",
                ..
            },
            Untagged {
                variants: [
                    SpecUntaggedVariant::Some(UntaggedVariantNameHint::Index(1), SpecType::Ref(_)),
                    SpecUntaggedVariant::Some(UntaggedVariantNameHint::Index(2), SpecType::Ref(_)),
                ],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Empty", &schema);

    assert_matches!(result, SpecType::Schema(SpecSchemaType::Any(_)));
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "JustNull", &schema);

    assert_matches!(result, SpecType::Schema(SpecSchemaType::Any(_)));
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "JustString", &schema);

    assert_matches!(result, SpecType::Ref(_));
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "ABC", &schema);

    // Variants should have 1-based indices in their name hints.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Untagged(
            SchemaTypeInfo { name: "ABC", .. },
            Untagged {
                variants: [
                    SpecUntaggedVariant::Some(UntaggedVariantNameHint::Index(1), _),
                    SpecUntaggedVariant::Some(UntaggedVariantNameHint::Index(2), _),
                    SpecUntaggedVariant::Some(UntaggedVariantNameHint::Index(3), _),
                ],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "StringOrNull", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "StringOrNull",
                ..
            },
            SpecContainer::Optional(_),
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Contact", &schema);

    // Both fields should be flattened.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "Contact",
                ..
            },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("Address"),
                        flattened: true,
                        required: false,
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("Email"),
                        flattened: true,
                        required: false,
                        ..
                    },
                ],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Contact", &schema);

    // Both fields should be named `Address`, since they reference the same
    // type.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "Contact",
                ..
            },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("Address"),
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("Address"),
                        ..
                    },
                ],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Mixed", &schema);

    // Both inline schemas should have index hints.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Mixed", .. },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Hint(StructFieldNameHint::Index(1)),
                        flattened: true,
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Hint(StructFieldNameHint::Index(2)),
                        flattened: true,
                        ..
                    },
                ],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Combined", &schema);

    // `Extra2` is an own field; `Extra1` and `Extra3` are flattened.
    // Own fields should precede the flattened fields.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "Combined",
                ..
            },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("Extra2"),
                        flattened: false,
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("Extra1"),
                        flattened: true,
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("Extra3"),
                        flattened: true,
                        ..
                    },
                ],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Container", &schema);

    // Both nullable schemas should be flattened.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "Container",
                ..
            },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("NullableString1"),
                        flattened: true,
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("NullableString2"),
                        flattened: true,
                        ..
                    },
                ],
                ..
            },
        )),
    );
}

#[test]
fn test_any_of_with_all_of() {
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Combined", &schema);

    // Only flattened `anyOf` fields should be stored directly; the inherited
    // `id` field is accessed via graph traversal through `parents()`.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "Combined",
                ..
            },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("Extra1"),
                        flattened: true,
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("Extra2"),
                        flattened: true,
                        ..
                    },
                ],
                // Parent reference to `Base` should be present.
                parents: [_],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Flag", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::Bool)),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "CustomType", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::String)),
    );
}

#[test]
fn test_empty_type_array_produces_any() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: []
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "NoType", &schema);

    assert_matches!(result, SpecType::Schema(SpecSchemaType::Any(_)));
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "ArrayAny", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "ArrayAny",
                ..
            },
            SpecContainer::Array(SpecInner {
                ty: SpecType::Inline(SpecInlineType::Any(_)),
                ..
            }),
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "EmptyObject", &schema);

    // An `object` schema without properties should become an empty struct.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "EmptyObject",
                ..
            },
            Struct { fields: [], .. },
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Empty", &schema);

    // A schema with no `type` and no `properties` should become `Any`.
    assert_matches!(result, SpecType::Schema(SpecSchemaType::Any(_)));
}

#[test]
fn test_type_and_null_in_type_array_creates_nullable() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: [string, 'null']
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "StringOrNull", &schema);

    // As a special case, `type: [T, "null"]` should produce a
    // `Container(Optional(T))`.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "StringOrNull",
                ..
            },
            SpecContainer::Optional(SpecInner {
                ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::String)),
                ..
            }),
        )),
    );
}

#[test]
fn test_type_array_and_null_creates_nullable_array() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: [array, 'null']
        items:
          type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "StringArrayOrNull", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "StringArrayOrNull",
                ..
            },
            SpecContainer::Optional(SpecInner {
                ty: SpecType::Inline(SpecInlineType::Container(
                    _,
                    SpecContainer::Array(SpecInner {
                        ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::String)),
                        ..
                    }),
                )),
                ..
            }),
        )),
    );
}

#[test]
fn test_type_object_and_null_creates_nullable_map() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: [object, 'null']
        additionalProperties:
          type: integer
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "IntMapOrNull", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "IntMapOrNull",
                ..
            },
            SpecContainer::Optional(SpecInner {
                ty: SpecType::Inline(SpecInlineType::Container(
                    _,
                    SpecContainer::Map(SpecInner {
                        ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::I32)),
                        ..
                    }),
                )),
                ..
            }),
        )),
    );
}

#[test]
fn test_multiple_types_string_and_integer_untagged() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: [string, integer]
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "StringOrInt", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Untagged(
            SchemaTypeInfo {
                name: "StringOrInt",
                ..
            },
            Untagged {
                variants: [
                    SpecUntaggedVariant::Some(
                        UntaggedVariantNameHint::Primitive(PrimitiveType::String),
                        SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::String)),
                    ),
                    SpecUntaggedVariant::Some(
                        UntaggedVariantNameHint::Primitive(PrimitiveType::I32),
                        SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::I32)),
                    ),
                ],
                ..
            },
        )),
    );
}

#[test]
fn test_type_array_with_format_produces_inline_variants() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: [string, integer]
        format: date-time
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "DateOrUnix", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Untagged(
            SchemaTypeInfo {
                name: "DateOrUnix",
                ..
            },
            Untagged {
                variants: [
                    SpecUntaggedVariant::Some(
                        UntaggedVariantNameHint::Primitive(PrimitiveType::DateTime),
                        SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::DateTime)),
                    ),
                    SpecUntaggedVariant::Some(
                        UntaggedVariantNameHint::Primitive(PrimitiveType::I32),
                        SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::I32)),
                    ),
                ],
                ..
            },
        )),
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
        required: [items]
        properties:
          items:
            type: array
            items:
              type: object
              required: [field]
              properties:
                field:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Outer", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Outer", .. },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("items"),
                    ty: SpecType::Inline(SpecInlineType::Container(
                        InlineTypePath {
                            root: InlineTypePathRoot::Type("Outer"),
                            segments: [InlineTypePathSegment::Field(StructFieldName::Name(
                                "items",
                            ))],
                        },
                        SpecContainer::Array(SpecInner {
                            ty: SpecType::Inline(SpecInlineType::Struct(
                                InlineTypePath {
                                    root: InlineTypePathRoot::Type("Outer"),
                                    segments: [
                                        InlineTypePathSegment::Field(StructFieldName::Name(
                                            "items",
                                        )),
                                        InlineTypePathSegment::ArrayItem,
                                    ],
                                },
                                Struct {
                                    fields: [SpecStructField {
                                        name: StructFieldName::Name("field"),
                                        ty: SpecType::Inline(SpecInlineType::Primitive(
                                            _,
                                            PrimitiveType::String
                                        )),
                                        ..
                                    }],
                                    ..
                                },
                            )),
                            ..
                        }),
                    )),
                    ..
                }],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "NullEnum", &schema);

    // `null` values are filtered out from enum variants, producing an enum
    // with zero variants.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Enum(
            SchemaTypeInfo {
                name: "NullEnum",
                ..
            },
            Enum { variants: [], .. },
        )),
    );
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
        required: [name]
        properties:
          name:
            type: string
        additionalProperties: false
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "StrictObject", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "StrictObject",
                ..
            },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("name"),
                    ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::String)),
                    ..
                }],
                ..
            },
        )),
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Container", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "Container",
                ..
            },
            SpecContainer::Array(SpecInner {
                ty: SpecType::Inline(SpecInlineType::Struct(
                    InlineTypePath {
                        root: InlineTypePathRoot::Type("Container"),
                        segments: [InlineTypePathSegment::ArrayItem],
                    },
                    _,
                )),
                ..
            }),
        )),
    );
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Dictionary", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "Dictionary",
                ..
            },
            SpecContainer::Map(SpecInner {
                ty: SpecType::Inline(SpecInlineType::Struct(
                    InlineTypePath {
                        root: InlineTypePathRoot::Type("Dictionary"),
                        segments: [InlineTypePathSegment::MapValue],
                    },
                    _,
                )),
                ..
            }),
        )),
    );
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
        required: [nested]
        properties:
          nested:
            type: object
            properties:
              inner:
                type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Outer", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Outer", .. },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("nested"),
                    ty: SpecType::Inline(SpecInlineType::Struct(
                        InlineTypePath {
                            root: InlineTypePathRoot::Type("Outer"),
                            segments: [InlineTypePathSegment::Field(StructFieldName::Name(
                                "nested",
                            ))],
                        },
                        _,
                    )),
                    ..
                }],
                ..
            },
        )),
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
        required: [animal]
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Container", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "Container",
                ..
            },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("animal"),
                    ty: SpecType::Inline(SpecInlineType::Tagged(
                        InlineTypePath {
                            root: InlineTypePathRoot::Type("Container"),
                            segments: [InlineTypePathSegment::Field(StructFieldName::Name(
                                "animal",
                            ))],
                        },
                        Tagged {
                            tag: "kind",
                            variants: [
                                SpecTaggedVariant {
                                    name: "Cat",
                                    aliases: ["cat"],
                                    ..
                                },
                                SpecTaggedVariant {
                                    name: "Dog",
                                    aliases: ["dog"],
                                    ..
                                },
                            ],
                            ..
                        },
                    )),
                    ..
                }],
                ..
            },
        )),
    );
}

// MARK: Recursive schemas

#[test]
fn test_recursive_all_of_ref_nullable() {
    // Tests that a schema with `nullable: true` + `allOf` + `$ref`
    // with a self-referential schema doesn't cause a stack overflow.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Node:
              type: object
              properties:
                value:
                  type: string
                next:
                  nullable: true
                  allOf:
                    - $ref: '#/components/schemas/Node'
              required:
                - value
                - next
    "})
    .unwrap();

    let schema = &doc.components.as_ref().unwrap().schemas["Node"];
    let arena = Arena::new();
    let result = transform(&arena, &doc, "Node", schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Node", .. },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("value"),
                        ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::String)),
                        required: true,
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("next"),
                        ty: SpecType::Inline(SpecInlineType::Container(
                            _,
                            SpecContainer::Optional(_),
                        )),
                        required: true,
                        ..
                    },
                ],
                ..
            },
        )),
    );
}

#[test]
fn test_recursive_all_of_ref() {
    // Similar to the above, but without `nullable: true`.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Node:
              type: object
              properties:
                value:
                  type: string
                next:
                  allOf:
                    - $ref: '#/components/schemas/Node'
    "})
    .unwrap();

    let schema = &doc.components.as_ref().unwrap().schemas["Node"];
    let arena = Arena::new();
    let result = transform(&arena, &doc, "Node", schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Node", .. },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("value"),
                        ty: SpecType::Inline(SpecInlineType::Container(
                            _,
                            SpecContainer::Optional(_),
                        )),
                        required: false,
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("next"),
                        ty: SpecType::Inline(SpecInlineType::Container(
                            _,
                            SpecContainer::Optional(_),
                        )),
                        required: false,
                        ..
                    },
                ],
                ..
            },
        )),
    );
}

#[test]
fn test_recursive_multi_all_of_ref_no_stack_overflow() {
    // Multiple elements in `allOf`, one of which is a self-reference.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        paths: {}
        components:
          schemas:
            Mixin:
              type: object
              properties:
                extra:
                  type: string
            Node:
              type: object
              properties:
                value:
                  type: string
                next:
                  allOf:
                    - $ref: '#/components/schemas/Node'
                    - $ref: '#/components/schemas/Mixin'
    "})
    .unwrap();

    let schema = &doc.components.as_ref().unwrap().schemas["Node"];
    let arena = Arena::new();
    let result = transform(&arena, &doc, "Node", schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Node", .. },
            Struct {
                fields: [
                    SpecStructField {
                        name: StructFieldName::Name("value"),
                        ..
                    },
                    SpecStructField {
                        name: StructFieldName::Name("next"),
                        ..
                    },
                ],
                ..
            },
        )),
    );
}

// MARK: Named containers

#[test]
fn test_named_array_schema_produces_container() {
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

    let arena = Arena::new();
    let result = transform(&arena, &doc, "StringList", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "StringList",
                ..
            },
            SpecContainer::Array(SpecInner {
                ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::String)),
                ..
            }),
        )),
    );
}

#[test]
fn test_named_array_with_inline_one_of_items_produces_container() {
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
        type: array
        items:
          oneOf:
            - $ref: '#/components/schemas/Cat'
            - $ref: '#/components/schemas/Dog'
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Animals", &schema);

    // A named array schema should produce a `Container` for the array,
    // wrapped in a `SpecSchemaType` that preserves the schema's identity.
    // The inline `oneOf` should produce an inline untagged union.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "Animals",
                ..
            },
            SpecContainer::Array(SpecInner {
                ty: SpecType::Inline(SpecInlineType::Untagged(..)),
                ..
            }),
        )),
    );
}

#[test]
fn test_named_map_schema_produces_container() {
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
          type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Metadata", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "Metadata",
                ..
            },
            SpecContainer::Map(_),
        )),
    );
}

#[test]
fn test_named_nullable_schema_produces_container() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.1.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: [string, 'null']
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "NullableString", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "NullableString",
                ..
            },
            SpecContainer::Optional(SpecInner {
                ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::String)),
                ..
            }),
        )),
    );
}

#[test]
fn test_named_container_preserves_description() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        description: A list of identifiers
        type: array
        items:
          type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Ids", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo { name: "Ids", .. },
            SpecContainer::Array(SpecInner {
                description: Some("A list of identifiers"),
                ..
            }),
        )),
    );
}

#[test]
fn test_named_primitive_does_not_produce_container() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Name", &schema);

    // Bare primitives should _not_ be wrapped; they don't contain inline types,
    // and don't benefit from a type alias.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Primitive(_, PrimitiveType::String)),
    );
}

#[test]
fn test_untagged_single_variant_one_of_ref_produces_container() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0.0
        components:
          schemas:
            Inner:
              type: object
              properties:
                value:
                  type: string
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        oneOf:
          - type: 'null'
          - $ref: '#/components/schemas/Inner'
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "MaybeInner", &schema);

    // A `oneOf` with `null` and a schema reference should produce a
    // `Container(Optional(Ref(...)))`.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Container(
            SchemaTypeInfo {
                name: "MaybeInner",
                ..
            },
            SpecContainer::Optional(SpecInner {
                ty: SpecType::Ref(_),
                ..
            }),
        )),
    );
}

// MARK: Inline containers

#[test]
fn test_inline_array_produces_inline_container() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: object
        required: [items]
        properties:
          items:
            type: array
            items:
              type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Container", &schema);

    // Struct fields that are arrays become inline containers,
    // not schema containers.
    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo {
                name: "Container",
                ..
            },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("items"),
                    ty: SpecType::Inline(SpecInlineType::Container(_, SpecContainer::Array(_))),
                    ..
                }],
                ..
            },
        )),
    );
}

#[test]
fn test_optional_field_container_description_is_not_parent_schema() {
    // When a struct field is wrapped in `Optional`, the container's `Inner`
    // description should reflect the field, not the parent struct's description.
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test
          version: 1.0.0
    "})
    .unwrap();
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        description: A parent struct
        type: object
        properties:
          nickname:
            description: The nickname
            type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let result = transform(&arena, &doc, "Parent", &schema);

    assert_matches!(
        result,
        SpecType::Schema(SpecSchemaType::Struct(
            SchemaTypeInfo { name: "Parent", .. },
            Struct {
                fields: [SpecStructField {
                    name: StructFieldName::Name("nickname"),
                    ty: SpecType::Inline(SpecInlineType::Container(
                        _,
                        SpecContainer::Optional(SpecInner {
                            description: Some("The nickname"),
                            ..
                        }),
                    )),
                    ..
                }],
                ..
            },
        )),
    );
}
