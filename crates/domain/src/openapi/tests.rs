use serde_json::json;

use super::*;
use crate::{
    collection::{Folder, Item, SavedRequest, SourceKind, auth_hash, value_hash},
    request::{ApiKeyLocation, Auth, Body, HttpMethod, KeyValue, RawKind},
};

fn file() -> SourceLocation {
    SourceLocation::File("/specs/api.json".into())
}

fn convert(spec: serde_json::Value) -> ImportResult {
    import(&spec.to_string(), file()).unwrap()
}

fn v3(paths: serde_json::Value) -> serde_json::Value {
    json!({
        "openapi": "3.0.3",
        "info": {"title": "API", "version": "1"},
        "servers": [{"url": "https://api.example.com/v1"}],
        "paths": paths,
    })
}

fn requests(items: &[Item]) -> Vec<&SavedRequest> {
    items
        .iter()
        .flat_map(|item| match item {
            Item::Request(saved) => vec![saved],
            Item::Folder(folder) => requests(&folder.items),
        })
        .collect()
}

fn only_request(result: &ImportResult) -> &SavedRequest {
    let all = requests(&result.collection.items);
    assert_eq!(all.len(), 1, "expected one request");
    all[0]
}

fn folders(items: &[Item]) -> Vec<&Folder> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Folder(folder) => Some(folder),
            Item::Request(_) => None,
        })
        .collect()
}

fn row(enabled: bool, key: &str, value: &str) -> KeyValue {
    KeyValue {
        enabled,
        key: key.into(),
        value: value.into(),
    }
}

fn variable(result: &ImportResult, key: &str) -> Option<String> {
    let variables = &result.collection.variables;
    variables
        .iter()
        .find(|row| row.key == key)
        .map(|row| row.value.clone())
}

fn raw_text(saved: &SavedRequest) -> &str {
    match &saved.request.body {
        Body::Raw { text, .. } => text,
        other => panic!("expected a raw body, got {other:?}"),
    }
}

const PETSTORE_3: &str = r##"{
  "openapi": "3.0.0",
  "info": {"title": "Swagger Petstore", "version": "1.0.0"},
  "servers": [{"url": "http://petstore.swagger.io/v1"}],
  "tags": [{"name": "pets"}],
  "paths": {
    "/pets": {
      "get": {
        "summary": "List all pets",
        "operationId": "listPets",
        "tags": ["pets"],
        "parameters": [{"name": "limit", "in": "query", "required": false,
                        "schema": {"type": "integer", "format": "int32", "default": 20}}]
      },
      "post": {
        "summary": "Create a pet",
        "operationId": "createPets",
        "tags": ["pets"],
        "requestBody": {"required": true,
          "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Pet"}}}}
      }
    },
    "/pets/{petId}": {
      "get": {
        "summary": "Info for a specific pet",
        "operationId": "showPetById",
        "tags": ["pets"],
        "parameters": [{"name": "petId", "in": "path", "required": true,
                        "schema": {"type": "string"}, "example": "42"}]
      }
    }
  },
  "components": {
    "schemas": {
      "Pet": {
        "type": "object",
        "required": ["id", "name"],
        "properties": {
          "id": {"type": "integer", "format": "int64"},
          "name": {"type": "string"},
          "tag": {"type": "string", "enum": ["dog", "cat"]}
        }
      }
    }
  }
}"##;

#[test]
fn petstore_3_converts() {
    let location = SourceLocation::Url("https://example.com/petstore.json".into());
    let result = import(PETSTORE_3, location.clone()).unwrap();
    let collection = &result.collection;
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(collection.name, "Swagger Petstore");
    assert_eq!(collection.auth, Auth::None);
    assert_eq!(
        collection.variables,
        [
            row(true, "baseUrl", "http://petstore.swagger.io/v1"),
            row(true, "petId", "42"),
        ]
    );

    let folders = folders(&collection.items);
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].name, "pets");
    let names: Vec<_> = requests(&collection.items)
        .iter()
        .map(|saved| saved.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["List all pets", "Create a pet", "Info for a specific pet"]
    );

    let list = requests(&collection.items)[0];
    assert_eq!(list.request.method, HttpMethod::Get);
    assert_eq!(list.request.url, "{{baseUrl}}/pets");
    assert_eq!(list.request.params, [row(false, "limit", "20")]);
    assert_eq!(list.request.auth, Auth::Inherit);

    let create = requests(&collection.items)[1];
    assert_eq!(create.request.method, HttpMethod::Post);
    assert_eq!(
        create.request.body,
        Body::Raw {
            kind: RawKind::Json,
            text: "{\n  \"id\": 0,\n  \"name\": \"string\",\n  \"tag\": \"dog\"\n}".into(),
        }
    );
    assert!(create.request.headers.is_empty());

    let show = requests(&collection.items)[2];
    assert_eq!(show.request.url, "{{baseUrl}}/pets/{{petId}}");

    let source = collection.source.as_ref().unwrap();
    assert_eq!(source.kind, SourceKind::OpenApi);
    assert_eq!(source.location, location);
    assert_eq!(source.title, "Swagger Petstore");
    assert_eq!(source.version, "1.0.0");
    assert_eq!(source.auth_hash, auth_hash(&Auth::None));
    let owned: Vec<_> = source.variables.iter().map(|v| v.key.as_str()).collect();
    assert_eq!(owned, ["baseUrl", "petId"]);
    assert_eq!(source.variables[1].hash, value_hash("42"));
}

