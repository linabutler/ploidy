use ploidy_pointer::{JsonPointee, JsonPointer};

#[test]
fn test_basic_tag_named_variants() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type")]
    enum Response {
        Success { data: String },
        Error { code: i32 },
    }

    let response = Response::Success {
        data: "hello".to_owned(),
    };

    // Tag field should return variant name.
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Success"));

    // Regular field should still be accessible.
    let pointer = JsonPointer::parse("/data").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    let response = Response::Error { code: 404 };
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Error"));

    let pointer = JsonPointer::parse("/code").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&404));
}

#[test]
fn test_tag_with_rename_all() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type", rename_all = "camelCase")]
    enum Response {
        SuccessResponse {
            data: String,
        },
        #[allow(dead_code)]
        ErrorResponse {
            error_code: i32,
        },
    }

    let response = Response::SuccessResponse {
        data: "hello".to_owned(),
    };

    // Tag should return camelCase-transformed variant name.
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"successResponse"));
}

#[test]
fn test_tag_with_variant_rename() {
    #[derive(JsonPointee)]
    #[pointer(tag = "kind")]
    enum Message {
        #[pointer(rename = "success")]
        Success { text: String },
        #[pointer(rename = "error")]
        Error { message: String },
    }

    let msg = Message::Success {
        text: "ok".to_owned(),
    };

    // Tag should return the explicit rename.
    let pointer = JsonPointer::parse("/kind").unwrap();
    let result = msg.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"success"));

    let msg = Message::Error {
        message: "fail".to_owned(),
    };
    let pointer = JsonPointer::parse("/kind").unwrap();
    let result = msg.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"error"));
}

#[test]
fn test_tag_with_unit_variants() {
    #[derive(JsonPointee)]
    #[pointer(tag = "status")]
    enum Status {
        Pending,
        #[allow(dead_code)]
        InProgress,
        #[allow(dead_code)]
        Complete,
    }

    let status = Status::Pending;

    // Tag is the only accessible field for unit variants.
    let pointer = JsonPointer::parse("/status").unwrap();
    let result = status.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Pending"));

    // Any other field should error.
    let pointer = JsonPointer::parse("/other").unwrap();
    assert!(status.resolve(pointer).is_err());

    // Empty pointer should return the variant itself.
    let pointer = JsonPointer::parse("").unwrap();
    assert!(status.resolve(pointer).is_ok());
}

#[test]
fn test_tag_with_newtype_variants() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type")]
    enum Wrapper {
        String(String),
        #[allow(dead_code)]
        Number(i32),
    }

    let wrapper = Wrapper::String("hello".to_owned());

    // Empty pointer should return the enum variant itself, not inner value.
    let pointer = JsonPointer::parse("").unwrap();
    let result = wrapper.resolve(pointer).unwrap();
    // Should return the enum, not the inner `String`.
    assert!(result.downcast_ref::<String>().is_none());

    // Tag field should be accessible.
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = wrapper.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"String"));
}

#[test]
fn test_tag_with_tuple_variants() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type")]
    enum Data {
        Pair(String, i32),
        #[allow(dead_code)]
        Triple(String, i32, bool),
    }

    let data = Data::Pair("test".to_owned(), 42);

    // Tag should be accessible.
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = data.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Pair"));

    // Tuple indices should still work.
    let pointer = JsonPointer::parse("/0").unwrap();
    let result = data.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"test".to_owned()));

    let pointer = JsonPointer::parse("/1").unwrap();
    let result = data.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));
}

#[test]
fn test_backward_compatibility_without_tag() {
    #[derive(JsonPointee)]
    #[pointer(untagged)]
    enum Response {
        Success {
            data: String,
        },
        #[allow(dead_code)]
        Error {
            code: i32,
        },
    }

    let response = Response::Success {
        data: "hello".to_owned(),
    };

    // Without tag, should work as before.
    let pointer = JsonPointer::parse("/data").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // No tag field should be accessible.
    let pointer = JsonPointer::parse("/type").unwrap();
    assert!(response.resolve(pointer).is_err());
}

#[test]
fn test_tag_with_skipped_variants() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type")]
    enum Response {
        #[allow(dead_code)]
        Success { data: String },
        #[pointer(skip)]
        #[allow(dead_code)]
        Internal { secret: String },
    }

    let response = Response::Internal {
        secret: "hidden".to_owned(),
    };

    // Tag field should be accessible even for skipped variants.
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Internal"));

    // Other fields should be inaccessible.
    let pointer = JsonPointer::parse("/secret").unwrap();
    assert!(response.resolve(pointer).is_err());

    // Empty pointer should return self.
    let pointer = JsonPointer::parse("").unwrap();
    assert!(response.resolve(pointer).is_ok());
}

