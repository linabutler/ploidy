//! Tests for [`Spec`].

use itertools::Itertools;

use crate::{
    arena::Arena,
    ir::{
        spec::Spec,
        types::{
            ParameterStyle, PrimitiveType, SpecInlineType, SpecOperation, SpecParameter,
            SpecParameterInfo, SpecRequest, SpecResponse, SpecType,
        },
    },
    parse::{Document, Method},
    tests::assert_matches,
};

// MARK: Basic operation extraction

#[test]
fn test_parses_single_operation_from_path() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            id: "listUsers",
            method: Method::Get,
            resource: None,
            ..
        }],
    );
}

#[test]
fn test_parses_multiple_operations_from_same_path() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              responses:
                '200':
                  description: Success
            post:
              operationId: createUser
              responses:
                '201':
                  description: Created
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [
            SpecOperation {
                method: Method::Get,
                ..
            },
            SpecOperation {
                method: Method::Post,
                ..
            },
        ],
    );
}

#[test]
fn test_parses_operations_from_multiple_paths() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              responses:
                '200':
                  description: Success
          /posts:
            get:
              operationId: listPosts
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [
            SpecOperation {
                id: "listUsers",
                ..
            },
            SpecOperation {
                id: "listPosts",
                ..
            },
        ],
    );
}

#[test]
fn test_parses_path_with_parameter_segments() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users/{id}:
            get:
              operationId: getUser
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [op @ SpecOperation { .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_eq!(op.path.len(), 2);
}

// MARK: Path parameters

#[test]
fn test_parses_path_parameter_string_type() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users/{id}:
            get:
              operationId: getUser
              parameters:
                - name: id
                  in: path
                  required: true
                  schema:
                    type: string
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [SpecParameter::Path(SpecParameterInfo {
            name: "id",
            required: true,
            ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::String)),
            ..
        })],
    );
}

#[test]
fn test_parses_path_parameter_integer_type() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users/{id}:
            get:
              operationId: getUser
              parameters:
                - name: id
                  in: path
                  required: true
                  schema:
                    type: integer
                    format: int64
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [SpecParameter::Path(SpecParameterInfo {
            name: "id",
            ty: SpecType::Inline(SpecInlineType::Primitive(_, PrimitiveType::I64)),
            ..
        })],
    );
}

#[test]
fn test_parses_multiple_path_parameters() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users/{userId}/posts/{postId}:
            get:
              operationId: getUserPost
              parameters:
                - name: userId
                  in: path
                  required: true
                  schema:
                    type: string
                - name: postId
                  in: path
                  required: true
                  schema:
                    type: string
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [
            SpecParameter::Path(SpecParameterInfo { name: "userId", .. }),
            SpecParameter::Path(SpecParameterInfo { name: "postId", .. }),
        ],
    );
}

// MARK: Query parameters

#[test]
fn test_parses_query_parameter_form_exploded() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: filter
                  in: query
                  required: false
                  schema:
                    type: string
                  style: form
                  explode: true
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [SpecParameter::Query(SpecParameterInfo {
            name: "filter",
            required: false,
            style: Some(ParameterStyle::Form { exploded: true }),
            ..
        })],
    );
}

#[test]
fn test_parses_query_parameter_form_unexploded() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: filter
                  in: query
                  required: false
                  schema:
                    type: string
                  style: form
                  explode: false
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [SpecParameter::Query(SpecParameterInfo {
            style: Some(ParameterStyle::Form { exploded: false }),
            ..
        })],
    );
}

#[test]
fn test_parses_query_parameter_pipe_delimited() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: tags
                  in: query
                  required: false
                  schema:
                    type: array
                    items:
                      type: string
                  style: pipeDelimited
                  explode: false
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [SpecParameter::Query(SpecParameterInfo {
            style: Some(ParameterStyle::PipeDelimited),
            ..
        })],
    );
}

#[test]
fn test_parses_query_parameter_space_delimited() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: tags
                  in: query
                  required: false
                  schema:
                    type: array
                    items:
                      type: string
                  style: spaceDelimited
                  explode: false
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [SpecParameter::Query(SpecParameterInfo {
            style: Some(ParameterStyle::SpaceDelimited),
            ..
        })],
    );
}