#[test]
fn origins_record_keys_and_hashes() {
    let result = import(PETSTORE_3, file()).unwrap();
    let collection = &result.collection;
    let keys: Vec<_> = requests(&collection.items)
        .iter()
        .map(|saved| saved.origin.as_ref().unwrap().key.as_str())
        .collect();
    assert_eq!(keys, ["listPets", "createPets", "showPetById"]);
    for saved in requests(&collection.items) {
        assert_eq!(saved.origin.as_ref().unwrap().hash, saved.content_hash());
    }
    let folder = folders(&collection.items)[0];
    let origin = folder.origin.as_ref().unwrap();
    assert_eq!(origin.key, "pets");
    assert_eq!(origin.hash, folder.content_hash());

    let again = import(PETSTORE_3, file()).unwrap();
    let hashes = |result: &ImportResult| -> Vec<String> {
        requests(&result.collection.items)
            .iter()
            .map(|saved| saved.origin.clone().unwrap().hash)
            .collect()
    };
    assert_eq!(hashes(&result), hashes(&again));
}

#[test]
fn duplicate_or_missing_operation_ids_key_by_method_and_path() {
    let result = convert(v3(json!({
        "/a": {"get": {"operationId": "same"}, "post": {"operationId": "same"}},
        "/b/{id}": {"delete": {}, "put": {"operationId": "putB"}}
    })));
    let keys: Vec<_> = requests(&result.collection.items)
        .iter()
        .map(|saved| saved.origin.clone().unwrap().key)
        .collect();
    assert_eq!(keys, ["GET /a", "POST /a", "DELETE /b/{id}", "putB"]);
    let names: Vec<_> = requests(&result.collection.items)
        .iter()
        .map(|saved| saved.name.clone())
        .collect();
    assert_eq!(names, ["same", "same", "DELETE /b/{id}", "putB"]);
}

const PETSTORE_2: &str = r##"{
  "swagger": "2.0",
  "info": {"title": "Petstore 2", "version": "1.0.5"},
  "host": "petstore.swagger.io",
  "basePath": "/v2",
  "schemes": ["https", "http"],
  "tags": [{"name": "pet"}, {"name": "store"}],
  "securityDefinitions": {
    "api_key": {"type": "apiKey", "name": "api_key", "in": "header"}
  },
  "paths": {
    "/pet": {
      "post": {
        "tags": ["pet"],
        "summary": "Add a new pet",
        "operationId": "addPet",
        "consumes": ["application/json", "application/xml"],
        "parameters": [{"in": "body", "name": "body", "required": true,
                        "schema": {"$ref": "#/definitions/Pet"}}]
      }
    },
    "/pet/{petId}": {
      "parameters": [{"name": "petId", "in": "path", "required": true, "type": "integer"}],
      "get": {
        "tags": ["pet"],
        "summary": "Find pet by ID",
        "parameters": [{"name": "petId", "in": "path", "required": true, "type": "integer",
                        "x-example": 7}],
        "security": [{"api_key": []}]
      },
      "post": {
        "tags": ["pet"],
        "summary": "Updates a pet with form data",
        "consumes": ["application/x-www-form-urlencoded"],
        "parameters": [
          {"name": "name", "in": "formData", "required": true, "type": "string"},
          {"name": "status", "in": "formData", "type": "string", "enum": ["available", "sold"]}
        ]
      }
    },
    "/pet/{petId}/uploadImage": {
      "post": {
        "tags": ["pet"],
        "summary": "uploads an image",
        "consumes": ["multipart/form-data"],
        "parameters": [
          {"name": "petId", "in": "path", "required": true, "type": "integer"},
          {"name": "additionalMetadata", "in": "formData", "type": "string"},
          {"name": "file", "in": "formData", "type": "file"}
        ]
      }
    },
    "/store/inventory": {
      "get": {"tags": ["store"], "summary": "Returns pet inventories",
              "parameters": [{"name": "X-Trace", "in": "header", "type": "string", "default": "on"}]}
    }
  },
  "definitions": {
    "Pet": {
      "type": "object",
      "properties": {
        "id": {"type": "integer"},
        "category": {"$ref": "#/definitions/Category"},
        "photoUrls": {"type": "array", "items": {"type": "string"}},
        "status": {"type": "string", "enum": ["available", "pending"]}
      }
    },
    "Category": {"type": "object", "properties": {"name": {"type": "string", "example": "Dogs"}}}
  }
}"##;