#[test]
fn test_tag_error_suggestions() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type")]
    enum Response {
        Success { data: String },
    }

    let response = Response::Success {
        data: "hello".to_owned(),
    };

    // Invalid field should error.
    let pointer = JsonPointer::parse("/invalid").unwrap();
    assert!(response.resolve(pointer).is_err());
}

#[test]
fn test_tag_with_mixed_variant_types() {
    #[derive(JsonPointee)]
    #[pointer(tag = "kind")]
    enum Mixed {
        Unit,
        Newtype(String),
        Tuple(i32, i32),
        Struct { x: i32, y: i32 },
    }

    // Unit variant.
    let m = Mixed::Unit;
    let pointer = JsonPointer::parse("/kind").unwrap();
    let result = m.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Unit"));

    // Newtype variant.
    let m = Mixed::Newtype("test".to_owned());
    let pointer = JsonPointer::parse("/kind").unwrap();
    let result = m.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Newtype"));

    // Tuple variant.
    let m = Mixed::Tuple(1, 2);
    let pointer = JsonPointer::parse("/kind").unwrap();
    let result = m.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Tuple"));
    let pointer = JsonPointer::parse("/0").unwrap();
    let result = m.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&1));

    // Struct variant.
    let m = Mixed::Struct { x: 10, y: 20 };
    let pointer = JsonPointer::parse("/kind").unwrap();
    let result = m.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Struct"));
    let pointer = JsonPointer::parse("/x").unwrap();
    let result = m.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&10));
}

#[test]
fn test_tag_priority_explicit_rename_over_rename_all() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
    enum Response {
        #[pointer(rename = "custom_success")]
        Success {
            data: String,
        },
        Error {
            code: i32,
        },
    }

    let response = Response::Success {
        data: "hello".to_owned(),
    };

    // Explicit rename should take priority.
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"custom_success"));

    // Error variant should use `rename_all`.
    let response = Response::Error { code: 404 };
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"ERROR"));
}

#[test]
fn test_newtype_variant_empty_pointer_returns_enum() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type")]
    enum Container {
        Value(String),
    }

    let container = Container::Value("test".to_owned());

    // Empty pointer should return the enum variant, not the inner string.
    let pointer = JsonPointer::parse("").unwrap();
    let result = container.resolve(pointer).unwrap();
    assert!(result.is::<Container>());
}

#[test]
fn test_untagged_newtype_transparent() {
    #[derive(JsonPointee)]
    #[pointer(untagged)]
    enum Container {
        Value(String),
    }

    let container = Container::Value("test".to_owned());

    // Without tag, newtype variants are transparent.
    let pointer = JsonPointer::parse("").unwrap();
    let result = container.resolve(pointer).unwrap();

    // Should give us the inner `String` directly.
    assert_eq!(result.downcast_ref::<String>(), Some(&"test".to_owned()));
}

#[test]
fn test_external_tag_named_variants() {
    #[derive(JsonPointee)]
    enum Response {
        Success { data: String },
        Error { code: i32 },
    }

    let response = Response::Success {
        data: "hello".to_owned(),
    };

    // Empty pointer returns self.
    let pointer = JsonPointer::parse("").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert!(result.is::<Response>());

    // First segment must be variant name.
    let pointer = JsonPointer::parse("/Success").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert!(result.is::<Response>());

    // Access field through variant wrapper.
    let pointer = JsonPointer::parse("/Success/data").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // Wrong variant name should error.
    let pointer = JsonPointer::parse("/Error/data").unwrap();
    assert!(response.resolve(pointer).is_err());

    let response = Response::Error { code: 404 };
    let pointer = JsonPointer::parse("/Error/code").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&404));
}

#[test]
fn test_external_tag_tuple_variants() {
    #[derive(JsonPointee)]
    enum Value {
        #[allow(dead_code)]
        Single(String),
        Pair(i32, String),
    }

    let value = Value::Pair(42, "test".to_owned());

    // Access through variant wrapper and index.
    let pointer = JsonPointer::parse("/Pair/0").unwrap();
    let result = value.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));

    let pointer = JsonPointer::parse("/Pair/1").unwrap();
    let result = value.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"test".to_owned()));

    // Wrong variant name should error.
    let pointer = JsonPointer::parse("/Single/0").unwrap();
    assert!(value.resolve(pointer).is_err());
}

#[test]
fn test_external_tag_unit_variants() {
    #[derive(JsonPointee)]
    enum Status {
        Pending,
        #[allow(dead_code)]
        InProgress,
        #[allow(dead_code)]
        Complete,
    }

    let status = Status::Pending;

    // Variant name is accessible.
    let pointer = JsonPointer::parse("/Pending").unwrap();
    let result = status.resolve(pointer).unwrap();
    assert!(result.is::<Status>());

    // Any further navigation should error.
    let pointer = JsonPointer::parse("/Pending/foo").unwrap();
    assert!(status.resolve(pointer).is_err());

    // Wrong variant name should error.
    let pointer = JsonPointer::parse("/Complete").unwrap();
    assert!(status.resolve(pointer).is_err());
}