#[test]
fn test_parses_query_parameter_deep_object() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: filter
                  in: query
                  required: false
                  schema:
                    type: object
                    properties:
                      status:
                        type: string
                  style: deepObject
                  explode: true
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [SpecParameter::Query(SpecParameterInfo {
            style: Some(ParameterStyle::DeepObject),
            ..
        })],
    );
}

#[test]
fn test_parses_multiple_query_parameters() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: page
                  in: query
                  required: false
                  schema:
                    type: integer
                - name: limit
                  in: query
                  required: false
                  schema:
                    type: integer
                - name: status
                  in: query
                  required: false
                  schema:
                    type: string
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [
            SpecParameter::Query(SpecParameterInfo { name: "page", .. }),
            SpecParameter::Query(SpecParameterInfo { name: "limit", .. }),
            SpecParameter::Query(SpecParameterInfo { name: "status", .. }),
        ],
    );
}

#[test]
fn test_parses_query_parameter_with_description() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: page
                  in: query
                  required: false
                  description: The page number for pagination
                  schema:
                    type: integer
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [SpecParameter::Query(SpecParameterInfo {
            description: Some("The page number for pagination"),
            ..
        })],
    );
}

// MARK: Request bodies

#[test]
fn test_parses_request_body_json_reference() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            post:
              operationId: createUser
              requestBody:
                content:
                  application/json:
                    schema:
                      $ref: '#/components/schemas/User'
              responses:
                '201':
                  description: Created
        components:
          schemas:
            User:
              type: object
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            request: Some(SpecRequest::Json(_)),
            ..
        }],
        "expected JSON request",
    );
}

#[test]
fn test_parses_request_body_json_inline_schema() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            post:
              operationId: createUser
              requestBody:
                content:
                  application/json:
                    schema:
                      type: object
                      properties:
                        name:
                          type: string
              responses:
                '201':
                  description: Created
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            request: Some(SpecRequest::Json(_)),
            ..
        }],
        "expected JSON request",
    );
}

#[test]
fn test_parses_request_body_multipart() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /upload:
            post:
              operationId: uploadFile
              requestBody:
                content:
                  multipart/form-data:
                    schema:
                      type: object
                      properties:
                        file:
                          type: string
                          format: binary
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            request: Some(SpecRequest::Multipart),
            ..
        }],
        "expected multipart request",
    );
}

#[test]
fn test_parses_request_body_wildcard_content_type() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /data:
            post:
              operationId: processData
              requestBody:
                content:
                  '*/*':
                    schema:
                      type: object
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            request: Some(SpecRequest::Json(_)),
            ..
        }],
        "expected JSON request with wildcard content type",
    );
}

#[test]
fn test_operation_without_request_body() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(&*ir.operations, [SpecOperation { request: None, .. }]);
}

// MARK: Response parsing

#[test]
fn test_parses_response_json_reference() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              responses:
                '200':
                  description: Success
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/User'
        components:
          schemas:
            User:
              type: object
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            response: Some(SpecResponse::Json(_)),
            ..
        }],
        "expected JSON response",
    );
}

#[test]
fn test_parses_response_json_inline_schema() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              responses:
                '200':
                  description: Success
                  content:
                    application/json:
                      schema:
                        type: object
                        properties:
                          name:
                            type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            response: Some(SpecResponse::Json(_)),
            ..
        }],
        "expected JSON response",
    );
}

#[test]
fn test_prioritizes_2xx_status_over_default_response() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              responses:
                'default':
                  description: Error
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/Error'
                '200':
                  description: Success
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/User'
        components:
          schemas:
            User:
              type: object
              properties:
                name:
                  type: string
            Error:
              type: object
              properties:
                message:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [op @ SpecOperation { .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    let ty = match &op.response {
        Some(SpecResponse::Json(ty)) => ty,
        other => panic!("expected JSON response; got `{other:?}`"),
    };

    // The response should be from the 200 status, not the default.
    let component_ref = match ty {
        SpecType::Ref(component_ref) => component_ref,
        other => panic!("expected schema reference; got `{other:?}`"),
    };
    assert_eq!(component_ref.name(), "User");
}

#[test]
fn test_falls_back_to_default_response_when_no_2xx_status() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              responses:
                'default':
                  description: Default response
                  content:
                    application/json:
                      schema:
                        type: object
                        properties:
                          error:
                            type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            response: Some(_),
            ..
        }]
    );
}

