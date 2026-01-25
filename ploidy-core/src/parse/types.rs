use std::str::FromStr;

use indexmap::IndexMap;
use ploidy_pointer::{JsonPointee, JsonPointer};
use serde::{Deserialize, Deserializer};

use crate::error::SerdeError;

/// An OpenAPI document.
#[derive(Debug, Deserialize, JsonPointee)]
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

#[derive(Clone, Debug, Deserialize, JsonPointee)]
pub struct Info {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub version: String,
}

/// Operation definitions for a single path.
#[derive(Debug, Deserialize, JsonPointee)]
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
    /// Returns an iterator over the operations for each HTTP method.
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
#[derive(Debug, Deserialize, JsonPointee)]
#[serde(rename_all = "camelCase")]
#[ploidy(rename_all = "camelCase")]
pub struct Operation {
    #[serde(default)]
    pub description: Option<String>,
    pub operation_id: Option<String>,
    #[serde(default)]
    pub parameters: Vec<RefOrParameter>,
    #[serde(default)]
    pub request_body: Option<RefOrRequestBody>,
    #[serde(default)]
    pub responses: IndexMap<String, RefOrResponse>,
    #[serde(flatten)]
    pub extensions: IndexMap<String, serde_json::Value>,
}

impl Operation {
    pub fn extension<'a, X: FromExtension<'a>>(&'a self, name: &str) -> Option<X> {
        X::from_extension(self.extensions.get(name)?)
    }
}

/// A path, query, header, or cookie parameter.
#[derive(Clone, Debug, Deserialize, JsonPointee)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "in")]
    #[ploidy(rename = "in")]
    pub location: ParameterLocation,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub schema: Option<RefOrSchema>,
    #[serde(default)]
    pub style: Option<ParameterStyle>,
    #[serde(default)]
    pub explode: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonPointee)]
#[serde(rename_all = "lowercase")]
#[ploidy(untagged, rename_all = "lowercase")]
pub enum ParameterLocation {
    Path,
    Query,
    Header,
    Cookie,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonPointee)]
#[serde(rename_all = "camelCase")]
#[ploidy(untagged, rename_all = "camelCase")]
pub enum ParameterStyle {
    Matrix,
    Label,
    Form,
    Simple,
    SpaceDelimited,
    PipeDelimited,
    DeepObject,
}

/// Request body definition.
#[derive(Clone, Debug, Deserialize, JsonPointee)]
pub struct RequestBody {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub content: IndexMap<String, MediaType>,
}

/// Response definition.
#[derive(Clone, Debug, Deserialize, JsonPointee)]
pub struct Response {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub content: Option<IndexMap<String, MediaType>>,
}

/// Example definition (placeholder).
#[derive(Debug, Deserialize, JsonPointee)]
pub struct Example {
    #[serde(flatten)]
    pub extensions: IndexMap<String, serde_json::Value>,
}

/// Header definition (placeholder).
#[derive(Debug, Deserialize, JsonPointee)]
pub struct Header {
    #[serde(flatten)]
    pub extensions: IndexMap<String, serde_json::Value>,
}

/// Security scheme definition (placeholder).
#[derive(Debug, Deserialize, JsonPointee)]
pub struct SecurityScheme {
    #[serde(flatten)]
    pub extensions: IndexMap<String, serde_json::Value>,
}

/// Link definition (placeholder).
#[derive(Debug, Deserialize, JsonPointee)]
pub struct Link {
    #[serde(flatten)]
    pub extensions: IndexMap<String, serde_json::Value>,
}

/// Callback definition (placeholder).
#[derive(Debug, Deserialize, JsonPointee)]
pub struct Callback {
    #[serde(flatten)]
    pub extensions: IndexMap<String, serde_json::Value>,
}

/// Media type content.
#[derive(Clone, Debug, Deserialize, JsonPointee)]
pub struct MediaType {
    #[serde(default)]
    pub schema: Option<RefOrSchema>,
}

/// Components section containing reusable schemas.
#[derive(Debug, Default, Deserialize, JsonPointee)]
#[serde(rename_all = "camelCase")]
#[ploidy(rename_all = "camelCase")]
pub struct Components {
    #[serde(default)]
    pub schemas: IndexMap<String, Schema>,
    #[serde(default)]
    pub responses: IndexMap<String, Response>,
    #[serde(default)]
    pub parameters: IndexMap<String, Parameter>,
    #[serde(default)]
    pub examples: IndexMap<String, Example>,
    #[serde(default)]
    pub request_bodies: IndexMap<String, RequestBody>,
    #[serde(default)]
    pub headers: IndexMap<String, Header>,
    #[serde(default)]
    pub security_schemes: IndexMap<String, SecurityScheme>,
    #[serde(default)]
    pub links: IndexMap<String, Link>,
    #[serde(default)]
    pub callbacks: IndexMap<String, Callback>,
}

/// Either a reference to a component or an inline component definition.
///
/// This generic type is used throughout the OpenAPI parse tree to represent
/// locations where a component can either be defined inline, or referenced via
/// `$ref`. The [`RefOr::Ref`] variant holds a JSON Pointer to a component definition
/// in the `#/components/*` section; the [`RefOr::Other`] variant holds an
/// inline definition.
///
/// The type uses `#[serde(untagged)]` to match the OpenAPI specification's
/// untagged union semantics, and `#[ploidy(untagged)]` for JSON Pointer traversal.
#[derive(Clone, Debug, Deserialize, JsonPointee)]
#[serde(untagged)]
#[ploidy(untagged)]
pub enum RefOr<T> {
    /// A reference to a component definition via `$ref`.
    #[ploidy(skip)]
    Ref(Ref),
    /// An inline component definition.
    Other(T),
}