#[test]
fn petstore_2_converts() {
    let result = import(PETSTORE_2, file()).unwrap();
    let collection = &result.collection;
    assert_eq!(collection.name, "Petstore 2");
    assert_eq!(
        variable(&result, "baseUrl").unwrap(),
        "https://petstore.swagger.io/v2"
    );
    assert_eq!(variable(&result, "petId").unwrap(), "7");
    assert_eq!(variable(&result, "apiKey").unwrap(), "");
    assert_eq!(
        result.warnings,
        ["1 file field became empty text field."],
        "{:?}",
        result.warnings
    );

    let folders = folders(&collection.items);
    let names: Vec<_> = folders.iter().map(|folder| folder.name.as_str()).collect();
    assert_eq!(names, ["pet", "store"]);

    let all = requests(&collection.items);
    let add = all[0];
    assert_eq!(
        raw_text(add),
        "{\n  \"id\": 0,\n  \"category\": {\n    \"name\": \"Dogs\"\n  },\n  \"photoUrls\": [\n    \"string\"\n  ],\n  \"status\": \"available\"\n}"
    );

    let find = all[1];
    assert_eq!(find.request.url, "{{baseUrl}}/pet/{{petId}}");
    assert_eq!(
        find.request.auth,
        Auth::ApiKey {
            key: "api_key".into(),
            value: "{{apiKey}}".into(),
            location: ApiKeyLocation::Header,
        }
    );

    let form = all[2];
    assert_eq!(
        form.request.body,
        Body::UrlEncoded(vec![
            row(true, "name", ""),
            row(false, "status", "available")
        ])
    );

    let upload = all[3];
    assert_eq!(
        upload.request.body,
        Body::FormData(vec![
            row(false, "additionalMetadata", ""),
            row(false, "file", "")
        ])
    );

    let inventory = all[4];
    assert_eq!(inventory.request.headers, [row(false, "X-Trace", "on")]);
}

#[test]
fn swagger_2_without_host_uses_the_source_url() {
    let spec = r#"{"swagger": "2.0", "info": {"title": "T", "version": "1"}, "basePath": "/api",
                   "paths": {}}"#;
    let location = SourceLocation::Url("http://localhost:8080/swagger.json".into());
    let result = import(spec, location).unwrap();
    assert_eq!(
        variable(&result, "baseUrl").unwrap(),
        "http://localhost:8080/api"
    );

    let result = import(spec, file()).unwrap();
    assert_eq!(variable(&result, "baseUrl").unwrap(), "/api");
    assert_eq!(
        result.warnings,
        ["The server URL \"/api\" is relative; set baseUrl to the full URL."]
    );
}

#[test]
fn tags_become_folders_in_declared_order_and_untagged_stay_at_the_root() {
    let mut spec = v3(json!({
        "/a": {"get": {"summary": "A", "tags": ["late", "other"]}},
        "/b": {"get": {"summary": "B"}},
        "/c": {"get": {"summary": "C", "tags": ["first"]}},
        "/d": {"get": {"summary": "D", "tags": ["late"]}},
        "/e": {"get": {"summary": "E", "tags": ["undeclared"]}}
    }));
    spec["tags"] = json!([{"name": "first"}, {"name": "unused"}, {"name": "late"}]);
    let result = convert(spec);
    let items = &result.collection.items;
    let summary: Vec<String> = items
        .iter()
        .map(|item| match item {
            Item::Folder(folder) => {
                let inner: Vec<_> = requests(&folder.items)
                    .iter()
                    .map(|saved| saved.name.clone())
                    .collect();
                format!("{}[{}]", folder.name, inner.join(","))
            }
            Item::Request(saved) => saved.name.clone(),
        })
        .collect();
    assert_eq!(summary, ["first[C]", "late[A,D]", "undeclared[E]", "B"]);
}

