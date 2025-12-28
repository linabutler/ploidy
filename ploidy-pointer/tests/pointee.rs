use ploidy_pointer::{JsonPointee, JsonPointer};

#[test]
fn test_rename_field() {
    #[derive(JsonPointee)]
    struct MyStruct {
        #[pointer(rename = "customName")]
        my_field: String,
    }

    let s = MyStruct {
        my_field: "hello".to_owned(),
    };

    // Should be accessible via `"customName"`, not `"my_field"`.
    let pointer = JsonPointer::parse("/customName").unwrap();
    let result = s.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // Original name should not work.
    let pointer = JsonPointer::parse("/my_field").unwrap();
    assert!(s.resolve(pointer).is_err());
}

#[test]
fn test_rename_all_snake_case() {
    #[derive(JsonPointee)]
    #[pointer(rename_all = "snake_case")]
    struct MyStruct {
        my_field: String,
        another_field: i32,
    }

    let s = MyStruct {
        my_field: "hello".to_owned(),
        another_field: 42,
    };

    // Already snake_case, should work as-is.
    let pointer = JsonPointer::parse("/my_field").unwrap();
    assert!(s.resolve(pointer).is_ok());

    let pointer = JsonPointer::parse("/another_field").unwrap();
    assert!(s.resolve(pointer).is_ok());
}

#[test]
fn test_rename_all_camel_case() {
    #[derive(JsonPointee)]
    #[pointer(rename_all = "camelCase")]
    struct MyStruct {
        my_field: String,
        another_field: i32,
    }

    let s = MyStruct {
        my_field: "hello".to_owned(),
        another_field: 42,
    };

    // Should be accessible via camelCase.
    let pointer = JsonPointer::parse("/myField").unwrap();
    let result = s.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    let pointer = JsonPointer::parse("/anotherField").unwrap();
    let result = s.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));

    // Original snake_case should not work.
    let pointer = JsonPointer::parse("/my_field").unwrap();
    assert!(s.resolve(pointer).is_err());
}

#[test]
fn test_rename_all_pascal_case() {
    #[derive(JsonPointee)]
    #[pointer(rename_all = "PascalCase")]
    struct MyStruct {
        my_field: String,
        another_field: i32,
    }

    let s = MyStruct {
        my_field: "hello".to_owned(),
        another_field: 42,
    };

    // Should be accessible via PascalCase.
    let pointer = JsonPointer::parse("/MyField").unwrap();
    assert!(s.resolve(pointer).is_ok());

    let pointer = JsonPointer::parse("/AnotherField").unwrap();
    assert!(s.resolve(pointer).is_ok());
}

#[test]
fn test_rename_all_kebab_case() {
    #[derive(JsonPointee)]
    #[pointer(rename_all = "kebab-case")]
    struct MyStruct {
        my_field: String,
        another_field: i32,
    }

    let s = MyStruct {
        my_field: "hello".to_owned(),
        another_field: 42,
    };

    // Should be accessible via kebab-case.
    let pointer = JsonPointer::parse("/my-field").unwrap();
    assert!(s.resolve(pointer).is_ok());

    let pointer = JsonPointer::parse("/another-field").unwrap();
    assert!(s.resolve(pointer).is_ok());
}

#[test]
fn test_rename_overrides_rename_all() {
    #[derive(JsonPointee)]
    #[pointer(rename_all = "camelCase")]
    struct MyStruct {
        #[pointer(rename = "customName")]
        my_field: String,
        another_field: i32,
    }

    let s = MyStruct {
        my_field: "hello".to_owned(),
        another_field: 42,
    };

    // `my_field` should use the explicit rename.
    let pointer = JsonPointer::parse("/customName").unwrap();
    assert!(s.resolve(pointer).is_ok());

    // `another_field` should use `rename_all` (camelCase).
    let pointer = JsonPointer::parse("/anotherField").unwrap();
    assert!(s.resolve(pointer).is_ok());

    // Neither should work with the original names.
    let pointer = JsonPointer::parse("/my_field").unwrap();
    assert!(s.resolve(pointer).is_err());

    let pointer = JsonPointer::parse("/another_field").unwrap();
    assert!(s.resolve(pointer).is_err());
}

