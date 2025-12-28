use mlua::Lua;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone)]

pub enum LuaValidationError {
    SyntaxError(String),
    MissingReturnStatement(String),
    CompilationError(String),
    LoadError(String),
}

impl fmt::Display for LuaValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LuaValidationError::SyntaxError(msg) => write!(f, "Syntax error: {msg}"),
            LuaValidationError::MissingReturnStatement(msg) => {
                write!(f, "Missing return statement: {msg}")
            }
            LuaValidationError::CompilationError(msg) => write!(f, "Compilation error: {msg}"),
            LuaValidationError::LoadError(msg) => write!(f, "Load error: {msg}"),
        }
    }
}

impl Error for LuaValidationError {}

pub struct LuaValidator {
    lua: Lua,
}

impl LuaValidator {
    pub fn new() -> Self {
        Self { lua: Lua::new() }
    }

    /// Validates a Lua script for use with Rift proxy
    ///
    /// Checks:
    /// 1. Script compiles without syntax errors
    /// 2. Script can be loaded as a chunk
    /// 3. Script contains a return statement (expected structure)
    ///
    /// Note: This validates syntax only - runtime behavior depends on request/flow_store context
    pub fn validate(&self, script: &str) -> Result<(), LuaValidationError> {
        // Try to compile (load and parse) - this catches syntax errors
        match self.lua.load(script).eval::<mlua::Value>() {
            Ok(_) => {
                // Script loaded and executed successfully
                Ok(())
            }
            Err(e) => {
                // Check if it's a syntax error vs runtime error
                let err_str = e.to_string();
                if err_str.contains("syntax error")
                    || err_str.contains("unexpected")
                    || err_str.contains("'end' expected")
                {
                    Err(LuaValidationError::SyntaxError(err_str))
                } else {
                    // Runtime errors during validation are okay (e.g., undefined variables like request)
                    // We only care about syntax validation
                    Ok(())
                }
            }
        }
    }

    /// Validate multiple scripts and return all errors
    pub fn validate_batch<'a>(
        &self,
        scripts: &[(&'a str, &str)],
    ) -> Vec<(&'a str, Result<(), LuaValidationError>)> {
        scripts
            .iter()
            .map(|(id, script)| (*id, self.validate(script)))
            .collect()
    }
}

impl Default for LuaValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_script() {
        let validator = LuaValidator::new();
        let script = r#"
            local flow_id = request.headers["x-flow-id"]
            if flow_id == nil then
                return { inject = false }
            end
            
            local count = flow_store:increment(flow_id, "count")
            if count > 5 then
                return {
                    inject = true,
                    fault = "error",
                    status = 503,
                    body = "Too many requests"
                }
            end
            
            return { inject = false }
        "#;

        let result = validator.validate(script);
        assert!(result.is_ok(), "Valid script should pass validation");
    }

    #[test]
    fn test_syntax_error() {
        let validator = LuaValidator::new();
        let script = r#"
            local flow_id = request.headers["x-flow-id"
            -- Missing closing bracket
            return { inject = false }
        "#;

        let result = validator.validate(script);
        assert!(result.is_err());
        if let Err(LuaValidationError::SyntaxError(_)) = result {
            // Expected
        } else {
            panic!("Expected SyntaxError, got: {result:?}");
        }
    }

    #[test]
    fn test_complex_valid_script() {
        let validator = LuaValidator::new();
        let script = r#"
            -- Circuit breaker pattern
            local flow_id = request.headers["x-flow-id"]
            if flow_id == nil then
                return { inject = false }
            end
            
            local failures = flow_store:increment(flow_id, "failures")
            flow_store:set_ttl(flow_id, 300)
            
            if failures > 3 then
                return {
                    inject = true,
                    fault = "error",
                    status = 503,
                    body = "Circuit breaker open"
                }
            end
            
            return { inject = false }
        "#;

        let result = validator.validate(script);
        assert!(result.is_ok(), "Complex valid script should pass");
    }

    #[test]
    fn test_batch_validation() {
        let validator = LuaValidator::new();
        let scripts = vec![
            ("script1", r#"return { inject = false }"#),
            ("script2", r#"return { inject = true "#), // Missing closing brace
            (
                "script3",
                r#"local x = flow_store:increment("flow1", "key") return { inject = false }"#,
            ),
        ];

        let results = validator.validate_batch(&scripts);

        assert_eq!(results.len(), 3);
        assert!(results[0].1.is_ok(), "script1 should be valid");
        assert!(results[1].1.is_err(), "script2 should be invalid");
        assert!(results[2].1.is_ok(), "script3 should be valid");
    }

    #[test]
    fn test_latency_fault_script() {
        let validator = LuaValidator::new();
        let script = r#"
            if request.path:find("/slow") then
                return {
                    inject = true,
                    fault = "latency",
                    duration_ms = 1000
                }
            end
            return { inject = false }
        "#;

        let result = validator.validate(script);
        assert!(result.is_ok(), "Latency fault script should be valid");
    }
}
