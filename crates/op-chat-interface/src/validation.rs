//! Schema_Validator (spec task 5.1): runs inside the executor and the
//! mounted-file runner paths — the spec's replacement note forbids a
//! second standalone validator. Rust equivalent of the Ajv2020 setup:
//! no coercion, no defaults, no removing additional properties, all
//! errors collected. Failures list path and keyword, never the value.

use serde::Serialize;

pub const MAX_ARGUMENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationFailure {
    pub path: String,
    pub keyword: String,
}

#[derive(Debug)]
pub enum ValidationOutcome {
    Accepted { input_bytes: Vec<u8> },
    Rejected { failures: Vec<ValidationFailure> },
}

pub fn validate_arguments(schema: &serde_json::Value, arguments: &serde_json::Value) -> ValidationOutcome {
    let validator = match jsonschema::validator_for(schema) {
        Ok(v) => v,
        Err(_) => {
            return ValidationOutcome::Rejected {
                failures: vec![ValidationFailure {
                    path: String::new(),
                    keyword: "schema".to_string(),
                }],
            };
        }
    };
    let failures: Vec<ValidationFailure> = validator
        .iter_errors(arguments)
        .map(|error| ValidationFailure {
            path: error.instance_path.to_string(),
            keyword: format!("{:?}", error.kind),
        })
        .collect();
    if !failures.is_empty() {
        return ValidationOutcome::Rejected { failures };
    }
    // Render the accepted value exactly once into the call input bytes.
    let input_bytes = serde_json::to_vec(arguments).unwrap_or_default();
    if input_bytes.len() > MAX_ARGUMENT_BYTES {
        return ValidationOutcome::Rejected {
            failures: vec![ValidationFailure {
                path: String::new(),
                keyword: "maxBytes 1048576".to_string(),
            }],
        };
    }
    ValidationOutcome::Accepted { input_bytes }
}
