//! IR transformation tests.

use serde_json::Number;

use crate::{
    ir::{
        Container, InlineIrType, InlineIrTypePathRoot, InlineIrTypePathSegment, Inner,
        IrEnumVariant, IrStructField, IrStructFieldName, IrStructFieldNameHint, IrTaggedVariant,
        IrType, IrUntaggedVariant, IrUntaggedVariantNameHint, PrimitiveIrType, SchemaIrType,
        SchemaTypeInfo, transform::transform,
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

    let result = transform(&doc, "Status", &schema);

    let enum_ = match result {
        IrType::Schema(SchemaIrType::Enum(SchemaTypeInfo { name: "Status", .. }, enum_)) => enum_,
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

    let result = transform(&doc, "Priority", &schema);

    let enum_ = match result {
        IrType::Schema(SchemaIrType::Enum(
            SchemaTypeInfo {
                name: "Priority", ..
            },
            enum_,
        )) => enum_,
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

    let result = transform(&doc, "Flag", &schema);

    let enum_ = match result {
        IrType::Schema(SchemaIrType::Enum(SchemaTypeInfo { name: "Flag", .. }, enum_)) => enum_,
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

    let result = transform(&doc, "Mixed", &schema);

    let enum_ = match result {
        IrType::Schema(SchemaIrType::Enum(SchemaTypeInfo { name: "Mixed", .. }, enum_)) => enum_,
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
    let result = transform(&doc, "Timestamp", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::DateTime));

    // `string` with `date` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: date
    "})
    .unwrap();
    let result = transform(&doc, "Date", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Date));

    // `string` with `uri` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: uri
    "})
    .unwrap();
    let result = transform(&doc, "Url", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Url));

    // `string` with `uuid` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: uuid
    "})
    .unwrap();
    let result = transform(&doc, "Id", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Uuid));

    // `string` with `byte` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: byte
    "})
    .unwrap();
    let result = transform(&doc, "Data", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Bytes));

    // `string` with `binary` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
        format: binary
    "})
    .unwrap();
    let result = transform(&doc, "RawData", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::Binary));

    // `string` without format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: string
    "})
    .unwrap();
    let result = transform(&doc, "Text", &schema);
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
    let result = transform(&doc, "Count", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::I32));

    // `integer` with `int64` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
        format: int64
    "})
    .unwrap();
    let result = transform(&doc, "BigCount", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::I64));

    // `integer` with `unix-time` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
        format: unix-time
    "})
    .unwrap();
    let result = transform(&doc, "Timestamp", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::UnixTime));

    // `integer` without format defaults to `int32`.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: integer
    "})
    .unwrap();
    let result = transform(&doc, "DefaultInt", &schema);
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
    let result = transform(&doc, "Price", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::F32));

    // `number` with `double` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
        format: double
    "})
    .unwrap();
    let result = transform(&doc, "BigPrice", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::F64));

    // `number` with `unix-time` format.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
        format: unix-time
    "})
    .unwrap();
    let result = transform(&doc, "FloatTime", &schema);
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::UnixTime));

    // `number` without format defaults to `double`.
    let schema: Schema = serde_yaml::from_str(indoc::indoc! {"
        type: number
    "})
    .unwrap();
    let result = transform(&doc, "DefaultNumber", &schema);
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

    let result = transform(&doc, "Items", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo { name: "Items", .. },
            container,
        )) => container,
        other => panic!("expected container `Items`; got `{other:?}`"),
    };
    let items = match &container {
        Container::Array(Inner { ty, .. }) => ty,
        other => panic!("expected array; got `{other:?}`"),
    };
    assert_matches!(&**items, IrType::Ref(_));
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

    let result = transform(&doc, "Strings", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "Strings", ..
            },
            container,
        )) => container,
        other => panic!("expected container `Strings`; got `{other:?}`"),
    };
    let items = match &container {
        Container::Array(Inner { ty, .. }) => ty,
        other => panic!("expected array; got `{other:?}`"),
    };
    assert_matches!(&**items, IrType::Primitive(PrimitiveIrType::String));
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
        required: [name, age]
        properties:
          name:
            type: string
          age:
            type: integer
    "})
    .unwrap();

    let result = transform(&doc, "Person", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Person", .. }, struct_)) => {
            struct_
        }
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

    let result = transform(&doc, "Config", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Config", .. }, struct_)) => {
            struct_
        }
        other => panic!("expected struct `Config`; got `{other:?}`"),
    };
    let [
        _,
        IrStructField {
            name: IrStructFieldName::Hint(IrStructFieldNameHint::AdditionalProperties),
            flattened: true,
            required: true,
            ty,
            ..
        },
    ] = &*struct_.fields
    else {
        panic!("expected two fields; got `{:?}`", struct_.fields);
    };
    assert_matches!(
        ty,
        IrType::Inline(
            InlineIrType::Container(_, Container::Map(inner)),
        ) if matches!(&*inner.ty, IrType::Ref(_)),
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

    let result = transform(&doc, "Config", &schema);

    // When `additionalProperties` is present alongside `properties`,
    // the result should be a struct with a flattened map field.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Config", .. }, struct_)) => {
            struct_
        }
        other => panic!("expected struct `Config`; got `{other:?}`"),
    };
    let [
        IrStructField {
            name: IrStructFieldName::Name("name"),
            flattened: false,
            ..
        },
        IrStructField {
            name: IrStructFieldName::Hint(IrStructFieldNameHint::AdditionalProperties),
            flattened: true,
            required: true,
            ty,
            ..
        },
    ] = &*struct_.fields
    else {
        panic!("expected two fields; got `{:?}`", struct_.fields);
    };

    // The container path should be `Type("Config") / Field(AdditionalProperties)`.
    let IrType::Inline(InlineIrType::Container(container_path, Container::Map(inner))) = ty else {
        panic!("expected map; got `{ty:?}`");
    };
    assert_matches!(container_path.root, InlineIrTypePathRoot::Type("Config"));
    assert_matches!(
        &*container_path.segments,
        [InlineIrTypePathSegment::Field(IrStructFieldName::Hint(
            IrStructFieldNameHint::AdditionalProperties,
        ))]
    );

    // The inline value type path should append `MapValue`.
    let IrType::Inline(InlineIrType::Struct(value_path, _)) = &*inner.ty else {
        panic!("expected inline struct; got `{:?}`", inner.ty);
    };
    assert_matches!(value_path.root, InlineIrTypePathRoot::Type("Config"));
    assert_matches!(
        &*value_path.segments,
        [
            InlineIrTypePathSegment::Field(IrStructFieldName::Hint(
                IrStructFieldNameHint::AdditionalProperties,
            )),
            InlineIrTypePathSegment::MapValue,
        ]
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

    let result = transform(&doc, "DynamicMap", &schema);

    // Empty `properties` with `additionalProperties: true` produces a
    // struct with a single flattened map field of type `Any`.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "DynamicMap", ..
            },
            struct_,
        )) => struct_,
        other => panic!("expected struct `DynamicMap`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [IrStructField {
            name: IrStructFieldName::Hint(IrStructFieldNameHint::AdditionalProperties),
            flattened: true,
            required: true,
            ty: IrType::Inline(InlineIrType::Container(_, Container::Map(inner))),
            ..
        }] if matches!(&*inner.ty, IrType::Any)
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

    let result = transform(&doc, "DynamicMap", &schema);

    assert_matches!(
        &result,
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "DynamicMap",
                ..
            },
            Container::Map(_),
        ))
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

    let result = transform(&doc, "User", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "User", .. }, struct_)) => {
            struct_
        }
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

    let result = transform(&doc, "Container", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "Container", ..
            },
            struct_,
        )) => struct_,
        other => panic!("expected struct `Container`; got `{other:?}`"),
    };
    let [
        IrStructField {
            name: IrStructFieldName::Name("value"),
            ty:
                IrType::Inline(InlineIrType::Container(_, Container::Optional(Inner { ty: inner, .. }))),
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

    let result = transform(&doc, "Container", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "Container", ..
            },
            struct_,
        )) => struct_,
        other => panic!("expected struct `Container`; got `{other:?}`"),
    };
    let [
        IrStructField {
            name: IrStructFieldName::Name("value"),
            ty:
                IrType::Inline(InlineIrType::Container(_, Container::Optional(Inner { ty: inner, .. }))),
            ..
        },
    ] = &*struct_.fields
    else {
        panic!("expected single nullable field; got `{:?}`", struct_.fields);
    };
    assert_matches!(&**inner, IrType::Primitive(PrimitiveIrType::String));
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

    let result = transform(&doc, "Container", &schema);

    // OpenAPI 3.1 `type: [T, 'null']` syntax should produce an `Optional(T)` field,
    // identical to OpenAPI 3.0 `nullable: true`.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "Container", ..
            },
            struct_,
        )) => struct_,
        other => panic!("expected struct `Container`; got `{other:?}`"),
    };
    let [
        IrStructField {
            name: IrStructFieldName::Name("value"),
            ty:
                IrType::Inline(InlineIrType::Container(_, Container::Optional(Inner { ty: inner, .. }))),
            required: true,
            ..
        },
    ] = &*struct_.fields
    else {
        panic!(
            "expected single required nullable field; got `{:?}`",
            struct_.fields
        );
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

    let result = transform(&doc, "Entity", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Entity", .. }, struct_)) => {
            struct_
        }
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

    let result = transform(&doc, "User", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "User", .. }, struct_)) => {
            struct_
        }
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

    let result = transform(&doc, "Person", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Person", .. }, struct_)) => {
            struct_
        }
        other => panic!("expected struct `Person`; got `{other:?}`"),
    };

    // The struct's own field is `email`; inherited fields come from parents.
    assert_matches!(
        &*struct_.fields,
        [IrStructField {
            name: IrStructFieldName::Name("email"),
            ..
        }],
    );

    // The inline `allOf` schemas become inline parent types.
    assert_matches!(
        &*struct_.parents,
        [
            IrType::Inline(InlineIrType::Struct(path1, parent1)),
            IrType::Inline(InlineIrType::Struct(path2, parent2)),
        ] if path1.root == InlineIrTypePathRoot::Type("Person")
            && path1.segments == vec![InlineIrTypePathSegment::Parent(1)]
            && matches!(&*parent1.fields, [IrStructField { name: IrStructFieldName::Name("name"), .. }])
            && path2.root == InlineIrTypePathRoot::Type("Person")
            && path2.segments == vec![InlineIrTypePathSegment::Parent(2)]
            && matches!(&*parent2.fields, [IrStructField { name: IrStructFieldName::Name("age"), .. }]),
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

    let result = transform(&doc, "Child", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Child", .. }, struct_)) => {
            struct_
        }
        other => panic!("expected struct `Child`; got `{other:?}`"),
    };

    // No own fields; all fields come from parents.
    assert!(struct_.fields.is_empty());

    // Parents include both the named and inline schemas.
    assert_matches!(
        &*struct_.parents,
        [
            IrType::Ref(r),
            IrType::Inline(InlineIrType::Struct(path, parent)),
        ] if r.name() == "Base"
            && path.root == InlineIrTypePathRoot::Type("Child")
            && path.segments == vec![InlineIrTypePathSegment::Parent(2)]
            && matches!(&*parent.fields, [IrStructField { name: IrStructFieldName::Name("name"), .. }])
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

    let result = transform(&doc, "Animal", &schema);

    let tagged = match result {
        IrType::Schema(SchemaIrType::Tagged(SchemaTypeInfo { name: "Animal", .. }, tagged)) => {
            tagged
        }
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

    let result = transform(&doc, "Animal", &schema);

    // Inline schemas can't have discriminator mappings, so `Animal`
    // should lower to an untagged union with two variants.
    let untagged = match result {
        IrType::Schema(SchemaIrType::Untagged(SchemaTypeInfo { name: "Animal", .. }, untagged)) => {
            untagged
        }
        other => panic!("expected untagged union `Animal`; got `{other:?}`"),
    };
    assert_matches!(
        &*untagged.variants,
        [
            IrUntaggedVariant::Some(IrUntaggedVariantNameHint::Index(1), IrType::Ref(_)),
            IrUntaggedVariant::Some(IrUntaggedVariantNameHint::Index(2), IrType::Inline(_)),
        ],
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

    let result = transform(&doc, "Result", &schema);

    let tagged = match result {
        IrType::Schema(SchemaIrType::Tagged(SchemaTypeInfo { name: "Result", .. }, tagged)) => {
            tagged
        }
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

    let result = transform(&doc, "Animal", &schema);

    // `Cat` has no discriminator tag, so `Animal` should lower to
    // an untagged union with two variants.
    let untagged = match result {
        IrType::Schema(SchemaIrType::Untagged(SchemaTypeInfo { name: "Animal", .. }, untagged)) => {
            untagged
        }
        other => panic!("expected untagged union `Animal`; got `{other:?}`"),
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

    let result = transform(&doc, "Animal", &schema);

    let tagged = match result {
        IrType::Schema(SchemaIrType::Tagged(SchemaTypeInfo { name: "Animal", .. }, tagged)) => {
            tagged
        }
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

    let result = transform(&doc, "StringOrNumber", &schema);

    let untagged = match result {
        IrType::Schema(SchemaIrType::Untagged(
            SchemaTypeInfo {
                name: "StringOrNumber",
                ..
            },
            untagged,
        )) => untagged,
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

    let result = transform(&doc, "Empty", &schema);

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

    let result = transform(&doc, "JustNull", &schema);

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

    let result = transform(&doc, "JustString", &schema);

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

    let result = transform(&doc, "ABC", &schema);

    // Variants should have 1-based indices in their name hints.
    let untagged = match result {
        IrType::Schema(SchemaIrType::Untagged(SchemaTypeInfo { name: "ABC", .. }, untagged)) => {
            untagged
        }
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

    let result = transform(&doc, "StringOrNull", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "StringOrNull",
                ..
            },
            container,
        )) => container,
        other => panic!("expected container `StringOrNull`; got `{other:?}`"),
    };
    assert_matches!(&container, Container::Optional(_));
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

    let result = transform(&doc, "Contact", &schema);

    // Both fields should be flattened.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "Contact", ..
            },
            struct_,
        )) => struct_,
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

    let result = transform(&doc, "Contact", &schema);

    // Both fields should be named `Address`, since they reference the same
    // type.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "Contact", ..
            },
            struct_,
        )) => struct_,
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

    let result = transform(&doc, "Mixed", &schema);

    // Both inline schemas should have index hints.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Mixed", .. }, struct_)) => {
            struct_
        }
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

    let result = transform(&doc, "Combined", &schema);

    // `Extra2` is an own field; `Extra1` and `Extra3` are flattened.
    // Own fields should precede the flattened fields.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "Combined", ..
            },
            struct_,
        )) => struct_,
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

    let result = transform(&doc, "Container", &schema);

    // Both nullable schemas should be flattened.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "Container", ..
            },
            struct_,
        )) => struct_,
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

    let result = transform(&doc, "Combined", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "Combined", ..
            },
            struct_,
        )) => struct_,
        other => panic!("expected struct `Combined`; got `{other:?}`"),
    };

    // Only flattened `anyOf` fields should be stored directly; the inherited
    // `id` field is accessed via graph traversal through `parents()`.
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Name("Extra1"),
                flattened: true,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("Extra2"),
                flattened: true,
                ..
            },
        ],
    );
    // Parent reference to `Base` should be present.
    assert_eq!(struct_.parents.len(), 1);
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

    let result = transform(&doc, "Flag", &schema);

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

    let result = transform(&doc, "CustomType", &schema);

    assert_matches!(result, IrType::Primitive(PrimitiveIrType::String));
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

    let result = transform(&doc, "NoType", &schema);

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

    let result = transform(&doc, "ArrayAny", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "ArrayAny", ..
            },
            container,
        )) => container,
        other => panic!("expected container `ArrayAny`; got `{other:?}`"),
    };
    let items = match &container {
        Container::Array(Inner { ty, .. }) => ty,
        other => panic!("expected array; got `{other:?}`"),
    };
    assert_matches!(&**items, IrType::Any);
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

    let result = transform(&doc, "EmptyObject", &schema);

    // An `object` schema without properties should become an empty struct.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "EmptyObject",
                ..
            },
            struct_,
        )) => struct_,
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

    let result = transform(&doc, "Empty", &schema);

    // A schema with no `type` and no `properties` should become `Any`.
    assert_matches!(result, IrType::Any);
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

    let result = transform(&doc, "StringOrNull", &schema);

    // As a special case, `type: [T, "null"]` should produce a
    // `Container(Optional(T))`.
    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "StringOrNull",
                ..
            },
            container,
        )) => container,
        other => panic!("expected container `StringOrNull`; got `{other:?}`"),
    };
    let inner = match &container {
        Container::Optional(Inner { ty, .. }) => ty,
        other => panic!("expected nullable; got `{other:?}`"),
    };
    assert_matches!(&**inner, IrType::Primitive(PrimitiveIrType::String));
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

    let result = transform(&doc, "StringArrayOrNull", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "StringArrayOrNull",
                ..
            },
            container,
        )) => container,
        other => panic!("expected container `StringArrayOrNull`; got `{other:?}`"),
    };
    let inner = match &container {
        Container::Optional(Inner { ty, .. }) => ty,
        other => panic!("expected optional; got `{other:?}`"),
    };
    let inner_container = match &**inner {
        IrType::Inline(InlineIrType::Container(_, container)) => container,
        other => panic!("expected inline container; got `{other:?}`"),
    };
    let items = match inner_container {
        Container::Array(Inner { ty, .. }) => ty,
        other => panic!("expected array; got `{other:?}`"),
    };
    assert_matches!(&**items, IrType::Primitive(PrimitiveIrType::String));
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

    let result = transform(&doc, "IntMapOrNull", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "IntMapOrNull",
                ..
            },
            container,
        )) => container,
        other => panic!("expected container `IntMapOrNull`; got `{other:?}`"),
    };
    let inner = match &container {
        Container::Optional(Inner { ty, .. }) => ty,
        other => panic!("expected optional; got `{other:?}`"),
    };
    let inner_container = match &**inner {
        IrType::Inline(InlineIrType::Container(_, container)) => container,
        other => panic!("expected inline container; got `{other:?}`"),
    };
    let values = match inner_container {
        Container::Map(Inner { ty, .. }) => ty,
        other => panic!("expected map; got `{other:?}`"),
    };
    assert_matches!(&**values, IrType::Primitive(PrimitiveIrType::I32));
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

    let result = transform(&doc, "StringOrInt", &schema);

    let untagged = match result {
        IrType::Schema(SchemaIrType::Untagged(
            SchemaTypeInfo {
                name: "StringOrInt",
                ..
            },
            untagged,
        )) => untagged,
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

    let result = transform(&doc, "Outer", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Outer", .. }, struct_)) => {
            struct_
        }
        other => panic!("expected struct `Outer`; got `{other:?}`"),
    };
    let [
        IrStructField {
            name: IrStructFieldName::Name("items"),
            ty:
                IrType::Inline(InlineIrType::Container(path, Container::Array(Inner { ty: items, .. }))),
            ..
        },
    ] = &*struct_.fields
    else {
        panic!("expected named inline array; got `{:?}`", struct_.fields);
    };

    // Container type path should be correct.
    assert_matches!(path.root, InlineIrTypePathRoot::Type("Outer"));
    assert_matches!(
        &*path.segments,
        [InlineIrTypePathSegment::Field(IrStructFieldName::Name(
            "items"
        ))],
    );

    let (path, inner_struct) = match &**items {
        IrType::Inline(InlineIrType::Struct(path, inner_struct)) => (path, inner_struct),
        other => panic!("expected inline struct; got `{other:?}`"),
    };

    // Inner struct path should have correct segments.
    assert_matches!(path.root, InlineIrTypePathRoot::Type("Outer"));
    assert_matches!(
        &*path.segments,
        [
            InlineIrTypePathSegment::Field(IrStructFieldName::Name("items")),
            InlineIrTypePathSegment::ArrayItem,
        ],
    );

    // Inner struct should have correct fields.
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

    let result = transform(&doc, "NullEnum", &schema);

    // `null` values are filtered out from enum variants, producing an enum
    // with zero variants.
    let enum_ = match result {
        IrType::Schema(SchemaIrType::Enum(
            SchemaTypeInfo {
                name: "NullEnum", ..
            },
            enum_,
        )) => enum_,
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
        required: [name]
        properties:
          name:
            type: string
        additionalProperties: false
    "})
    .unwrap();

    let result = transform(&doc, "StrictObject", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "StrictObject",
                ..
            },
            struct_,
        )) => struct_,
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

    let result = transform(&doc, "Container", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "Container", ..
            },
            container,
        )) => container,
        other => panic!("expected container `Container`; got `{other:?}`"),
    };
    let items = match &container {
        Container::Array(Inner { ty, .. }) => ty,
        other => panic!("expected array; got `{other:?}`"),
    };
    let path = match &**items {
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

    let result = transform(&doc, "Dictionary", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "Dictionary", ..
            },
            container,
        )) => container,
        other => panic!("expected container `Dictionary`; got `{other:?}`"),
    };
    let value = match &container {
        Container::Map(Inner { ty, .. }) => ty,
        other => panic!("expected map; got `{other:?}`"),
    };
    let path = match &**value {
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
        required: [nested]
        properties:
          nested:
            type: object
            properties:
              inner:
                type: string
    "})
    .unwrap();

    let result = transform(&doc, "Outer", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Outer", .. }, struct_)) => {
            struct_
        }
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

    let result = transform(&doc, "Container", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "Container", ..
            },
            struct_,
        )) => struct_,
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
    let result = transform(&doc, "Node", schema);

    // Should successfully produce a struct.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Node", .. }, struct_)) => {
            struct_
        }
        other => panic!("expected struct `Node`; got `{other:?}`"),
    };

    // Verify the struct has the expected fields.
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Name("value"),
                ty: IrType::Primitive(PrimitiveIrType::String),
                required: true,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("next"),
                ty: IrType::Inline(InlineIrType::Container(_, Container::Optional(_))),
                required: true,
                ..
            },
        ],
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
    let result = transform(&doc, "Node", schema);

    // Should successfully produce a struct.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Node", .. }, struct_)) => {
            struct_
        }
        other => panic!("expected struct `Node`; got `{other:?}`"),
    };

    // Verify the struct has the expected fields.
    assert_matches!(
        &*struct_.fields,
        [
            IrStructField {
                name: IrStructFieldName::Name("value"),
                ty: IrType::Inline(InlineIrType::Container(_, Container::Optional(_))),
                required: false,
                ..
            },
            IrStructField {
                name: IrStructFieldName::Name("next"),
                ty: IrType::Inline(InlineIrType::Container(_, Container::Optional(_))),
                required: false,
                ..
            },
        ],
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
    let result = transform(&doc, "Node", schema);

    // Should successfully produce a struct.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Node", .. }, struct_)) => {
            struct_
        }
        other => panic!("expected struct `Node`; got `{other:?}`"),
    };

    // The struct should have `value` and `next` fields.
    assert_eq!(struct_.fields.len(), 2);
    assert_matches!(
        &struct_.fields[0],
        IrStructField {
            name: IrStructFieldName::Name("value"),
            ..
        }
    );
    assert_matches!(
        &struct_.fields[1],
        IrStructField {
            name: IrStructFieldName::Name("next"),
            ..
        }
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

    let result = transform(&doc, "StringList", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "StringList", ..
            },
            container,
        )) => container,
        other => panic!("expected container `StringList`; got `{other:?}`"),
    };
    let items = match &container {
        Container::Array(Inner { ty, .. }) => ty,
        other => panic!("expected array; got `{other:?}`"),
    };
    assert_matches!(&**items, IrType::Primitive(PrimitiveIrType::String));
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

    let result = transform(&doc, "Animals", &schema);

    // A named array schema should produce a `Container` for the array,
    // wrapped in a `SchemaIrType` that preserves the schema's identity.
    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "Animals", ..
            },
            container,
        )) => container,
        other => panic!("expected container `Animals`; got `{other:?}`"),
    };
    let items = match &container {
        Container::Array(Inner { ty, .. }) => ty,
        other => panic!("expected array; got `{other:?}`"),
    };
    // The inline `oneOf` should produce an inline untagged union.
    assert_matches!(&**items, IrType::Inline(InlineIrType::Untagged(..)));
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

    let result = transform(&doc, "Metadata", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "Metadata", ..
            },
            container,
        )) => container,
        other => panic!("expected container `Metadata`; got `{other:?}`"),
    };
    assert_matches!(&container, Container::Map(_));
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

    let result = transform(&doc, "NullableString", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "NullableString",
                ..
            },
            container,
        )) => container,
        other => panic!("expected container `NullableString`; got `{other:?}`"),
    };
    let inner = match &container {
        Container::Optional(Inner { ty, .. }) => ty,
        other => panic!("expected optional; got `{other:?}`"),
    };
    assert_matches!(&**inner, IrType::Primitive(PrimitiveIrType::String));
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

    let result = transform(&doc, "Ids", &schema);

    let container = match result {
        IrType::Schema(SchemaIrType::Container(SchemaTypeInfo { name: "Ids", .. }, container)) => {
            container
        }
        other => panic!("expected container `Ids`; got `{other:?}`"),
    };
    let description = match &container {
        Container::Array(Inner { description, .. }) => *description,
        other => panic!("expected array; got `{other:?}`"),
    };
    assert_eq!(description, Some("A list of identifiers"));
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

    let result = transform(&doc, "Name", &schema);

    // Bare primitives should _not_ be wrapped; they don't contain inline types,
    // and don't benefit from a type alias.
    assert_matches!(result, IrType::Primitive(PrimitiveIrType::String));
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

    let result = transform(&doc, "MaybeInner", &schema);

    // A `oneOf` with `null` and a schema reference should produce a
    // `Container(Optional(Ref(...)))`.
    let container = match result {
        IrType::Schema(SchemaIrType::Container(
            SchemaTypeInfo {
                name: "MaybeInner", ..
            },
            container,
        )) => container,
        other => panic!("expected container `MaybeInner`; got `{other:?}`"),
    };
    let inner = match &container {
        Container::Optional(Inner { ty, .. }) => ty,
        other => panic!("expected optional; got `{other:?}`"),
    };
    assert_matches!(&**inner, IrType::Ref(_));
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

    let result = transform(&doc, "Container", &schema);

    // Struct fields that are arrays become inline containers,
    // not schema containers.
    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(
            SchemaTypeInfo {
                name: "Container", ..
            },
            struct_,
        )) => struct_,
        other => panic!("expected struct `Container`; got `{other:?}`"),
    };
    assert_matches!(
        &*struct_.fields,
        [IrStructField {
            name: IrStructFieldName::Name("items"),
            ty: IrType::Inline(InlineIrType::Container(_, Container::Array(_))),
            ..
        }],
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

    let result = transform(&doc, "Parent", &schema);

    let struct_ = match result {
        IrType::Schema(SchemaIrType::Struct(SchemaTypeInfo { name: "Parent", .. }, struct_)) => {
            struct_
        }
        other => panic!("expected struct `Parent`; got `{other:?}`"),
    };
    let [
        IrStructField {
            name: IrStructFieldName::Name("nickname"),
            ty:
                IrType::Inline(InlineIrType::Container(
                    _,
                    Container::Optional(Inner { description, .. }),
                )),
            ..
        },
    ] = &*struct_.fields
    else {
        panic!(
            "expected optional field `nickname`; got `{:?}`",
            struct_.fields
        );
    };
    assert_matches!(description, Some("The nickname"));
}