#[test]
fn params_merge_path_and_operation_levels() {
    let result = convert(v3(json!({
        "/users/{userId}/posts/{postId}": {
            "parameters": [
                {"name": "userId", "in": "path", "required": true, "schema": {"type": "string", "default": "u1"}},
                {"name": "page", "in": "query", "schema": {"type": "integer", "default": 1}},
                {"name": "X-Shared", "in": "header", "required": true, "schema": {"type": "string"}}
            ],
            "get": {
                "parameters": [
                    {"name": "page", "in": "query", "required": true, "example": 3},
                    {"name": "sort", "in": "query", "schema": {"type": "string", "enum": ["asc", "desc"]}},
                    {"name": "ids", "in": "query", "example": [1, 2]},
                    {"name": "postId", "in": "path", "required": true,
                     "examples": {"one": {"value": "p9"}}},
                    {"name": "Accept", "in": "header", "schema": {"type": "string"}},
                    {"name": "X-Request-Id", "in": "header", "schema": {"type": "string", "example": "abc"}},
                    {"name": "session", "in": "cookie", "schema": {"type": "string"}}
                ]
            }
        }
    })));
    let saved = only_request(&result);
    assert_eq!(
        saved.request.url,
        "{{baseUrl}}/users/{{userId}}/posts/{{postId}}"
    );
    assert_eq!(
        saved.request.params,
        [
            row(true, "page", "3"),
            row(false, "sort", "asc"),
            row(false, "ids", "1,2")
        ]
    );
    assert_eq!(
        saved.request.headers,
        [row(true, "X-Shared", ""), row(false, "X-Request-Id", "abc")]
    );
    assert_eq!(variable(&result, "userId").unwrap(), "u1");
    assert_eq!(variable(&result, "postId").unwrap(), "p9");
    assert_eq!(result.warnings, ["1 cookie parameter was skipped."]);
}

#[test]
fn path_variables_are_added_once_and_filled_by_a_later_example() {
    let result = convert(v3(json!({
        "/a/{id}": {"get": {}},
        "/b/{id}/{id2}": {"get": {"parameters": [
            {"name": "id", "in": "path", "required": true, "example": 5}
        ]}},
        "/c/{id}": {"get": {"parameters": [
            {"name": "id", "in": "path", "required": true, "example": 6}
        ]}}
    })));
    let keys: Vec<_> = result
        .collection
        .variables
        .iter()
        .map(|row| (row.key.as_str(), row.value.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            ("baseUrl", "https://api.example.com/v1"),
            ("id", "5"),
            ("id2", "")
        ]
    );
}

#[test]
fn refs_resolve_for_parameters_bodies_and_path_items() {
    let mut spec = v3(json!({
        "/items": {"$ref": "#/components/pathItems/Items"},
        "/other": {"post": {
            "parameters": [{"$ref": "#/components/parameters/Limit"}],
            "requestBody": {"$ref": "#/components/requestBodies/Thing"}
        }}
    }));
    spec["components"] = json!({
        "pathItems": {"Items": {"get": {"summary": "From a ref"}}},
        "parameters": {"Limit": {"$ref": "#/components/parameters/RealLimit"},
                       "RealLimit": {"name": "limit", "in": "query", "example": 10}},
        "requestBodies": {"Thing": {"content": {"application/json": {
            "schema": {"$ref": "#/components/schemas/Thing"}}}}},
        "schemas": {"Thing": {"type": "object", "properties": {"n": {"type": "number"}}}}
    });
    let result = convert(spec);
    let all = requests(&result.collection.items);
    assert_eq!(all[0].name, "From a ref");
    assert_eq!(all[1].request.params, [row(false, "limit", "10")]);
    assert_eq!(raw_text(all[1]), "{\n  \"n\": 0\n}");
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
}

#[test]
fn recursive_schemas_stop_at_the_cycle() {
    let mut spec = v3(json!({
        "/nodes": {"post": {"requestBody": {"content": {"application/json": {
            "schema": {"$ref": "#/components/schemas/Node"}}}}}}
    }));
    spec["components"] = json!({"schemas": {
        "Node": {"type": "object", "properties": {
            "name": {"type": "string"},
            "children": {"type": "array", "items": {"$ref": "#/components/schemas/Node"}},
            "parent": {"$ref": "#/components/schemas/Node"}
        }},
        "A": {"$ref": "#/components/schemas/B"},
        "B": {"$ref": "#/components/schemas/A"}
    }});
    spec["paths"]["/loop"] = json!({"post": {"requestBody": {"content": {"application/json": {
        "schema": {"$ref": "#/components/schemas/A"}}}}}});
    let result = convert(spec);
    let all = requests(&result.collection.items);
    assert_eq!(
        raw_text(all[0]),
        "{\n  \"name\": \"string\",\n  \"children\": []\n}"
    );
    assert_eq!(raw_text(all[1]), "null");
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
}

#[test]
fn ref_chains_that_loop_are_reported() {
    let mut spec = v3(json!({
        "/x": {"get": {"parameters": [{"$ref": "#/components/parameters/A"}]}}
    }));
    spec["components"] = json!({"parameters": {
        "A": {"$ref": "#/components/parameters/B"},
        "B": {"$ref": "#/components/parameters/A"}
    }});
    let result = convert(spec);
    assert!(only_request(&result).request.params.is_empty());
    assert_eq!(result.warnings, ["1 $ref couldn't be resolved."]);
}

