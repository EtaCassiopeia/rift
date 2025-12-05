use rhai::{Engine, AST};
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ValidationError {
    SyntaxError(String),
    MissingFunction(String),
    InvalidSignature(String),
    CompilationError(String),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::SyntaxError(msg) => write!(f, "Syntax error: {msg}"),
            ValidationError::MissingFunction(func) => {
                write!(f, "Missing required function: {func}")
            }
            ValidationError::InvalidSignature(msg) => {
                write!(f, "Invalid function signature: {msg}")
            }
            ValidationError::CompilationError(msg) => write!(f, "Compilation error: {msg}"),
        }
    }
}

impl Error for ValidationError {}

#[allow(dead_code)]
pub struct RhaiValidator {
    #[allow(dead_code)]
    engine: Engine,
}

impl RhaiValidator {
    #[allow(dead_code)]
    pub fn new() -> Self {
        let engine = Engine::new();
        Self { engine }
    }

    /// Validates a Rhai script for use with Rift proxy
    ///
    /// Checks:
    /// 1. Script compiles without syntax errors
    /// 2. Basic structural validity (contains function definition)
    ///
    /// Note: This does NOT validate runtime behavior - only syntax.
    /// The actual should_inject function must be verified at runtime.
    #[allow(dead_code)]
    pub fn validate(&self, script: &str) -> Result<AST, ValidationError> {
        // Compile the script - this catches syntax errors
        let ast = self
            .engine
            .compile(script)
            .map_err(|e| ValidationError::SyntaxError(e.to_string()))?;

        // Basic check: script should contain "should_inject"
        if !script.contains("should_inject") {
            return Err(ValidationError::MissingFunction(
                "should_inject function not found in script".to_string(),
            ));
        }

        Ok(ast)
    }

    /// Validate multiple scripts and return all errors
    #[allow(dead_code)]
    pub fn validate_batch<'a>(
        &self,
        scripts: &[(&'a str, &str)],
    ) -> Vec<(&'a str, Result<AST, ValidationError>)> {
        scripts
            .iter()
            .map(|(id, script)| (*id, self.validate(script)))
            .collect()
    }
}

impl Default for RhaiValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_script() {
        let validator = RhaiValidator::new();
        let script = r#"
            fn should_inject(request, flow_store) {
                return #{ inject: true, fault: "latency", duration_ms: 100 };
            }
        "#;

        let result = validator.validate(script);
        assert!(result.is_ok(), "Valid script should pass validation");
    }

    #[test]
    fn test_syntax_error() {
        let validator = RhaiValidator::new();
        let script = r#"
            fn should_inject(request, flow_store) {
                return #{ inject: true  // Missing closing brace
            }
        "#;

        let result = validator.validate(script);
        assert!(result.is_err());
        if let Err(ValidationError::SyntaxError(_)) = result {
            // Expected
        } else {
            panic!("Expected SyntaxError");
        }
    }

    #[test]
    fn test_missing_function() {
        let validator = RhaiValidator::new();
        let script = r#"
            fn wrong_function_name(request, flow_store) {
                return #{ inject: false };
            }
        "#;

        let result = validator.validate(script);
        assert!(result.is_err());
        if let Err(ValidationError::MissingFunction(_)) = result {
            // Expected - error message will contain "should_inject"
        } else {
            panic!("Expected MissingFunction error");
        }
    }

    #[test]
    fn test_complex_valid_script() {
        let validator = RhaiValidator::new();
        let script = r#"
            fn should_inject(request, flow_store) {
                let path = request.path;
                if path.contains("/api/") {
                    let flow_id = request.headers["x-flow-id"];
                    let attempts = flow_store.increment(flow_id, "attempts");
                    
                    if attempts <= 2 {
                        return #{ inject: true, fault: "error", status: 503 };
                    }
                }
                return #{ inject: false };
            }
        "#;

        let result = validator.validate(script);
        assert!(result.is_ok(), "Complex valid script should pass");
    }

    #[test]
    fn test_batch_validation() {
        let validator = RhaiValidator::new();
        let scripts = vec![
            (
                "script1",
                r#"fn should_inject(req, fs) { return #{ inject: false }; }"#,
            ),
            ("script2", r#"fn wrong_name() { return true; }"#),
            (
                "script3",
                r#"fn should_inject(req, fs) { return #{ inject: true, fault: "latency", duration_ms: 50 }; }"#,
            ),
        ];

        let results = validator.validate_batch(&scripts);

        assert_eq!(results.len(), 3);
        assert!(results[0].1.is_ok(), "script1 should be valid");
        assert!(results[1].1.is_err(), "script2 should be invalid");
        assert!(results[2].1.is_ok(), "script3 should be valid");
    }
}
