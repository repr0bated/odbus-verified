#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_trusted_session_bypass() {
        let validator = InputValidator::new();
        let schema = json!({"type": "object"});
        
        // Trusted session should pass even with invalid input
        let result = validator.validate_input(
            "test_tool",
            &json!({"invalid": "data"}),
            &schema,
            Some("chatbot"),
        ).await.unwrap();
        
        assert!(result.session_trusted);
        assert!(result.should_proceed());
    }

    #[tokio::test]
    async fn test_non_trusted_restriction() {
        let validator = InputValidator::new();
        let schema = json!({"type": "object"});
        
        // Non-trusted session should be restricted
        let result = validator.validate_input(
            "shell_tool",
            &json!({"command": "rm -rf /etc/passwd"}),
            &schema,
            Some("anonymous"),
        ).await.unwrap();
        
        assert!(!result.session_trusted);
        assert!(!result.should_proceed());
        assert!(!result.validation_errors.is_empty());
    }

    #[tokio::test]
    async fn test_input_sanitization() {
        let validator = InputValidator::new();
        let schema = json!({"type": "object"});
        
        let input_with_null_bytes = json!({"text": "hello\x00world"});
        let result = validator.validate_input(
            "text_tool",
            &input_with_null_bytes,
            &schema,
            Some("anonymous"),
        ).await.unwrap();
        
        let sanitized_text = result.input["text"].as_str().unwrap();
        assert!(!sanitized_text.contains('\0'));
        assert_eq!(sanitized_text, "helloworld");
        assert!(result.was_sanitized);
    }

    #[tokio::test]
    async fn test_schema_validation() {
        let validator = InputValidator::new();
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "number"}
            },
            "required": ["name"]
        });
        
        // Valid input should pass
        let valid_input = json!({"name": "test", "age": 25});
        let result = validator.validate_input(
            "test_tool",
            &valid_input,
            &schema,
            Some("anonymous"),
        ).await.unwrap();
        
        assert!(result.is_valid);
        assert!(result.should_proceed());
        
        // Invalid input should fail
        let invalid_input = json!({"age": 25}); // missing required "name"
        let result = validator.validate_input(
            "test_tool",
            &invalid_input,
            &schema,
            Some("anonymous"),
        ).await.unwrap();
        
        assert!(!result.is_valid);
        assert!(!result.should_proceed());
        assert!(!result.validation_errors.is_empty());
    }
}