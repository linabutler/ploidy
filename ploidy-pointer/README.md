# ploidy-pointer

This crate provides a way to traverse typed Rust data structures using JSON Pointers ([RFC 6901](https://datatracker.ietf.org/doc/html/rfc6901)). At its heart is the `JsonPointee` trait, which can be implemented on types to make them traversable.

**ploidy-pointer** is part of the [Ploidy](https://crates.io/crates/ploidy) OpenAPI code generator, but can be used standalone.

## Features

- Parse and resolve JSON Pointer strings.
- Built-in `JsonPointee` implementations for primitives, collections, and common external types.
- Derive `JsonPointee` implementations for your own types.

### Cargo features

- `derive` (_default_): Enables the `#[derive(JsonPointee)]` macro.
- `did-you-mean`: Adds suggestions for typos to error messages.
- `serde_json`: Implements `JsonPointee` for `serde_json::Value`.
- `chrono`: Implements `JsonPointee` for `chrono::DateTime<Utc>`.
- `url`: Implements `JsonPointee` for `url::Url`.
- `indexmap`: Implements `JsonPointee` for `indexmap::IndexMap`.
- `full`: Enables all features.

## JSON Pointer Syntax

JSON Pointers are strings that identify a specific value within a JSON structure:

- `""` (empty string) - References the root value.
- `"/foo"` - References the `foo` field.
- `"/foo/0"` - References the first element of the `foo` array.
- `"/foo/bar"` - References the `bar` field of the `foo` object.

Two special characters need to be escaped: `~` is written as `~0`, and `/` is written as `~1`.

Note that `"/"` (a single slash) does _not_ reference the root; it references a _field_ named `""` (the empty string). If you see an "unknown key" error for a field that you know exists, double-check that an extra slash hasn't snuck in to the pointer string.

## Usage

```rust
use ploidy_pointer::{JsonPointee, JsonPointer};
use std::collections::HashMap;

let mut data = HashMap::new();
data.insert("foo".to_owned(), vec![1, 2, 3]);

// Parse a JSON Pointer.
let pointer = JsonPointer::parse("/foo/1").unwrap();

// Resolve it against your data.
let result = data.resolve(pointer).unwrap();

// Downcast to the expected type.
assert_eq!(result.downcast_ref::<i32>(), Some(&2));
```

### Deriving `JsonPointee` for your own types

The `#[derive(JsonPointee)]` macro can generate implementations of `JsonPointer` for structs and enums, and supports [Serde](https://serde.rs)-like attributes for customizing the implementations. For more details, please see the [**ploidy-pointer-derive** docs](https://docs.rs/ploidy-pointer-derive).

```rust
use ploidy_pointer::{JsonPointee, JsonPointer};

#[derive(JsonPointee)]
struct User {
    name: String,
    age: u32,
}

let user = User {
    name: "Alice".to_owned(),
    age: 30,
};

let pointer = JsonPointer::parse("/name").unwrap();
let result = user.resolve(pointer).unwrap();
assert_eq!(result.downcast_ref::<String>(), Some(&"Alice".to_owned()));
```

### Errors

Type errors and missing key errors omit details by default, but you can enable the `did-you-mean` Cargo feature to add more context to error messages. Ploidy does this to provide more helpful errors when parsing OpenAPI documents:

```rust
let pointer = JsonPointer::parse("/naem").unwrap();
match user.resolve(pointer) {
    Ok(_) => unreachable!(),
    Err(err) => {
        // Error: unknown key "naem" for value of struct `User`;
        // did you mean "name"?
        println!("{}", err);
    }
}
```

## Similar crates

There are many great options for working with JSON Pointers in Rust: [**jsonptr**](https://crates.io/crates/jsonptr), [**json-pointer**](https://crates.io/crates/json-pointer) and its [forks](https://crates.io/crates/json-pointer-simd), and [`serde_json::Value::pointer`](https://docs.rs/serde_json/latest/serde_json/enum.Value.html#method.pointer).

For native Rust data structures, [**bevy_reflect**](https://crates.io/crates/bevy_reflect) and [**facet**](https://facet.rs) offer much more powerful runtime reflection capabilities.

**ploidy-pointer** fills a niche somewhere in between these two, providing JSON Pointers for native Rust data structures. This is especially useful for code generators like Ploidy, and strongly-typed API clients that want to navigate structured responses.

In short:

- If you're working with structured data, and want to add type-safe JSON Pointer traversal, **ploidy-pointer** could be a good fit.
- If you're working with dynamic JSON documents, and want to read and write values, consider **jsonptr** or **json-pointer**.
- If you're working with simpler JSON values, and don't need more advanced features, the `pointer()` method on `serde_json::Value` might be enough.
- If you'd like full runtime reflection for your structured data, give **bevy_reflect** or **facet** a try.