#[test]
fn external_and_broken_refs_are_warnings() {
    let result = convert(v3(json!({
        "/x": {"get": {"parameters": [
            {"$ref": "common.yaml#/parameters/Limit"},
            {"$ref": "https://example.com/p.json"},
            {"$ref": "common.yaml#/parameters/Limit"},
            {"$ref": "#/components/parameters/Missing"}
        ]}}
    })));
    assert!(only_request(&result).request.params.is_empty());
    assert_eq!(
        result.warnings,
        [
            "2 $refs to other files or URLs were not followed.",
            "1 $ref couldn't be resolved."
        ]
    );
}

#[test]
fn body_examples_win_over_generated_ones() {
    let mut spec = v3(json!({
        "/example": {"post": {"requestBody": {"content": {"application/json": {
            "example": {"b": 2, "a": 1},
            "schema": {"type": "object", "properties": {"z": {"type": "string"}}}
        }}}}},
        "/examples": {"post": {"requestBody": {"content": {"application/json": {
            "examples": {"first": {"$ref": "#/components/examples/One"}, "second": {"value": 2}},
            "schema": {"type": "object"}
        }}}}},
        "/schema-example": {"post": {"requestBody": {"content": {"application/json": {
            "schema": {"type": "object", "example": {"from": "schema"}}
        }}}}},
        "/string-example": {"post": {"requestBody": {"content": {"application/json": {
            "example": "{\"compact\":true}"
        }}}}},
        "/generated": {"post": {"requestBody": {"content": {"application/json": {
            "schema": {"allOf": [
                {"type": "object", "properties": {"id": {"type": "string", "format": "uuid"}}},
                {"type": "object", "properties": {"at": {"type": "string", "format": "date-time"},
                                                  "ok": {"type": "boolean"},
                                                  "kind": {"oneOf": [{"type": "integer"}, {"type": "string"}]},
                                                  "maybe": {"type": ["null", "string"]}}}
            ]}
        }}}}}
    }));
    spec["components"] = json!({"examples": {"One": {"value": {"one": 1}}}});
    let result = convert(spec);
    let texts: Vec<_> = requests(&result.collection.items)
        .into_iter()
        .map(raw_text)
        .map(str::to_string)
        .collect();
    assert_eq!(texts[0], "{\n  \"b\": 2,\n  \"a\": 1\n}");
    assert_eq!(texts[1], "{\n  \"one\": 1\n}");
    assert_eq!(texts[2], "{\n  \"from\": \"schema\"\n}");
    assert_eq!(texts[3], "{\n  \"compact\": true\n}");
    assert_eq!(
        texts[4],
        "{\n  \"id\": \"00000000-0000-0000-0000-000000000000\",\n  \"at\": \"2024-01-01T00:00:00Z\",\n  \"ok\": true,\n  \"kind\": 0,\n  \"maybe\": \"string\"\n}"
    );
}

#[test]
fn body_media_types_pick_json_then_forms_then_raw() {
    let result = convert(v3(json!({
        "/both": {"post": {"requestBody": {"content": {
            "application/xml": {"example": "<a/>"},
            "application/json": {"example": {"a": 1}}
        }}}},
        "/form": {"post": {"requestBody": {"content": {
            "text/plain": {},
            "application/x-www-form-urlencoded": {"schema": {
                "type": "object", "required": ["name"],
                "properties": {"name": {"type": "string", "example": "Rex"}, "age": {"type": "integer"}}
            }}
        }}}},
        "/multipart": {"post": {"requestBody": {"content": {
            "multipart/form-data": {"schema": {"type": "object", "properties": {
                "note": {"type": "string", "default": "hi"},
                "file": {"type": "string", "format": "binary"},
                "files": {"type": "array", "items": {"type": "string", "format": "binary"}}
            }}}
        }}}},
        "/xml": {"put": {"requestBody": {"content": {"text/xml": {"example": "<pet/>"}}}}},
        "/text": {"put": {"requestBody": {"content": {"text/plain": {"example": "hello"}}}}},
        "/vendor": {"patch": {"requestBody": {"content": {
            "application/merge-patch+json": {"example": {"a": null}}
        }}}},
        "/binary": {"put": {"requestBody": {"content": {"application/octet-stream": {}}}}}
    })));
    let all = requests(&result.collection.items);
    assert_eq!(
        all[0].request.body,
        Body::Raw {
            kind: RawKind::Json,
            text: "{\n  \"a\": 1\n}".into()
        }
    );
    assert_eq!(
        all[1].request.body,
        Body::UrlEncoded(vec![row(true, "name", "Rex"), row(false, "age", "")])
    );
    assert_eq!(
        all[2].request.body,
        Body::FormData(vec![
            row(false, "note", "hi"),
            row(false, "file", ""),
            row(false, "files", "")
        ])
    );
    assert_eq!(
        all[3].request.body,
        Body::Raw {
            kind: RawKind::Xml,
            text: "<pet/>".into()
        }
    );
    assert_eq!(
        all[3].request.headers,
        [row(true, "Content-Type", "text/xml")]
    );
    assert_eq!(
        all[4].request.body,
        Body::Raw {
            kind: RawKind::Text,
            text: "hello".into()
        }
    );
    assert!(all[4].request.headers.is_empty());
    assert_eq!(
        all[5].request.headers,
        [row(true, "Content-Type", "application/merge-patch+json")]
    );
    assert_eq!(all[6].request.body, Body::None);
    assert_eq!(
        result.warnings,
        [
            "2 file fields became empty text fields.",
            "1 request body of type application/octet-stream was left empty."
        ]
    );
}