#[test]
fn test_enum_with_rename() {
    #[derive(JsonPointee)]
    #[pointer(untagged, rename_all = "camelCase")]
    enum MyEnum {
        VariantA { my_field: String },
        VariantB { another_field: i32 },
    }

    let e = MyEnum::VariantA {
        my_field: "hello".to_owned(),
    };

    // Should be accessible via camelCase.
    let pointer = JsonPointer::parse("/myField").unwrap();
    let result = e.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // Original name should not work.
    let pointer = JsonPointer::parse("/my_field").unwrap();
    assert!(e.resolve(pointer).is_err());

    let e = MyEnum::VariantB { another_field: 123 };

    let pointer = JsonPointer::parse("/anotherField").unwrap();
    let result = e.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&123));
}

#[test]
fn test_flatten_field() {
    #[derive(JsonPointee)]
    struct Inner {
        inner_field: String,
    }

    #[derive(JsonPointee)]
    struct Outer {
        #[pointer(flatten)]
        inner: Inner,
    }

    let outer = Outer {
        inner: Inner {
            inner_field: "hello".to_owned(),
        },
    };

    // Should be able to access `inner_field` directly through the flattened field.
    let pointer = JsonPointer::parse("/inner_field").unwrap();
    let result = outer.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));
}

#[test]
fn test_multiple_flattened_fields() {
    #[derive(JsonPointee)]
    struct Inner1 {
        field1: String,
    }

    #[derive(JsonPointee)]
    struct Inner2 {
        field2: i32,
    }

    #[derive(JsonPointee)]
    struct Outer {
        #[pointer(flatten)]
        inner1: Inner1,
        #[pointer(flatten)]
        inner2: Inner2,
    }

    let outer = Outer {
        inner1: Inner1 {
            field1: "hello".to_owned(),
        },
        inner2: Inner2 { field2: 42 },
    };

    // Should access `field1` from first flattened field.
    let pointer = JsonPointer::parse("/field1").unwrap();
    let result = outer.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // Should access `field2` from second flattened field.
    let pointer = JsonPointer::parse("/field2").unwrap();
    let result = outer.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));
}

#[test]
fn test_priority_regular_over_flattened() {
    #[derive(JsonPointee)]
    struct Inner {
        my_field: String,
    }

    #[derive(JsonPointee)]
    struct Outer {
        my_field: i32, // Regular field with same name
        #[pointer(flatten)]
        inner: Inner,
    }

    let outer = Outer {
        my_field: 42,
        inner: Inner {
            my_field: "hello".to_owned(),
        },
    };

    // Should access the regular field, not the flattened one.
    let pointer = JsonPointer::parse("/my_field").unwrap();
    let result = outer.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));
}

#[test]
fn test_all_flattened_fields() {
    #[derive(JsonPointee)]
    struct Inner1 {
        field1: String,
    }

    #[derive(JsonPointee)]
    struct Inner2 {
        field2: i32,
    }

    #[derive(JsonPointee)]
    struct Outer {
        #[pointer(flatten)]
        inner1: Inner1,
        #[pointer(flatten)]
        inner2: Inner2,
    }

    let outer = Outer {
        inner1: Inner1 {
            field1: "hello".to_owned(),
        },
        inner2: Inner2 { field2: 42 },
    };

    // Both fields should be accessible.
    let pointer = JsonPointer::parse("/field1").unwrap();
    assert_eq!(
        outer
            .resolve(pointer)
            .unwrap()
            .downcast_ref::<String>()
            .unwrap()
            .clone(),
        "hello".to_owned()
    );

    let pointer = JsonPointer::parse("/field2").unwrap();
    assert_eq!(
        outer
            .resolve(pointer)
            .unwrap()
            .downcast_ref::<i32>()
            .copied()
            .unwrap(),
        42,
    );
}

