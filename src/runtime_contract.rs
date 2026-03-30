use crate::config::ConfigSchemaVersion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiSchemaVersion {
    V1,
}

impl ApiSchemaVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeContract {
    pub config_schema: ConfigSchemaVersion,
    pub api_schema: ApiSchemaVersion,
    pub policy_provider: &'static str,
}

impl RuntimeContract {
    pub fn new(config_schema: ConfigSchemaVersion, policy_provider: &'static str) -> Self {
        Self {
            config_schema,
            api_schema: ApiSchemaVersion::V1,
            policy_provider,
        }
    }
}