/// Either a reference or a schema definition.
pub type RefOrSchema = RefOr<Box<Schema>>;

/// Either a reference or a parameter definition.
pub type RefOrParameter = RefOr<Parameter>;

/// Either a reference or a request body definition.
pub type RefOrRequestBody = RefOr<RequestBody>;

/// Either a reference or a response definition.
pub type RefOrResponse = RefOr<Response>;

/// A reference to another schema.
#[derive(Debug, Clone, Deserialize)]
pub struct Ref {
    #[serde(rename = "$ref")]
    pub path: ComponentRef,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonPointee)]
#[serde(rename_all = "lowercase")]
#[ploidy(untagged, rename_all = "lowercase")]
pub enum Ty {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
    Null,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, JsonPointee)]
#[serde(rename_all = "lowercase")]
#[ploidy(untagged, rename_all = "lowercase")]
pub enum Format {
    #[serde(rename = "date-time")]
    DateTime,
    #[serde(rename = "unixtime", alias = "unix-time")]
    UnixTime,
    Date,
    Uri,
    Uuid,
    Byte,
    Binary,
    Int8,
    UInt8,
    Int16,
    UInt16,
    Int32,
    UInt32,
    Int64,
    UInt64,
    Float,
    Double,
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Deserialize, JsonPointee)]
#[serde(untagged)]
#[ploidy(untagged)]
pub enum AdditionalProperties {
    Bool(bool),
    RefOrSchema(RefOrSchema),
}

/// An OpenAPI schema definition.
#[derive(Debug, Clone, Default, Deserialize, JsonPointee)]
#[serde(rename_all = "camelCase")]
#[ploidy(rename_all = "camelCase")]
pub struct Schema {
    #[serde(rename = "type", default, deserialize_with = "deserialize_type")]
    #[ploidy(rename = "type")]
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

    // Extensions.
    #[serde(flatten)]
    pub extensions: IndexMap<String, serde_json::Value>,
}

impl Schema {
    /// Returns the value of an extension field as a string.
    pub fn extension<'a, X: FromExtension<'a>>(&'a self, name: &str) -> Option<X> {
        X::from_extension(self.extensions.get(name)?)
    }
}

/// A discriminator for a polymorphic type.
#[derive(Debug, Clone, Deserialize, JsonPointee)]
#[serde(rename_all = "camelCase")]
#[ploidy(rename_all = "camelCase")]
pub struct Discriminator {
    pub property_name: String,
    #[serde(default)]
    pub mapping: IndexMap<String, ComponentRef>,
}

/// A JSON Pointer reference to a component in the current document.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, JsonPointee)]
pub struct ComponentRef {
    /// The parsed JSON pointer (without the '#' prefix)
    #[ploidy(skip)]
    pointer: JsonPointer<'static>,
}

impl ComponentRef {
    /// Returns a reference to the pointer
    pub fn pointer(&self) -> &JsonPointer<'static> {
        &self.pointer
    }

    /// Extracts the component name (final segment, unescaped)
    pub fn name(&self) -> &str {
        self.pointer
            .segments()
            .next_back()
            .map(|s| s.as_str())
            .unwrap_or("")
    }
}

impl FromStr for ComponentRef {
    type Err = BadComponentRef;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some(s) = s
            .trim_matches(|c| c <= ' ')
            .strip_prefix('#')
            .map(|rest| &rest[..rest.find(['\t', '\n', '\r']).unwrap_or(rest.len())])
        else {
            return Err(BadComponentRef::NotSameDocument);
        };
        let pointer = JsonPointer::parse_owned(s).map_err(BadComponentRef::Syntax)?;
        Ok(Self { pointer })
    }
}

impl<'de> Deserialize<'de> for ComponentRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ComponentRef;
            fn expecting(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str("a component reference")
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
pub enum BadComponentRef {
    #[error("references must start with `#`; external references aren't supported")]
    NotSameDocument,
    #[error("invalid JSON Pointer syntax: {0}")]
    Syntax(#[from] ploidy_pointer::BadJsonPointerSyntax),
}

pub trait FromExtension<'a>: Sized {
    fn from_extension(value: &'a serde_json::Value) -> Option<Self>;
}

impl<'a> FromExtension<'a> for &'a str {
    fn from_extension(value: &'a serde_json::Value) -> Option<&'a str> {
        value.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_schema_ref() {
        let r: ComponentRef = "#/components/schemas/Pet".parse().unwrap();
        assert_eq!(r.name(), "Pet");
    }

    #[test]
    fn parse_all_component_types() {
        for type_name in [
            "schemas",
            "responses",
            "parameters",
            "examples",
            "requestBodies",
            "headers",
            "securitySchemes",
            "links",
            "callbacks",
        ] {
            let ref_str = format!("#/components/{}/Test", type_name);
            let r: ComponentRef = ref_str.parse().unwrap();
            assert_eq!(r.name(), "Test");
        }
    }

    #[test]
    fn reject_external_ref() {
        let err = "other.yaml#/components/schemas/Pet".parse::<ComponentRef>();
        assert!(matches!(err, Err(BadComponentRef::NotSameDocument)));
    }

    #[test]
    fn handle_escaping() {
        let r: ComponentRef = "#/components/schemas/Foo~1Bar".parse().unwrap();
        assert_eq!(r.name(), "Foo/Bar");
    }
}