#[test]
fn test_nested_flattening() {
    #[derive(JsonPointee)]
    struct Deep {
        deep_field: String,
    }

    #[derive(JsonPointee)]
    struct Middle {
        #[pointer(flatten)]
        deep: Deep,
    }

    #[derive(JsonPointee)]
    struct Outer {
        #[pointer(flatten)]
        middle: Middle,
    }

    let outer = Outer {
        middle: Middle {
            deep: Deep {
                deep_field: "hello".to_owned(),
            },
        },
    };

    // Should be able to access `deep_field` through nested flattening.
    let pointer = JsonPointer::parse("/deep_field").unwrap();
    let result = outer.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));
}

#[test]
fn test_flatten_error_not_found() {
    #[derive(JsonPointee)]
    struct Inner {
        inner_field: String,
    }

    #[derive(JsonPointee)]
    struct Outer {
        regular_field: i32,
        #[pointer(flatten)]
        inner: Inner,
    }

    let outer = Outer {
        regular_field: 42,
        inner: Inner {
            inner_field: "hello".to_owned(),
        },
    };

    // Try to access a field that doesn't exist anywhere.
    let pointer = JsonPointer::parse("/nonexistent").unwrap();
    assert!(outer.resolve(pointer).is_err());
}

#[test]
fn test_enum_variant_flatten() {
    #[derive(JsonPointee)]
    struct Inner {
        inner_field: String,
    }

    #[derive(JsonPointee)]
    #[pointer(untagged)]
    enum MyEnum {
        VariantA {
            regular_field: i32,
            #[pointer(flatten)]
            inner: Inner,
        },
    }

    let e = MyEnum::VariantA {
        regular_field: 42,
        inner: Inner {
            inner_field: "hello".to_owned(),
        },
    };

    // Should access regular field normally.
    let pointer = JsonPointer::parse("/regular_field").unwrap();
    let result = e.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));

    // Should access `inner_field` through flattened field.
    let pointer = JsonPointer::parse("/inner_field").unwrap();
    let result = e.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));
}

#[test]
fn test_flatten_with_rename_all() {
    #[derive(JsonPointee)]
    #[pointer(rename_all = "camelCase")]
    struct Inner {
        inner_field: String,
    }

    #[derive(JsonPointee)]
    #[pointer(rename_all = "camelCase")]
    struct Outer {
        regular_field: i32,
        #[pointer(flatten)]
        inner: Inner,
    }

    let outer = Outer {
        regular_field: 42,
        inner: Inner {
            inner_field: "hello".to_owned(),
        },
    };

    // Regular field should use camelCase.
    let pointer = JsonPointer::parse("/regularField").unwrap();
    let result = outer.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));

    // Flattened field's fields should also use camelCase.
    let pointer = JsonPointer::parse("/innerField").unwrap();
    let result = outer.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));
}

#[test]
fn test_flatten_order_matters() {
    #[derive(JsonPointee)]
    struct Inner1 {
        shared_field: String,
    }

    #[derive(JsonPointee)]
    struct Inner2 {
        shared_field: i32,
    }

    #[derive(JsonPointee)]
    struct Outer {
        #[pointer(flatten)]
        inner1: Inner1,
        #[pointer(flatten)]
        inner2: Inner2,
    }

    let outer = Outer {
        inner1: Inner1 {
            shared_field: "hello".to_owned(),
        },
        inner2: Inner2 { shared_field: 42 },
    };

    // Should resolve to the first flattened field's value.
    let pointer = JsonPointer::parse("/shared_field").unwrap();
    let result = outer.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));
}