#[test]
fn huge_generated_examples_are_cut_short() {
    let properties: serde_json::Map<String, serde_json::Value> = (0..5000)
        .map(|i| (format!("field{i}"), json!({"type": "integer"})))
        .collect();
    let deep = (0..20).fold(
        json!({"type": "string"}),
        |inner, _| json!({"type": "object", "properties": {"next": inner}}),
    );
    let result = convert(v3(json!({
        "/wide": {"post": {"requestBody": {"content": {"application/json": {
            "schema": {"type": "object", "properties": properties}}}}}},
        "/deep": {"post": {"requestBody": {"content": {"application/json": {"schema": deep}}}}}
    })));
    let all = requests(&result.collection.items);
    let wide: serde_json::Value = serde_json::from_str(raw_text(all[0])).unwrap();
    assert_eq!(wide.as_object().unwrap().len(), example::MAX_NODES - 1);
    let deep: serde_json::Value = serde_json::from_str(raw_text(all[1])).unwrap();
    let mut depth = 0;
    let mut current = &deep;
    while let Some(next) = current.get("next") {
        depth += 1;
        current = next;
    }
    assert_eq!(depth, example::MAX_DEPTH - 1);
    assert_eq!(
        result.warnings,
        ["2 generated example bodies were cut short."]
    );
}

fn with_security(schemes: serde_json::Value, security: serde_json::Value) -> serde_json::Value {
    let mut spec = v3(json!({"/x": {"get": {}}}));
    spec["components"] = json!({"securitySchemes": schemes});
    spec["security"] = security;
    spec
}

#[test]
fn each_supported_scheme_becomes_collection_auth_with_variables() {
    let cases = [
        (
            json!({"type": "apiKey", "name": "X-API-Key", "in": "header"}),
            Auth::ApiKey {
                key: "X-API-Key".into(),
                value: "{{apiKey}}".into(),
                location: ApiKeyLocation::Header,
            },
            vec!["apiKey"],
        ),
        (
            json!({"type": "apiKey", "name": "key", "in": "query"}),
            Auth::ApiKey {
                key: "key".into(),
                value: "{{apiKey}}".into(),
                location: ApiKeyLocation::Query,
            },
            vec!["apiKey"],
        ),
        (
            json!({"type": "http", "scheme": "Bearer", "bearerFormat": "JWT"}),
            Auth::Bearer {
                token: "{{token}}".into(),
            },
            vec!["token"],
        ),
        (
            json!({"type": "http", "scheme": "basic"}),
            Auth::Basic {
                username: "{{username}}".into(),
                password: "{{password}}".into(),
            },
            vec!["username", "password"],
        ),
    ];
    for (scheme, auth, variables) in cases {
        let result = convert(with_security(json!({"s": scheme}), json!([{"s": []}])));
        assert_eq!(result.collection.auth, auth);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let keys: Vec<_> = result.collection.variables[1..]
            .iter()
            .map(|row| (row.key.as_str(), row.value.as_str()))
            .collect();
        let expected: Vec<_> = variables.iter().map(|key| (*key, "")).collect();
        assert_eq!(keys, expected);
        assert_eq!(only_request(&result).request.auth, Auth::Inherit);
        let source = result.collection.source.as_ref().unwrap();
        assert_eq!(source.auth_hash, auth_hash(&auth));
    }
}

#[test]
fn swagger_2_basic_auth() {
    let spec = json!({
        "swagger": "2.0", "info": {"title": "T", "version": "1"}, "host": "h",
        "securityDefinitions": {"b": {"type": "basic"}},
        "security": [{"b": []}],
        "paths": {}
    });
    let result = convert(spec);
    assert!(matches!(result.collection.auth, Auth::Basic { .. }));
    assert_eq!(variable(&result, "baseUrl").unwrap(), "https://h");
}