#[test]
fn test_parses_response_with_wildcard_content_type() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /data:
            get:
              operationId: getData
              responses:
                '200':
                  description: Success
                  content:
                    '*/*':
                      schema:
                        type: object
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            response: Some(SpecResponse::Json(_)),
            ..
        }],
        "expected JSON response with wildcard content type",
    );
}

#[test]
fn test_selects_first_2xx_status_when_multiple_exist() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              responses:
                '200':
                  description: Success 200
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/UserList'
                '202':
                  description: Success 202
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/AcceptedResponse'
        components:
          schemas:
            UserList:
              type: object
              properties:
                users:
                  type: array
                  items:
                    type: string
            AcceptedResponse:
              type: object
              properties:
                accepted:
                  type: boolean
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [op @ SpecOperation { .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    let ty = match &op.response {
        Some(SpecResponse::Json(ty)) => ty,
        other => panic!("expected JSON response; got `{other:?}`"),
    };

    // The response should be from the first 2xx status (200), not 202.
    let component_ref = match ty {
        SpecType::Ref(component_ref) => component_ref,
        other => panic!("expected schema reference; got `{other:?}`"),
    };
    assert_eq!(component_ref.name(), "UserList");
}

#[test]
fn test_operation_without_response() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            delete:
              operationId: deleteUser
              responses: {}
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(&*ir.operations, [SpecOperation { response: None, .. }]);
}

// MARK: `x-resource-name` extension

#[test]
fn test_parses_custom_resource_name_from_extension() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              x-resource-name: user_management
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            resource: Some("user_management"),
            ..
        }],
    );
}

#[test]
fn test_different_operations_can_have_different_resources() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              x-resource-name: user_management
              responses:
                '200':
                  description: Success
          /posts:
            get:
              operationId: listPosts
              x-resource-name: content
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [
            SpecOperation {
                resource: Some("user_management"),
                ..
            },
            SpecOperation {
                resource: Some("content"),
                ..
            }
        ],
    );
}

// MARK: `x-resourceId` extension

#[test]
fn test_schema_stores_x_resource_id() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths: {}
        components:
          schemas:
            User:
              type: object
              x-resourceId: users
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let schema = spec.schemas.get("User").unwrap();

    assert_matches!(
        schema,
        SpecType::Schema(schema_ty) if schema_ty.resource() == Some("users"),
    );
}

#[test]
fn test_schema_without_x_resource_id_has_none() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths: {}
        components:
          schemas:
            User:
              type: object
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let spec = Spec::from_doc(&arena, &doc).unwrap();
    let schema = spec.schemas.get("User").unwrap();

    assert_matches!(
        schema,
        SpecType::Schema(schema_ty) if schema_ty.resource().is_none(),
    );
}

// MARK: Error cases

#[test]
fn test_operation_without_id_is_skipped() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              responses:
                '200':
                  description: Success
            post:
              responses:
                '201':
                  description: Created
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    // Only the GET operation with `operationId` should be present.
    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            id: "listUsers",
            ..
        }],
    );
}

// MARK: Schema extraction

#[test]
fn test_extracts_schemas_from_components() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths: {}
        components:
          schemas:
            User:
              type: object
              properties:
                name:
                  type: string
            Post:
              type: object
              properties:
                title:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let ids = ir.schemas.keys().copied().collect_vec();
    assert_matches!(&*ids, ["User", "Post"]);
}

#[test]
fn test_empty_spec() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths: {}
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_eq!(ir.schemas.len(), 0);
    assert_eq!(ir.operations.len(), 0);
}

// MARK: Combined scenarios