#[test]
fn test_external_tag_newtype_variants() {
    #[derive(JsonPointee)]
    enum Wrapper {
        Text(String),
        #[allow(dead_code)]
        Number(i32),
    }

    let wrapper = Wrapper::Text("hello".to_owned());

    // Access through variant name wrapper.
    let pointer = JsonPointer::parse("/Text").unwrap();
    let result = wrapper.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    // Wrong variant should error.
    let pointer = JsonPointer::parse("/Number").unwrap();
    assert!(wrapper.resolve(pointer).is_err());
}

#[test]
fn test_external_tag_with_rename_all() {
    #[derive(JsonPointee)]
    #[pointer(rename_all = "snake_case")]
    enum Response {
        SuccessResponse {
            data: String,
        },
        #[allow(dead_code)]
        ErrorResponse {
            error_code: i32,
        },
    }

    let response = Response::SuccessResponse {
        data: "hello".to_owned(),
    };

    // Variant name should be snake_case.
    let pointer = JsonPointer::parse("/success_response/data").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));
}

#[test]
fn test_external_tag_with_variant_rename() {
    #[derive(JsonPointee)]
    enum Message {
        #[pointer(rename = "ok")]
        Success { text: String },
        #[allow(dead_code)]
        #[pointer(rename = "err")]
        Error { message: String },
    }

    let msg = Message::Success {
        text: "good".to_owned(),
    };

    // Should use explicit rename.
    let pointer = JsonPointer::parse("/ok/text").unwrap();
    let result = msg.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"good".to_owned()));
}

#[test]
fn test_external_tag_mixed_variants() {
    #[derive(JsonPointee)]
    enum Mixed {
        Unit,
        Named { value: String },
        Tuple(i32, i32),
        Newtype(String),
    }

    let unit = Mixed::Unit;
    let pointer = JsonPointer::parse("/Unit").unwrap();
    assert!(unit.resolve(pointer).is_ok());

    let named = Mixed::Named {
        value: "test".to_owned(),
    };
    let pointer = JsonPointer::parse("/Named/value").unwrap();
    let result = named.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"test".to_owned()));

    let tuple = Mixed::Tuple(1, 2);
    let pointer = JsonPointer::parse("/Tuple/0").unwrap();
    let result = tuple.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&1));

    let newtype = Mixed::Newtype("wrapped".to_owned());
    let pointer = JsonPointer::parse("/Newtype").unwrap();
    let result = newtype.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"wrapped".to_owned()));
}

#[test]
fn test_adjacent_tag_named_variants() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type", content = "value")]
    enum Response {
        Success { data: String },
        Error { code: i32 },
    }

    let response = Response::Success {
        data: "hello".to_owned(),
    };

    // Tag field should return variant name.
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Success"));

    // Content field should contain the data.
    let pointer = JsonPointer::parse("/value/data").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));

    let response = Response::Error { code: 404 };
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Error"));

    let pointer = JsonPointer::parse("/value/code").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&404));
}

#[test]
fn test_adjacent_tag_tuple_variants() {
    #[derive(JsonPointee)]
    #[pointer(tag = "t", content = "c")]
    enum Value {
        #[allow(dead_code)]
        Single(String),
        Pair(i32, String),
    }

    let value = Value::Pair(42, "test".to_owned());

    // Tag field.
    let pointer = JsonPointer::parse("/t").unwrap();
    let result = value.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Pair"));

    // Content with index.
    let pointer = JsonPointer::parse("/c/0").unwrap();
    let result = value.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&42));

    let pointer = JsonPointer::parse("/c/1").unwrap();
    let result = value.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"test".to_owned()));
}

#[test]
fn test_adjacent_tag_unit_variants() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type", content = "data")]
    enum Status {
        Pending,
        #[allow(dead_code)]
        InProgress,
        #[allow(dead_code)]
        Complete,
    }

    let status = Status::Pending;

    // Tag field is accessible.
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = status.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Pending"));

    // Content field should error for unit variants.
    let pointer = JsonPointer::parse("/data").unwrap();
    assert!(status.resolve(pointer).is_err());
}

#[test]
fn test_adjacent_tag_newtype_variants() {
    #[derive(JsonPointee)]
    #[pointer(tag = "kind", content = "payload")]
    enum Wrapper {
        Text(String),
        #[allow(dead_code)]
        Number(i32),
    }

    let wrapper = Wrapper::Text("hello".to_owned());

    // Tag field.
    let pointer = JsonPointer::parse("/kind").unwrap();
    let result = wrapper.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Text"));

    // Content field delegates to inner value.
    let pointer = JsonPointer::parse("/payload").unwrap();
    let result = wrapper.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"hello".to_owned()));
}

