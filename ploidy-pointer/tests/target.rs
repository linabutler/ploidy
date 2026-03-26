use ploidy_pointer::{JsonPointee, JsonPointeeExt, JsonPointerTarget};

#[test]
fn test_struct() {
    #[derive(JsonPointee, JsonPointerTarget)]
    struct MyStruct {
        name: String,
        count: i32,
    }

    let s = MyStruct {
        name: "hello".to_owned(),
        count: 42,
    };

    // `JsonPointerTarget` for `&MyStruct`.
    let result: &MyStruct = s.pointer("").unwrap();
    assert_eq!(result.count, 42);

    // `JsonPointerTarget` through fields.
    let name: &str = s.pointer("/name").unwrap();
    assert_eq!(name, "hello");

    let count: i32 = s.pointer("/count").unwrap();
    assert_eq!(count, 42);
}

#[test]
fn test_generic_struct() {
    #[derive(JsonPointee, JsonPointerTarget)]
    struct Wrapper<T: JsonPointee> {
        inner: T,
        label: String,
    }

    #[derive(Debug, Eq, JsonPointee, JsonPointerTarget, PartialEq)]
    struct Payload {
        value: i32,
    }

    let w = Wrapper {
        inner: Payload { value: 99 },
        label: "test".to_owned(),
    };

    // Extract the generic struct itself via `JsonPointerTarget`.
    let result: &Wrapper<Payload> = w.pointer("").unwrap();
    assert_eq!(result.label, "test");

    // Reach through the generic field into the payload.
    let val: i32 = w.pointer("/inner/value").unwrap();
    assert_eq!(val, 99);

    // Access a non-generic field.
    let label: &str = w.pointer("/label").unwrap();
    assert_eq!(label, "test");
}