#[test]
#[cfg(feature = "chrono")]
fn test_pointer_to_chrono_datetime() {
    use chrono::{DateTime, Utc};

    let timestamp: DateTime<Utc> = "2024-01-15T10:30:00Z".parse().unwrap();

    // Empty path should return the timestamp itself.
    let pointer = JsonPointer::parse("").unwrap();
    let result = timestamp.resolve(pointer).unwrap();
    assert!(result.downcast_ref::<DateTime<Utc>>().is_some());

    // Non-empty path should fail.
    let pointer = JsonPointer::parse("/foo").unwrap();
    assert!(timestamp.resolve(pointer).is_err());
}

#[test]
#[cfg(feature = "url")]
fn test_pointer_to_url() {
    use url::Url;

    let url = Url::parse("https://example.com/path?query=value").unwrap();

    // Empty path should return the URL itself.
    let pointer = JsonPointer::parse("").unwrap();
    let result = url.resolve(pointer).unwrap();
    assert!(result.downcast_ref::<Url>().is_some());

    // Non-empty path should fail.
    let pointer = JsonPointer::parse("/foo").unwrap();
    assert!(url.resolve(pointer).is_err());
}

#[test]
#[cfg(feature = "serde_json")]
fn test_pointer_to_serde_json() {
    use serde_json::json;

    let data = json!({
        "name": "Alice",
        "age": 30,
        "items": [1, 2, 3],
        "nested": {
            "field": "value"
        }
    });

    // Test object field access.
    let pointer = JsonPointer::parse("/name").unwrap();
    let result = data.resolve(pointer).unwrap();
    assert!(result.downcast_ref::<serde_json::Value>().is_some());

    // Test array index access.
    let pointer = JsonPointer::parse("/items/1").unwrap();
    let result = data.resolve(pointer).unwrap();
    assert!(result.downcast_ref::<serde_json::Value>().is_some());

    // Test nested object access.
    let pointer = JsonPointer::parse("/nested/field").unwrap();
    let result = data.resolve(pointer).unwrap();
    assert!(result.downcast_ref::<serde_json::Value>().is_some());

    // Test empty path returns the whole value.
    let pointer = JsonPointer::parse("").unwrap();
    let result = data.resolve(pointer).unwrap();
    assert!(result.downcast_ref::<serde_json::Value>().is_some());

    // Test non-existent key.
    let pointer = JsonPointer::parse("/nonexistent").unwrap();
    assert!(data.resolve(pointer).is_err());

    // Test out of bounds array index.
    let pointer = JsonPointer::parse("/items/10").unwrap();
    assert!(data.resolve(pointer).is_err());
}

#[test]
#[cfg(feature = "indexmap")]
fn test_indexmap() {
    use indexmap::IndexMap;

    let mut map = IndexMap::new();
    map.insert("first".to_string(), 10);
    map.insert("second".to_string(), 20);
    map.insert("third".to_string(), 30);

    // Test accessing values.
    let pointer = JsonPointer::parse("/first").unwrap();
    let result = map.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&10));

    let pointer = JsonPointer::parse("/second").unwrap();
    let result = map.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&20));

    // Test empty path returns the map itself.
    let pointer = JsonPointer::parse("").unwrap();
    let result = map.resolve(pointer).unwrap();
    assert!(result.downcast_ref::<IndexMap<String, i32>>().is_some());

    // Test non-existent key.
    let pointer = JsonPointer::parse("/nonexistent").unwrap();
    assert!(map.resolve(pointer).is_err());
}

#[test]
fn test_skip_field() {
    #[derive(JsonPointee)]
    struct MyStruct {
        visible: String,
        #[pointer(skip)]
        hidden: String,
    }

    let s = MyStruct {
        visible: "hello".to_owned(),
        hidden: "secret".to_owned(),
    };

    // `visible` field should be accessible.
    let pointer = JsonPointer::parse("/visible").unwrap();
    let result = s.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // `hidden` field should NOT be accessible.
    let pointer = JsonPointer::parse("/hidden").unwrap();
    assert!(s.resolve(pointer).is_err());

    // Empty path should still resolve to the struct itself.
    let pointer = JsonPointer::parse("").unwrap();
    assert!(s.resolve(pointer).is_ok());
}

