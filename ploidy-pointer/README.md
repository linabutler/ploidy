# ploidy-pointer

This crate provides a way to traverse strongly-typed data structures using JSON Pointers ([RFC 6901](https://datatracker.ietf.org/doc/html/rfc6901)). It's part of the [Ploidy](https://crates.io/crates/ploidy) OpenAPI code generator, but can be used standalone.

The cornerstone of **ploidy-pointer** is the `JsonPointee` trait, which can be implemented on types to make them traversable with JSON Pointers.

## Features

- Parse and validate JSON Pointer strings.
- Recursively resolve pointers against Rust data structures.
- Built-in implementations for primitive and collection types.
- Optional support for `serde_json`, `chrono`, `url`, and `indexmap`.
- Error handling with helpful suggestions for typos.
- Derive macro support via the `derive` feature (enabled by default).

### Cargo features

- `derive` (default): Enables the `#[derive(JsonPointee)]` macro.
- `serde_json`: Implements `JsonPointee` for `serde_json::Value`.
- `chrono`: Implements `JsonPointee` for `chrono::DateTime<Utc>`.
- `url`: Implements `JsonPointee` for `url::Url`.
- `indexmap`: Implements `JsonPointee` for `indexmap::IndexMap`.

## JSON Pointer Syntax

JSON Pointers are strings that identify a specific value within a JSON document:

* `` (empty string) - References the root value.
* `/foo` - References the `foo` field.
* `/foo/0` - References the first element of the `foo` array.
* `/foo/bar` - References the `bar` field of the `foo` object.

Two special characters are escaped:

- `~0` represents `~`, and...
- `~1` represents `/`.

## Usage

```rust
use ploidy_pointer::{JsonPointer, JsonPointee};
use std::collections::HashMap;

let mut data = HashMap::new();
data.insert("foo".to_string(), vec![1, 2, 3]);

// Parse a JSON Pointer
let pointer = JsonPointer::parse("/foo/1").unwrap();

// Resolve it against your data
let result = data.resolve(pointer).unwrap();

// Downcast to the expected type
assert_eq!(result.downcast_ref::<i32>(), Some(&2));
```

### With the derive macro

```rust
use ploidy_pointer::{JsonPointer, JsonPointee};

#[derive(JsonPointee)]
struct User {
    name: String,
    age: u32,
}

let user = User {
    name: "Alice".to_string(),
    age: 30,
};

let pointer = JsonPointer::parse("/name").unwrap();
let result = user.resolve(pointer).unwrap();
assert_eq!(result.downcast_ref::<String>(), Some(&"Alice".to_string()));
```

## Errors

The crate tries to provide helpful error messages, with suggestions for typos:

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