#[test]
fn test_operation_with_all_components() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users/{id}:
            put:
              operationId: updateUser
              x-resource-name: users
              description: Update an existing user
              parameters:
                - name: id
                  in: path
                  required: true
                  schema:
                    type: string
                - name: includeMetadata
                  in: query
                  required: false
                  schema:
                    type: boolean
              requestBody:
                content:
                  application/json:
                    schema:
                      $ref: '#/components/schemas/UpdateUserRequest'
              responses:
                '200':
                  description: User updated successfully
                  content:
                    application/json:
                      schema:
                        $ref: '#/components/schemas/User'
                'default':
                  description: Error
        components:
          schemas:
            User:
              type: object
            UpdateUserRequest:
              type: object
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            id: "updateUser",
            method: Method::Put,
            resource: Some("users"),
            description: Some("Update an existing user"),
            request: Some(_),
            response: Some(_),
            params,
            ..
        }] if params.len() == 2,
    );
    assert_eq!(ir.schemas.len(), 2);
}

#[test]
fn test_preserves_operation_descriptions() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              description: Retrieves a list of all users in the system
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    assert_matches!(
        &*ir.operations,
        [SpecOperation {
            description: Some("Retrieves a list of all users in the system"),
            ..
        }],
    );
}

#[test]
fn test_complex_spec_with_multiple_operations_and_resources() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Blog API
          version: 2.0
        paths:
          /users:
            get:
              operationId: listUsers
              x-resource-name: users
              parameters:
                - name: page
                  in: query
                  schema:
                    type: integer
              responses:
                '200':
                  description: Success
            post:
              operationId: createUser
              x-resource-name: users
              requestBody:
                content:
                  application/json:
                    schema:
                      $ref: '#/components/schemas/User'
              responses:
                '201':
                  description: Created
          /posts:
            get:
              operationId: listPosts
              x-resource-name: posts
              responses:
                '200':
                  description: Success
          /posts/{id}:
            get:
              operationId: getPost
              x-resource-name: posts
              parameters:
                - name: id
                  in: path
                  required: true
                  schema:
                    type: string
              responses:
                '200':
                  description: Success
        components:
          schemas:
            User:
              type: object
              properties:
                name:
                  type: string
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();
    assert_eq!(ir.schemas.len(), 1);
    assert_matches!(
        &*ir.operations,
        [
            SpecOperation {
                id: "listUsers",
                method: Method::Get,
                resource: Some("users"),
                ..
            },
            SpecOperation {
                id: "createUser",
                method: Method::Post,
                resource: Some("users"),
                ..
            },
            SpecOperation {
                id: "listPosts",
                method: Method::Get,
                resource: Some("posts"),
                ..
            },
            SpecOperation {
                id: "getPost",
                resource: Some("posts"),
                ..
            },
        ],
    );
}

// MARK: Parameter details

#[test]
fn test_query_parameter_default_style_is_form_exploded() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: filter
                  in: query
                  schema:
                    type: string
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(
        &**params,
        [SpecParameter::Query(SpecParameterInfo {
            // When `style` and `explode` are not specified,
            // the default should be an exploded form.
            style: Some(ParameterStyle::Form { exploded: true }),
            ..
        })]
    );
}

#[test]
fn test_mixed_path_and_query_parameters() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users/{id}/posts:
            get:
              operationId: getUserPosts
              parameters:
                - name: id
                  in: path
                  required: true
                  schema:
                    type: string
                - name: limit
                  in: query
                  required: false
                  schema:
                    type: integer
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    assert_matches!(&**params, [SpecParameter::Path(_), SpecParameter::Query(_)]);
}

#[test]
fn test_ignores_header_and_cookie_parameters() {
    let doc = Document::from_yaml(indoc::indoc! {"
        openapi: 3.0.0
        info:
          title: Test API
          version: 1.0
        paths:
          /users:
            get:
              operationId: listUsers
              parameters:
                - name: X-API-Key
                  in: header
                  required: true
                  schema:
                    type: string
                - name: sessionId
                  in: cookie
                  required: true
                  schema:
                    type: string
              responses:
                '200':
                  description: Success
    "})
    .unwrap();

    let arena = Arena::new();
    let ir = Spec::from_doc(&arena, &doc).unwrap();

    let [SpecOperation { params, .. }] = &*ir.operations else {
        panic!("expected single operation; got `{:?}`", ir.operations);
    };
    // Header and cookie parameters are ignored for now.
    assert_eq!(params.len(), 0);
}
