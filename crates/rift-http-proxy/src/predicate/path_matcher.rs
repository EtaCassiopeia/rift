//! Path matching configuration and compilation.
//!
//! Supports Mountebank-style path predicates with backward compatibility
//! for existing Rift config format.

use super::options::PredicateOptions;
use super::string_matcher::StringMatcher;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Path matching configuration with full predicate support.
///
/// Backward compatible with existing Rift config format while supporting
/// new Mountebank-style predicates.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(untagged)]
pub enum PathMatcher {
    /// Match any path (default)
    #[default]
    Any,

    /// Exact path match (backward compatible): { exact: "/api/users" }
    Exact { exact: String },

    /// Prefix match (backward compatible): { prefix: "/api" }
    Prefix { prefix: String },

    /// Regex match (backward compatible): { regex: "^/api/v\\d+/" }
    Regex { regex: String },

    /// Contains substring: { contains: "/api" }
    Contains { contains: String },

    /// Ends with suffix: { endsWith: ".json" }
    EndsWith {
        #[serde(rename = "endsWith")]
        ends_with: String,
    },

    /// Full predicate with options
    Full {
        #[serde(flatten)]
        matcher: StringMatcher,
        #[serde(flatten, default)]
        options: PredicateOptions,
    },
}

/// Compiled path matcher for efficient runtime evaluation.
#[derive(Debug, Clone)]
pub enum CompiledPathMatcher {
    Any,
    Exact { value: String, lower: String },
    Prefix { value: String, lower: String },
    Contains { value: String, lower: String },
    EndsWith { value: String, lower: String },
    Regex(Arc<Regex>),
}

/// Compiled path match configuration including options.
#[derive(Debug, Clone)]
pub struct CompiledPathMatch {
    pub matcher: CompiledPathMatcher,
    pub case_sensitive: bool,
}

impl CompiledPathMatch {
    /// Compile a PathMatcher configuration.
    pub fn compile(config: &PathMatcher) -> Result<Self, regex::Error> {
        match config {
            PathMatcher::Any => Ok(CompiledPathMatch {
                matcher: CompiledPathMatcher::Any,
                case_sensitive: true,
            }),

            PathMatcher::Exact { exact } => Ok(CompiledPathMatch {
                matcher: CompiledPathMatcher::Exact {
                    value: exact.clone(),
                    lower: exact.to_lowercase(),
                },
                case_sensitive: true,
            }),

            PathMatcher::Prefix { prefix } => Ok(CompiledPathMatch {
                matcher: CompiledPathMatcher::Prefix {
                    value: prefix.clone(),
                    lower: prefix.to_lowercase(),
                },
                case_sensitive: true,
            }),

            PathMatcher::Regex { regex } => Ok(CompiledPathMatch {
                matcher: CompiledPathMatcher::Regex(Arc::new(Regex::new(regex)?)),
                case_sensitive: true,
            }),

            PathMatcher::Contains { contains } => Ok(CompiledPathMatch {
                matcher: CompiledPathMatcher::Contains {
                    value: contains.clone(),
                    lower: contains.to_lowercase(),
                },
                case_sensitive: true,
            }),

            PathMatcher::EndsWith { ends_with } => Ok(CompiledPathMatch {
                matcher: CompiledPathMatcher::EndsWith {
                    value: ends_with.clone(),
                    lower: ends_with.to_lowercase(),
                },
                case_sensitive: true,
            }),

            PathMatcher::Full { matcher, options } => {
                let compiled = match matcher {
                    StringMatcher::Equals(v) => CompiledPathMatcher::Exact {
                        value: v.clone(),
                        lower: v.to_lowercase(),
                    },
                    StringMatcher::Contains(v) => CompiledPathMatcher::Contains {
                        value: v.clone(),
                        lower: v.to_lowercase(),
                    },
                    StringMatcher::StartsWith(v) => CompiledPathMatcher::Prefix {
                        value: v.clone(),
                        lower: v.to_lowercase(),
                    },
                    StringMatcher::EndsWith(v) => CompiledPathMatcher::EndsWith {
                        value: v.clone(),
                        lower: v.to_lowercase(),
                    },
                    StringMatcher::Matches(pattern) => {
                        CompiledPathMatcher::Regex(Arc::new(Regex::new(pattern)?))
                    }
                    StringMatcher::Exists(_) => CompiledPathMatcher::Any, // Path always exists
                };

                Ok(CompiledPathMatch {
                    matcher: compiled,
                    case_sensitive: options.case_sensitive,
                })
            }
        }
    }