#[test]
fn unsupported_schemes_leave_auth_empty_with_a_warning() {
    let cases = [
        (json!({"type": "oauth2", "flows": {}}), "OAuth 2.0"),
        (
            json!({"type": "openIdConnect", "openIdConnectUrl": "https://x"}),
            "OpenID Connect",
        ),
        (
            json!({"type": "apiKey", "name": "sid", "in": "cookie"}),
            "an API key in a cookie",
        ),
        (
            json!({"type": "http", "scheme": "digest"}),
            "HTTP digest auth",
        ),
    ];
    for (scheme, label) in cases {
        let result = convert(with_security(json!({"s": scheme}), json!([{"s": []}])));
        assert_eq!(result.collection.auth, Auth::None);
        assert_eq!(
            result.warnings,
            [format!(
                "The API uses {label}, which isn't supported; the collection's auth was left empty."
            )]
        );
        assert_eq!(result.collection.variables.len(), 1);
    }
}

#[test]
fn a_supported_alternative_wins_over_an_unsupported_one() {
    let schemes = json!({
        "oauth": {"type": "oauth2", "flows": {}},
        "key": {"type": "apiKey", "name": "k", "in": "header"}
    });
    let result = convert(with_security(
        schemes.clone(),
        json!([{"oauth": []}, {"key": []}]),
    ));
    assert!(matches!(result.collection.auth, Auth::ApiKey { .. }));
    assert!(result.warnings.is_empty());

    let result = convert(with_security(schemes.clone(), json!([{}, {"oauth": []}])));
    assert_eq!(result.collection.auth, Auth::None);
    assert!(result.warnings.is_empty());

    let result = convert(with_security(schemes, json!([{"key": [], "oauth": []}])));
    assert!(matches!(result.collection.auth, Auth::ApiKey { .. }));
    assert_eq!(
        result.warnings,
        ["The API requires several security schemes together; only the first was used."]
    );
}

#[test]
fn operation_security_overrides_the_global_one() {
    let mut spec = with_security(
        json!({
            "key": {"type": "apiKey", "name": "X-Key", "in": "header"},
            "bearer": {"type": "http", "scheme": "bearer"},
            "oauth": {"type": "oauth2", "flows": {}}
        }),
        json!([{"key": []}]),
    );
    spec["paths"] = json!({
        "/inherit": {"get": {}},
        "/same": {"get": {"security": [{"key": []}]}},
        "/public": {"get": {"security": []}},
        "/bearer": {"get": {"security": [{"bearer": []}]}},
        "/oauth1": {"get": {"security": [{"oauth": ["read"]}]}},
        "/oauth2": {"post": {"security": [{"oauth": ["write"]}]}},
        "/both": {"get": {"security": [{"bearer": [], "key": []}]}},
        "/undefined": {"get": {"security": [{"nope": []}]}}
    });
    let result = convert(spec);
    let auths: Vec<_> = requests(&result.collection.items)
        .iter()
        .map(|saved| saved.request.auth.clone())
        .collect();
    let bearer = Auth::Bearer {
        token: "{{token}}".into(),
    };
    assert_eq!(
        auths,
        [
            Auth::Inherit,
            Auth::Inherit,
            Auth::None,
            bearer.clone(),
            Auth::Inherit,
            Auth::Inherit,
            bearer,
            Auth::None,
        ]
    );
    let keys: Vec<_> = result
        .collection
        .variables
        .iter()
        .map(|row| row.key.as_str())
        .collect();
    assert_eq!(keys, ["baseUrl", "apiKey", "token"]);
    assert_eq!(
        result.warnings,
        [
            "The security scheme \"nope\" isn't defined.",
            "2 operations use OAuth 2.0, which isn't supported; their auth was left empty.",
            "1 operation requires several security schemes together; only its first was used.",
        ]
    );
}

#[test]
fn server_variables_fill_from_defaults() {
    let mut spec = v3(json!({}));
    spec["servers"] = json!([
        {"url": "{scheme}://{env}.example.com:{port}/{base}/",
         "variables": {
             "scheme": {"default": "https", "enum": ["https", "http"]},
             "env": {"default": "staging"},
             "port": {"default": "8443"},
             "base": {"enum": ["v2"], "default": "v2"}
         }},
        {"url": "https://other.example.com"}
    ]);
    let result = convert(spec);
    assert_eq!(
        variable(&result, "baseUrl").unwrap(),
        "https://staging.example.com:8443/v2"
    );
}

#[test]
fn relative_server_urls_resolve_against_the_source_url() {
    let mut spec = v3(json!({}));
    spec["servers"] = json!([{"url": "/api/v3"}]);
    let location = SourceLocation::Url("https://petstore3.swagger.io/api/v3/openapi.json".into());
    let result = import(&spec.to_string(), location).unwrap();
    assert_eq!(
        variable(&result, "baseUrl").unwrap(),
        "https://petstore3.swagger.io/api/v3"
    );
    assert!(result.warnings.is_empty());

    spec["servers"] = json!([{"url": "v1"}]);
    let location = SourceLocation::Url("https://example.com/specs/openapi.yaml".into());
    let result = import(&spec.to_string(), location).unwrap();
    assert_eq!(
        variable(&result, "baseUrl").unwrap(),
        "https://example.com/specs/v1"
    );

    spec.as_object_mut().unwrap().remove("servers");
    let location = SourceLocation::Url("https://example.com/specs/openapi.yaml".into());
    let result = import(&spec.to_string(), location).unwrap();
    assert_eq!(variable(&result, "baseUrl").unwrap(), "https://example.com");

    let result = convert(spec);
    assert_eq!(variable(&result, "baseUrl").unwrap(), "");
    assert_eq!(
        result.warnings,
        ["The spec doesn't say where the API is; set baseUrl."]
    );
}