#[test]
fn test_adjacent_tag_with_rename_all() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type", content = "value", rename_all = "SCREAMING_SNAKE_CASE")]
    enum Response {
        SuccessResponse {
            data: String,
        },
        #[allow(dead_code)]
        ErrorResponse {
            error_code: i32,
        },
    }

    let response = Response::SuccessResponse {
        data: "hello".to_owned(),
    };

    // Tag should return transformed variant name.
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"SUCCESS_RESPONSE"));
}

#[test]
fn test_adjacent_tag_with_variant_rename() {
    #[derive(JsonPointee)]
    #[pointer(tag = "kind", content = "data")]
    enum Message {
        #[pointer(rename = "success")]
        Success { text: String },
        #[allow(dead_code)]
        #[pointer(rename = "error")]
        Error { message: String },
    }

    let msg = Message::Success {
        text: "ok".to_owned(),
    };

    // Tag should return the explicit rename.
    let pointer = JsonPointer::parse("/kind").unwrap();
    let result = msg.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"success"));
}

#[test]
fn test_adjacent_tag_mixed_variants() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type", content = "value")]
    enum Mixed {
        Unit,
        Named { value: String },
        Tuple(i32, i32),
        Newtype(String),
    }

    // Unit variant.
    let unit = Mixed::Unit;
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = unit.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Unit"));
    // Content should error.
    let pointer = JsonPointer::parse("/value").unwrap();
    assert!(unit.resolve(pointer).is_err());

    // Named variant.
    let named = Mixed::Named {
        value: "test".to_owned(),
    };
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = named.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Named"));
    let pointer = JsonPointer::parse("/value/value").unwrap();
    let result = named.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"test".to_owned()));

    // Tuple variant.
    let tuple = Mixed::Tuple(1, 2);
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = tuple.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Tuple"));
    let pointer = JsonPointer::parse("/value/0").unwrap();
    let result = tuple.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<i32>(), Some(&1));

    // Newtype variant.
    let newtype = Mixed::Newtype("wrapped".to_owned());
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = newtype.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Newtype"));
    let pointer = JsonPointer::parse("/value").unwrap();
    let result = newtype.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<String>(), Some(&"wrapped".to_owned()));
}

#[test]
fn test_external_tag_wrong_variant_error() {
    #[derive(JsonPointee)]
    enum Response {
        Success {
            data: String,
        },
        #[allow(dead_code)]
        Error {
            code: i32,
        },
    }

    let response = Response::Success {
        data: "hello".to_owned(),
    };

    // Wrong variant name at top level should error.
    let pointer = JsonPointer::parse("/Error").unwrap();
    assert!(response.resolve(pointer).is_err());

    // Wrong field within correct variant should error.
    let pointer = JsonPointer::parse("/Success/code").unwrap();
    assert!(response.resolve(pointer).is_err());
}

#[test]
fn test_adjacent_tag_wrong_field_error() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type", content = "value")]
    enum Response {
        Success { data: String },
    }

    let response = Response::Success {
        data: "hello".to_owned(),
    };

    // Wrong top-level field should error.
    let pointer = JsonPointer::parse("/wrong").unwrap();
    assert!(response.resolve(pointer).is_err());

    // Wrong field within content should error.
    let pointer = JsonPointer::parse("/value/wrong").unwrap();
    assert!(response.resolve(pointer).is_err());
}

#[test]
fn test_external_tag_skipped_variant() {
    #[derive(JsonPointee)]
    enum Response {
        #[allow(dead_code)]
        Success { data: String },
        #[allow(dead_code)]
        #[pointer(skip)]
        Internal { debug: String },
    }

    let response = Response::Internal {
        debug: "secret".to_owned(),
    };

    // All access to skipped variant should error.
    let pointer = JsonPointer::parse("/Internal").unwrap();
    assert!(response.resolve(pointer).is_err());

    let pointer = JsonPointer::parse("/Internal/debug").unwrap();
    assert!(response.resolve(pointer).is_err());
}

#[test]
fn test_adjacent_tag_skipped_variant() {
    #[derive(JsonPointee)]
    #[pointer(tag = "type", content = "value")]
    enum Response {
        #[allow(dead_code)]
        Success { data: String },
        #[allow(dead_code)]
        #[pointer(skip)]
        Internal { debug: String },
    }

    let response = Response::Internal {
        debug: "secret".to_owned(),
    };

    // Tag field is accessible.
    let pointer = JsonPointer::parse("/type").unwrap();
    let result = response.resolve(pointer).unwrap();
    assert_eq!(result.downcast_ref::<&str>(), Some(&"Internal"));

    // Content field should error.
    let pointer = JsonPointer::parse("/value").unwrap();
    assert!(response.resolve(pointer).is_err());
}
