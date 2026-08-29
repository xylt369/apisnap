use crate::engine::masker::*;
use serde_json::{json, Map, Value};

/// Infers an OpenAPI 3.1 / JSON Schema representation from a masked JSON AST.
pub fn infer_schema_from_value(val: &Value) -> Value {
    match val {
        Value::Null => json!({ "type": "null" }),
        Value::Bool(_) => json!({ "type": "boolean" }),
        Value::Number(num) => {
            if num.is_i64() || num.is_u64() {
                json!({ "type": "integer" })
            } else {
                json!({ "type": "number" })
            }
        }
        Value::String(s) => infer_string_schema(s),
        Value::Array(arr) => {
            if arr.is_empty() {
                json!({
                    "type": "array",
                    "items": {}
                })
            } else {
                // Infer item schema from first element (or union)
                let item_schema = infer_schema_from_value(&arr[0]);
                json!({
                    "type": "array",
                    "items": item_schema
                })
            }
        }
        Value::Object(map) => {
            let mut properties = Map::new();
            let mut required = Vec::new();

            for (k, v) in map {
                properties.insert(k.clone(), infer_schema_from_value(v));
                required.push(k.clone());
            }

            json!({
                "type": "object",
                "properties": properties,
                "required": required
            })
        }
    }
}

fn infer_string_schema(s: &str) -> Value {
    match s {
        MASKED_UUID => json!({
            "type": "string",
            "format": "uuid",
            "example": "550e8400-e29b-41d4-a716-446655440000"
        }),
        MASKED_TIMESTAMP => json!({
            "type": "string",
            "format": "date-time",
            "example": "2026-08-30T00:00:00Z"
        }),
        MASKED_EMAIL => json!({
            "type": "string",
            "format": "email",
            "example": "user@example.com"
        }),
        MASKED_JWT => json!({
            "type": "string",
            "format": "jwt",
            "description": "JWT Bearer Token"
        }),
        MASKED_OBJECT_ID => json!({
            "type": "string",
            "format": "objectid",
            "example": "507f1f77bcf86cd799439011"
        }),
        MASKED_CREDIT_CARD => json!({
            "type": "string",
            "format": "credit-card"
        }),
        MASKED_SSN => json!({
            "type": "string",
            "format": "ssn"
        }),
        MASKED_EPOCH => json!({
            "type": "integer",
            "description": "Unix epoch timestamp"
        }),
        REDACTED => json!({
            "type": "string",
            "description": "Redacted sensitive field"
        }),
        _ => json!({
            "type": "string",
            "example": s
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_inference_with_mask_tokens() {
        let input = json!({
            "id": "<MASKED_UUID>",
            "created_at": "<MASKED_TIMESTAMP>",
            "email": "<MASKED_EMAIL>",
            "age": 28,
            "roles": ["admin"]
        });

        let schema = infer_schema_from_value(&input);
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["id"]["format"], "uuid");
        assert_eq!(schema["properties"]["created_at"]["format"], "date-time");
        assert_eq!(schema["properties"]["email"]["format"], "email");
        assert_eq!(schema["properties"]["age"]["type"], "integer");
        assert_eq!(schema["properties"]["roles"]["type"], "array");
    }
}