    /// Check if a path matches this matcher.
    pub fn matches(&self, path: &str) -> bool {
        match &self.matcher {
            CompiledPathMatcher::Any => true,

            CompiledPathMatcher::Exact { value, lower } => {
                if self.case_sensitive {
                    path == value
                } else {
                    path.to_lowercase() == *lower
                }
            }

            CompiledPathMatcher::Prefix { value, lower } => {
                if self.case_sensitive {
                    path.starts_with(value.as_str())
                } else {
                    path.to_lowercase().starts_with(lower.as_str())
                }
            }

            CompiledPathMatcher::Contains { value, lower } => {
                if self.case_sensitive {
                    path.contains(value.as_str())
                } else {
                    path.to_lowercase().contains(lower.as_str())
                }
            }

            CompiledPathMatcher::EndsWith { value, lower } => {
                if self.case_sensitive {
                    path.ends_with(value.as_str())
                } else {
                    path.to_lowercase().ends_with(lower.as_str())
                }
            }

            CompiledPathMatcher::Regex(regex) => regex.is_match(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_matcher_backward_compatible() {
        // Test existing Rift config format works
        let exact = CompiledPathMatch::compile(&PathMatcher::Exact {
            exact: "/api/users".to_string(),
        })
        .unwrap();
        assert!(exact.matches("/api/users"));
        assert!(!exact.matches("/api/users/1"));

        let prefix = CompiledPathMatch::compile(&PathMatcher::Prefix {
            prefix: "/api".to_string(),
        })
        .unwrap();
        assert!(prefix.matches("/api"));
        assert!(prefix.matches("/api/users"));
        assert!(!prefix.matches("/other"));

        let regex = CompiledPathMatch::compile(&PathMatcher::Regex {
            regex: r"^/api/v\d+/.*".to_string(),
        })
        .unwrap();
        assert!(regex.matches("/api/v1/users"));
        assert!(!regex.matches("/api/users"));
    }

    #[test]
    fn test_path_matcher_new_operators() {
        let contains = CompiledPathMatch::compile(&PathMatcher::Contains {
            contains: "users".to_string(),
        })
        .unwrap();
        assert!(contains.matches("/api/users"));
        assert!(contains.matches("/users/list"));
        assert!(!contains.matches("/api/items"));

        let ends_with = CompiledPathMatch::compile(&PathMatcher::EndsWith {
            ends_with: ".json".to_string(),
        })
        .unwrap();
        assert!(ends_with.matches("/data.json"));
        assert!(!ends_with.matches("/data.xml"));
    }

    #[test]
    fn test_path_matcher_serde() {
        // Exact path (backward compatible)
        let json = r#"{"exact": "/api/users"}"#;
        let matcher: PathMatcher = serde_json::from_str(json).unwrap();
        assert!(matches!(matcher, PathMatcher::Exact { .. }));

        // Prefix path (backward compatible)
        let json = r#"{"prefix": "/api"}"#;
        let matcher: PathMatcher = serde_json::from_str(json).unwrap();
        assert!(matches!(matcher, PathMatcher::Prefix { .. }));

        // Regex path (backward compatible)
        let json = r#"{"regex": "^/api/v\\d+"}"#;
        let matcher: PathMatcher = serde_json::from_str(json).unwrap();
        assert!(matches!(matcher, PathMatcher::Regex { .. }));

        // New contains
        let json = r#"{"contains": "users"}"#;
        let matcher: PathMatcher = serde_json::from_str(json).unwrap();
        assert!(matches!(matcher, PathMatcher::Contains { .. }));

        // New endsWith
        let json = r#"{"endsWith": ".json"}"#;
        let matcher: PathMatcher = serde_json::from_str(json).unwrap();
        assert!(matches!(matcher, PathMatcher::EndsWith { .. }));
    }
}
