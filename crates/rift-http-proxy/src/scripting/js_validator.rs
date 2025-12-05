use boa_engine::{js_string, Context, Source};
use thiserror::Error;

/// JavaScript validation error
#[derive(Debug, Error)]
pub enum JsValidationError {
    #[error("Missing required function: {0}")]
    MissingFunction(String),
    #[error("Evaluation error: {0}")]
    EvaluationError(String),
}

/// JavaScript script validator
pub struct JsValidator;

impl JsValidator {
    /// Validate JavaScript script syntax
    pub fn validate(script: &str) -> Result<(), JsValidationError> {
        // Create a context for validation
        let mut context = Context::default();

        // Try to evaluate the script (this parses and executes top-level code)
        context
            .eval(Source::from_bytes(script.as_bytes()))
            .map_err(|e| JsValidationError::EvaluationError(e.to_string()))?;

        // Check that should_inject function exists
        let global = context.global_object();
        let func = global.get(js_string!("should_inject"), &mut context);
        match func {
            Ok(val) if val.is_callable() => Ok(()),
            _ => Err(JsValidationError::MissingFunction(
                "should_inject".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_script() {
        let script = r#"
function should_inject(request, flow_store) {
    return {inject: false};
}
"#;
        assert!(JsValidator::validate(script).is_ok());
    }

    #[test]
    fn test_syntax_error() {
        let script = r#"
function should_inject(request, flow_store {
    return {inject: false};
}
"#;
        let result = JsValidator::validate(script);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_function() {
        let script = r#"
function some_other_function(request, flow_store) {
    return {inject: false};
}
"#;
        let result = JsValidator::validate(script);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            JsValidationError::MissingFunction(_)
        ));
    }

    #[test]
    fn test_complex_script() {
        let script = r#"
function should_inject(request, flow_store) {
    var flowId = request.headers["x-flow-id"];
    if (!flowId) {
        return {inject: false};
    }

    var attempts = flow_store.increment(flowId, "attempts");

    if (attempts <= 2) {
        return {
            inject: true,
            fault: "error",
            status: 503,
            body: "Service temporarily unavailable"
        };
    }

    return {inject: false};
}
"#;
        assert!(JsValidator::validate(script).is_ok());
    }
}