#[test]
fn test_skip_not_in_suggestions() {
    #[derive(JsonPointee)]
    struct MyStruct {
        visible: String,
        #[pointer(skip)]
        hidden: String,
    }

    let s = MyStruct {
        visible: "hello".to_owned(),
        hidden: "secret".to_owned(),
    };

    // Try accessing a non-existent field - error should suggest "visible", not "hidden".
    let pointer = JsonPointer::parse("/nonexistent").unwrap();
    match s.resolve(pointer) {
        Err(ploidy_pointer::BadJsonPointer::Key(key_err)) => {
            // Suggestion should be `"visible"`, not `"hidden"`.
            assert_eq!(key_err.suggestion.as_deref(), Some("visible"));
        }
        _ => panic!("Expected BadJsonPointer::Key error"),
    }
}

#[test]
fn test_skip_with_rename_all() {
    #[derive(JsonPointee)]
    #[pointer(rename_all = "camelCase")]
    struct MyStruct {
        my_field: String,
        #[pointer(skip)]
        hidden_field: String,
    }

    let s = MyStruct {
        my_field: "hello".to_owned(),
        hidden_field: "secret".to_owned(),
    };

    // `my_field` should be accessible as camelCase.
    let pointer = JsonPointer::parse("/myField").unwrap();
    let result = s.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // `hidden_field` should NOT be accessible (even as `hiddenField`).
    let pointer = JsonPointer::parse("/hiddenField").unwrap();
    assert!(s.resolve(pointer).is_err());

    let pointer = JsonPointer::parse("/hidden_field").unwrap();
    assert!(s.resolve(pointer).is_err());
}

#[test]
fn test_multiple_skip_fields() {
    #[derive(JsonPointee)]
    struct MyStruct {
        visible1: String,
        #[pointer(skip)]
        hidden1: String,
        visible2: i32,
        #[pointer(skip)]
        hidden2: i32,
    }

    let s = MyStruct {
        visible1: "hello".to_owned(),
        hidden1: "secret1".to_owned(),
        visible2: 42,
        hidden2: 99,
    };

    // Both visible fields accessible.
    let pointer = JsonPointer::parse("/visible1").unwrap();
    assert!(s.resolve(pointer).is_ok());

    let pointer = JsonPointer::parse("/visible2").unwrap();
    assert!(s.resolve(pointer).is_ok());

    // Both hidden fields inaccessible.
    let pointer = JsonPointer::parse("/hidden1").unwrap();
    assert!(s.resolve(pointer).is_err());

    let pointer = JsonPointer::parse("/hidden2").unwrap();
    assert!(s.resolve(pointer).is_err());
}

#[test]
fn test_skip_and_flatten_on_different_fields() {
    #[derive(JsonPointee)]
    struct Inner {
        inner_field: String,
    }

    #[derive(JsonPointee)]
    struct Outer {
        #[pointer(skip)]
        hidden: String,
        #[pointer(flatten)]
        inner: Inner,
        visible: i32,
    }

    let outer = Outer {
        hidden: "secret".to_owned(),
        inner: Inner {
            inner_field: "hello".to_owned(),
        },
        visible: 42,
    };

    // Flattened field's content should be accessible.
    let pointer = JsonPointer::parse("/inner_field").unwrap();
    let result = outer.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // Regular visible field accessible.
    let pointer = JsonPointer::parse("/visible").unwrap();
    let result = outer.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));

    // Hidden field not accessible.
    let pointer = JsonPointer::parse("/hidden").unwrap();
    assert!(outer.resolve(pointer).is_err());
}

