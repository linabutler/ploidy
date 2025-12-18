use std::str::FromStr;

use indexmap::IndexMap;
use itertools::Itertools;
use serde::{Deserialize, Deserializer};

use crate::error::SerdeError;

/// An OpenAPI document.
#[derive(Debug, Deserialize)]
pub struct Document {
    pub openapi: String,
    pub info: Info,
    #[serde(default)]
    pub paths: IndexMap<String, PathItem>,
    #[serde(default)]
    pub components: Option<Components>,
}

impl Document {
    /// Parse an OpenAPI document from a YAML or JSON string.
    pub fn from_yaml(yaml: &str) -> Result<Self, SerdeError> {
        let deserializer = serde_yaml::Deserializer::from_str(yaml);
        let result = serde_path_to_error::deserialize(deserializer)?;
        Ok(result)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Info {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub version: String,
}

/// Operation definitions for a single path.
#[derive(Debug, Deserialize)]
pub struct PathItem {
    #[serde(default)]
    pub get: Option<Operation>,
    #[serde(default)]
    pub post: Option<Operation>,
    #[serde(default)]
    pub put: Option<Operation>,
    #[serde(default)]
    pub delete: Option<Operation>,
    #[serde(default)]
    pub patch: Option<Operation>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl PathItem {
    /// Yields all operations and their HTTP methods.
    pub fn operations(&self) -> impl Iterator<Item = (Method, &Operation)> {
        [
            (Method::Get, self.get.as_ref()),
            (Method::Post, self.post.as_ref()),
            (Method::Put, self.put.as_ref()),
            (Method::Delete, self.delete.as_ref()),
            (Method::Patch, self.patch.as_ref()),
        ]
        .into_iter()
        .filter_map(|(method, op)| op.map(|o| (method, o)))
    }
}

/// An HTTP operation.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    #[serde(default)]
    pub description: Option<String>,
    pub operation_id: Option<String>,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default)]
    pub request_body: Option<RequestBody>,
    #[serde(default)]
    pub responses: IndexMap<String, Response>,
    #[serde(flatten)]
    pub extensions: IndexMap<String, serde_json::Value>,
}

impl Operation {
    pub fn extension(&self, name: &str) -> Option<&str> {
        self.extensions.get(name)?.as_str()
    }
}

/// A path, query, header, or cookie parameter.
#[derive(Debug, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "in")]
    pub location: ParameterLocation,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub schema: Option<RefOrSchema>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterLocation {
    Path,
    Query,
    Header,
    Cookie,
}

/// Request body definition.
#[derive(Debug, Deserialize)]
pub struct RequestBody {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub content: IndexMap<String, MediaType>,
}

/// Response definition.
#[derive(Debug, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub content: Option<IndexMap<String, MediaType>>,
}

/// Media type content.
#[derive(Debug, Deserialize)]
pub struct MediaType {
    #[serde(default)]
    pub schema: Option<RefOrSchema>,
}

/// Components section containing reusable schemas.
#[derive(Debug, Deserialize)]
pub struct Components {
    #[serde(default)]
    pub schemas: IndexMap<String, Schema>,
}

/// Either a reference or a schema definition.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum RefOrSchema {
    Ref(Ref),
    Schema(Box<Schema>),
}

/// A reference to another schema.
#[derive(Debug, Clone, Deserialize)]
pub struct Ref {
    #[serde(rename = "$ref")]
    pub path: SchemaRefPath,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Ty {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
    Null,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[serde(rename = "date-time")]
    DateTime,
    Date,
    Uri,
    Uuid,
    Byte,
    Binary,
    Int32,
    Int64,
    Float,
    Double,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum AdditionalProperties {
    Bool(bool),
    RefOrSchema(RefOrSchema),
}

/// An OpenAPI schema definition.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema {
    #[serde(rename = "type", default, deserialize_with = "deserialize_type")]
    pub ty: Vec<Ty>,
    #[serde(default)]
    pub format: Option<Format>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub nullable: bool,

    // Object properties.
    #[serde(default)]
    pub properties: Option<IndexMap<String, RefOrSchema>>,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub additional_properties: Option<AdditionalProperties>,

    // Array items.
    #[serde(default)]
    pub items: Option<RefOrSchema>,

    // Enum variants.
    #[serde(rename = "enum", default)]
    pub variants: Option<Vec<serde_json::Value>>,

    // Composition.
    #[serde(default)]
    pub all_of: Option<Vec<RefOrSchema>>,
    #[serde(default)]
    pub one_of: Option<Vec<RefOrSchema>>,
    #[serde(default)]
    pub any_of: Option<Vec<RefOrSchema>>,
    #[serde(default)]
    pub discriminator: Option<Discriminator>,
}

/// A discriminator for a polymorphic type.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discriminator {
    pub property_name: String,
    #[serde(default)]
    pub mapping: IndexMap<String, SchemaRefPath>,
}

/// The path of a schema reference.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SchemaRefPath(String);

impl SchemaRefPath {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl FromStr for SchemaRefPath {
    type Err = BadSchemaRefPath;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // A makeshift JSON Schema reference parser (<URI> # <JSON-Pointer>)
        // that only understands references to keys under `/components/schemas`
        // in the current document.
        let Some(pointer) = s
            .trim_matches(|c| c <= ' ')
            .strip_prefix('#')
            .map(|rest| &rest[..rest.find(['\t', '\n', '\r']).unwrap_or(rest.len())])
        else {
            return Err(BadSchemaRefPath);
        };
        if !pointer.starts_with('/') {
            return Err(BadSchemaRefPath);
        }
        let mut parts = pointer.split('/').skip(1);
        let Some(["components", "schemas", name]) = parts.next_array() else {
            return Err(BadSchemaRefPath);
        };
        if parts.next().is_some() {
            return Err(BadSchemaRefPath);
        }
        Ok(Self(name.replace("~1", "/").replace("~0", "~")))
    }
}

impl<'de> Deserialize<'de> for SchemaRefPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = SchemaRefPath;

            fn expecting(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str("a schema reference")
            }

            fn visit_str<E: ::serde::de::Error>(self, s: &str) -> Result<Self::Value, E> {
                s.parse().map_err(E::custom)
            }
        }
        deserializer.deserialize_str(Visitor)
    }
}

fn deserialize_type<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Ty>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum TypesOr {
        /// An OpenAPI 3.1-style `type` array.
        Types(Vec<Ty>),
        /// A single `type`.
        Type(Ty),
    }
    Ok(match TypesOr::deserialize(deserializer)? {
        TypesOr::Types(types) => types,
        TypesOr::Type(ty) => vec![ty],
    })
}

#[derive(Debug, thiserror::Error)]
#[error("only `#/components/schemas/{{name}}` references are supported")]
pub struct BadSchemaRefPath;