#[test]
fn missing_title_uses_the_default_name() {
    let result = import(r#"{"openapi": "3.1.0", "info": {"version": 2}}"#, file()).unwrap();
    assert_eq!(result.collection.name, DEFAULT_NAME);
    let source = result.collection.source.unwrap();
    assert_eq!(source.title, "");
    assert_eq!(source.version, "2");
}

#[test]
fn invalid_documents_are_clear_errors() {
    let not_openapi = [
        r#"{"name": "a package.json"}"#,
        "[1, 2]",
        "just: yaml\n",
        "plain text",
    ];
    for text in not_openapi {
        assert_eq!(
            import(text, file()).unwrap_err(),
            ImportError::NotOpenApi,
            "{text}"
        );
    }
    assert_eq!(
        ImportError::NotOpenApi.to_string(),
        "This isn't an OpenAPI or Swagger document."
    );

    let err = import(r#"{"swagger": "1.2"}"#, file()).unwrap_err();
    assert_eq!(err, ImportError::UnsupportedVersion("Swagger 1.2".into()));
    assert_eq!(
        err.to_string(),
        "Swagger 1.2 isn't supported. Use OpenAPI 3.0 or 3.1, or Swagger 2.0."
    );
    let err = import(r#"{"openapi": "2.0"}"#, file()).unwrap_err();
    assert_eq!(err, ImportError::UnsupportedVersion("OpenAPI 2.0".into()));

    let err = import("{\n  \"openapi\": \"3.0.0\",\n  \"info\": {\n}", file()).unwrap_err();
    let ImportError::Parse { line, message } = &err else {
        panic!("expected a parse error, got {err:?}");
    };
    assert_eq!(*line, Some(4));
    assert!(!message.contains("line"), "{message}");
    assert!(err.to_string().ends_with("(line 4)."), "{err}");

    let err = import("openapi: 3.0.0\ninfo: [unclosed\n", file()).unwrap_err();
    assert!(
        matches!(err, ImportError::Parse { line: Some(_), .. }),
        "{err:?}"
    );
}

#[test]
fn newer_3x_versions_import_with_a_note() {
    let result = import(r#"{"openapi": "3.2.0", "info": {"title": "T"}}"#, file()).unwrap();
    assert_eq!(
        result.warnings,
        [
            "OpenAPI 3.2 is newer than 3.1; parts of it may not import.",
            "The spec doesn't say where the API is; set baseUrl."
        ]
    );
}

#[test]
fn trace_operations_are_skipped() {
    let result = convert(v3(json!({
        "/a": {"get": {}, "trace": {}},
        "/b": {"trace": {}},
        "/c": {"summary": "path summary", "description": "d", "post": {}}
    })));
    assert_eq!(requests(&result.collection.items).len(), 2);
    assert_eq!(result.warnings, ["2 TRACE operations were skipped."]);
}

const PETSTORE_YAML: &str = r#"
openapi: 3.0.0
info:
  title: YAML Pets
  version: 1.0.0
servers:
  - url: https://pets.example.com
x-defaults: &defaults
  name: limit
  in: query
paths:
  /pets:
    get:
      summary: List pets
      tags: [pets]
      parameters:
        - <<: *defaults
          schema:
            type: integer
            default: 10
      responses:
        200:
          description: OK
    post:
      summary: Create
      tags: [pets]
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/Pet'
components:
  schemas:
    Pet:
      type: object
      properties:
        name:
          type: string
          example: Rex
        age:
          type: integer
"#;

#[test]
fn yaml_specs_convert() {
    let result = import(PETSTORE_YAML, file()).unwrap();
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert_eq!(result.collection.name, "YAML Pets");
    assert_eq!(
        variable(&result, "baseUrl").unwrap(),
        "https://pets.example.com"
    );
    let all = requests(&result.collection.items);
    assert_eq!(all[0].request.params, [row(false, "limit", "10")]);
    assert_eq!(raw_text(all[1]), "{\n  \"name\": \"Rex\",\n  \"age\": 0\n}");
}

#[test]
fn yaml_with_unquoted_version_numbers() {
    let yaml = "swagger: 2.0\ninfo:\n  title: T\n  version: 1.0\nhost: h\npaths: {}\n";
    let result = import(yaml, file()).unwrap();
    assert_eq!(result.collection.source.unwrap().version, "1.0");
}