#[test]
fn test_skip_in_enum_variant() {
    #[derive(JsonPointee)]
    #[pointer(untagged)]
    enum MyEnum {
        VariantA {
            visible: String,
            #[pointer(skip)]
            hidden: String,
        },
    }

    let e = MyEnum::VariantA {
        visible: "hello".to_owned(),
        hidden: "secret".to_owned(),
    };

    let pointer = JsonPointer::parse("/visible").unwrap();
    let result = e.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    let pointer = JsonPointer::parse("/hidden").unwrap();
    assert!(e.resolve(pointer).is_err());
}

#[test]
fn test_skip_in_tuple_struct() {
    #[derive(JsonPointee)]
    struct MyTuple(String, #[pointer(skip)] String, i32);

    let t = MyTuple("hello".to_owned(), "secret".to_owned(), 42);

    // Index 0 accessible.
    let pointer = JsonPointer::parse("/0").unwrap();
    let result = t.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // Index 1 NOT accessible (skipped).
    let pointer = JsonPointer::parse("/1").unwrap();
    assert!(t.resolve(pointer).is_err());

    // Index 2 accessible.
    let pointer = JsonPointer::parse("/2").unwrap();
    let result = t.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));
}

#[test]
fn test_all_fields_skipped() {
    #[derive(JsonPointee)]
    struct AllHidden {
        #[pointer(skip)]
        field1: String,
        #[pointer(skip)]
        field2: i32,
    }

    let s = AllHidden {
        field1: "secret".to_owned(),
        field2: 42,
    };

    // Empty path should still resolve to self.
    let pointer = JsonPointer::parse("").unwrap();
    assert!(s.resolve(pointer).is_ok());

    // No fields accessible.
    let pointer = JsonPointer::parse("/field1").unwrap();
    assert!(s.resolve(pointer).is_err());

    let pointer = JsonPointer::parse("/field2").unwrap();
    assert!(s.resolve(pointer).is_err());
}

#[test]
fn test_skip_unit_variant() {
    #[derive(JsonPointee)]
    #[pointer(untagged)]
    enum MyEnum {
        Active,
        #[pointer(skip)]
        Inactive,
    }

    // Skipped variant should fail.
    let e = MyEnum::Inactive;
    let pointer = JsonPointer::parse("").unwrap();
    assert!(e.resolve(pointer).is_err());

    // Non-skipped variant should work.
    let e = MyEnum::Active;
    let pointer = JsonPointer::parse("").unwrap();
    assert!(e.resolve(pointer).is_ok());
}

#[test]
fn test_skip_newtype_variant() {
    #[derive(JsonPointee)]
    #[pointer(untagged)]
    #[allow(dead_code)]
    enum MyEnum {
        Value(String),
        #[pointer(skip)]
        Ref(String),
    }

    // Skipped variant should fail for any pointer.
    let e = MyEnum::Ref("test".to_owned());
    let pointer = JsonPointer::parse("").unwrap();
    assert!(e.resolve(pointer).is_err());

    // Non-skipped newtype variant should transparently resolve to inner value.
    let e = MyEnum::Value("hello".to_owned());
    let pointer = JsonPointer::parse("").unwrap();
    let result = e.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));
}

#[test]
fn test_skip_struct_variant() {
    #[derive(JsonPointee)]
    #[pointer(untagged)]
    #[allow(dead_code)]
    enum MyEnum {
        Active {
            field: String,
        },
        #[pointer(skip)]
        Inactive {
            field: String,
        },
    }

    // Skipped variant should fail for field access.
    let e = MyEnum::Inactive {
        field: "test".to_owned(),
    };
    let pointer = JsonPointer::parse("/field").unwrap();
    assert!(e.resolve(pointer).is_err());

    // Even empty pointer should fail.
    let pointer = JsonPointer::parse("").unwrap();
    assert!(e.resolve(pointer).is_err());

    // Non-skipped variant should allow field access.
    let e = MyEnum::Active {
        field: "hello".to_owned(),
    };
    let pointer = JsonPointer::parse("/field").unwrap();
    let result = e.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));
}

