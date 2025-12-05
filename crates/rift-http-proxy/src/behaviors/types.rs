//! Configuration types for response behaviors.

use super::copy::CopyBehavior;
use super::lookup::LookupBehavior;
use super::wait::WaitBehavior;
use serde::{Deserialize, Serialize};

/// Response behaviors that modify how responses are generated
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResponseBehaviors {
    /// Add latency before response
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<WaitBehavior>,

    /// Repeat response N times before advancing to next
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<u32>,

    /// Copy fields from request to response
    /// Mountebank allows both single object and array format
    #[serde(
        default,
        deserialize_with = "deserialize_copy_behaviors",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub copy: Vec<CopyBehavior>,

    /// Lookup from external data source
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lookup: Vec<LookupBehavior>,

    /// Shell transform - external program transforms response
    /// The program receives MB_REQUEST and MB_RESPONSE env vars
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_transform: Option<String>,

    /// Decorate - Rhai script to post-process response (Mountebank-compatible)
    /// Script receives `request` and `response` variables and can modify response
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decorate: Option<String>,
}

/// Custom deserializer for copy behaviors that accepts both object and array
fn deserialize_copy_behaviors<'de, D>(deserializer: D) -> Result<Vec<CopyBehavior>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct CopyBehaviorsVisitor;

    impl<'de> Visitor<'de> for CopyBehaviorsVisitor {
        type Value = Vec<CopyBehavior>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a copy behavior object or array of copy behaviors")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut behaviors = Vec::new();
            while let Some(behavior) = seq.next_element()? {
                behaviors.push(behavior);
            }
            Ok(behaviors)
        }

        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            // Single object - wrap in vec
            let behavior = CopyBehavior::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(vec![behavior])
        }
    }

    deserializer.deserialize_any(CopyBehaviorsVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_behaviors_serde() {
        let yaml = r#"
wait: 500
repeat: 3
copy:
  - from: path
    into: "${PATH}"
    using:
      method: regex
      selector: ".*"
"#;
        let behaviors: ResponseBehaviors = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(behaviors.wait, Some(WaitBehavior::Fixed(500))));
        assert_eq!(behaviors.repeat, Some(3));
        assert_eq!(behaviors.copy.len(), 1);
    }

    #[test]
    fn test_shell_transform_config_serde() {
        let yaml = r#"
wait: 100
shellTransform: "echo 'transformed'"
"#;
        let behaviors: ResponseBehaviors = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(behaviors.wait, Some(WaitBehavior::Fixed(100))));
        assert_eq!(
            behaviors.shell_transform,
            Some("echo 'transformed'".to_string())
        );
    }

    #[test]
    fn test_decorate_behavior_serde() {
        let yaml = r#"
wait: 100
decorate: "response.body = 'decorated';"
"#;
        let behaviors: ResponseBehaviors = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(behaviors.wait, Some(WaitBehavior::Fixed(100))));
        assert_eq!(
            behaviors.decorate,
            Some("response.body = 'decorated';".to_string())
        );
    }
}
