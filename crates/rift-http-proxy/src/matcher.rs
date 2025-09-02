use regex::Regex;
use std::sync::Arc;
use crate::config::{Rule, MatchConfig};

#[derive(Clone)]
pub struct CompiledRule {
    pub id: String,
    pub path_matcher: Option<PathMatcher>,
    pub method: Option<String>,
    pub rule: Arc<Rule>,
}

#[derive(Clone)]
pub enum PathMatcher {
    Exact(String),
    Prefix(String),
    Regex(Regex),
}

impl PathMatcher {
    pub fn matches(&self, path: &str) -> bool {
        match self {
            PathMatcher::Exact(p) => path == p,
            PathMatcher::Prefix(p) => path.starts_with(p),
            PathMatcher::Regex(r) => r.is_match(path),
        }
    }
}

pub fn compile_rules(rules: &[Rule]) -> Vec<CompiledRule> {
    rules.iter().enumerate().map(|(i, rule)| {
        let id = rule.id.clone().unwrap_or_else(|| format!("rule_{}", i));
        let path_matcher = rule.match_config.as_ref()
            .and_then(|m| m.path.as_ref())
            .map(|p| {
                if p.starts_with("~") {
                    PathMatcher::Regex(Regex::new(&p[1..]).unwrap())
                } else if p.ends_with("*") {
                    PathMatcher::Prefix(p[..p.len()-1].to_string())
                } else {
                    PathMatcher::Exact(p.clone())
                }
            });
        let method = rule.match_config.as_ref()
            .and_then(|m| m.method.clone());

        CompiledRule { id, path_matcher, method, rule: Arc::new(rule.clone()) }
    }).collect()
}

pub fn find_matching_rule<'a>(rules: &'a [CompiledRule], path: &str, method: &str) -> Option<&'a CompiledRule> {
    rules.iter().find(|r| {
        let path_match = r.path_matcher.as_ref().map(|m| m.matches(path)).unwrap_or(true);
        let method_match = r.method.as_ref().map(|m| m.eq_ignore_ascii_case(method)).unwrap_or(true);
        path_match && method_match
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_path() {
        let m = PathMatcher::Exact("/api".to_string());
        assert!(m.matches("/api"));
        assert!(!m.matches("/api/v1"));
    }

    #[test]
    fn test_prefix_path() {
        let m = PathMatcher::Prefix("/api".to_string());
        assert!(m.matches("/api"));
        assert!(m.matches("/api/v1"));
    }

    #[test]
    fn test_regex_path() {
        let m = PathMatcher::Regex(Regex::new(r"/api/v\d+").unwrap());
        assert!(m.matches("/api/v1"));
        assert!(!m.matches("/api/test"));
    }
}