#[test]
fn test_multiple_variants_with_skip() {
    #[derive(JsonPointee)]
    #[pointer(untagged)]
    #[allow(dead_code)]
    enum Status {
        Active {
            count: i32,
        },
        #[pointer(skip)]
        Pending,
        #[pointer(skip)]
        Deleted {
            reason: String,
        },
        Archived {
            date: String,
        },
    }

    // Active works.
    let s = Status::Active { count: 42 };
    assert!(s.resolve(JsonPointer::parse("/count").unwrap()).is_ok());

    // Pending blocked.
    let s = Status::Pending;
    assert!(s.resolve(JsonPointer::parse("").unwrap()).is_err());

    // Deleted blocked - both empty pointer and field access.
    let s = Status::Deleted {
        reason: "test".to_owned(),
    };
    assert!(s.resolve(JsonPointer::parse("").unwrap()).is_err());
    assert!(s.resolve(JsonPointer::parse("/reason").unwrap()).is_err());

    // Archived works.
    let s = Status::Archived {
        date: "2024".to_owned(),
    };
    assert!(s.resolve(JsonPointer::parse("/date").unwrap()).is_ok());
}

#[test]
fn test_generic_type_with_bounds() {
    // Test that the derive macro correctly generates `JsonPointee` bounds for
    // generic type parameters. This mirrors the `RefOr<T>` type in the main
    // codebase.
    #[derive(JsonPointee)]
    #[pointer(untagged)]
    enum GenericWrapper<T> {
        Value(T),
        None,
    }

    // Test with a concrete type that implements `JsonPointee`.
    #[derive(JsonPointee)]
    struct Inner {
        field: String,
    }

    let wrapped = GenericWrapper::Value(Inner {
        field: "hello".to_owned(),
    });

    // Empty pointer should resolve to the enum itself.
    let pointer = JsonPointer::parse("").unwrap();
    assert!(wrapped.resolve(pointer).is_ok());

    // Should be able to resolve into the wrapped value's fields.
    let pointer = JsonPointer::parse("/field").unwrap();
    let result = wrapped.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // Test the None variant.
    let wrapped: GenericWrapper<Inner> = GenericWrapper::None;
    let pointer = JsonPointer::parse("").unwrap();
    assert!(wrapped.resolve(pointer).is_ok());
}

#[test]
fn test_generic_struct_with_bounds() {
    // Test generic struct to ensure bounds work for both enums and structs.
    #[derive(JsonPointee)]
    struct Container<T> {
        value: T,
        name: String,
    }

    #[derive(JsonPointee)]
    struct Item {
        id: i32,
    }

    let container = Container {
        value: Item { id: 42 },
        name: "test".to_owned(),
    };

    // Access the name field.
    let pointer = JsonPointer::parse("/name").unwrap();
    let result = container.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"test".to_owned()));

    // Access nested field through the generic type.
    let pointer = JsonPointer::parse("/value/id").unwrap();
    let result = container.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));
}

#[test]
fn test_multiple_generic_parameters_with_bounds() {
    // Test that bounds are correctly generated for multiple type parameters.
    #[derive(JsonPointee)]
    struct Pair<A, B> {
        first: A,
        second: B,
    }

    #[derive(JsonPointee)]
    struct Left {
        left_value: String,
    }

    #[derive(JsonPointee)]
    struct Right {
        right_value: i32,
    }

    let pair = Pair {
        first: Left {
            left_value: "left".to_owned(),
        },
        second: Right { right_value: 100 },
    };

    // Access first generic parameter's field.
    let pointer = JsonPointer::parse("/first/left_value").unwrap();
    let result = pair.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"left".to_owned()));

    // Access second generic parameter's field.
    let pointer = JsonPointer::parse("/second/right_value").unwrap();
    let result = pair.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&100));
}
