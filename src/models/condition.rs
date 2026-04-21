use serde::{Deserialize, Serialize};

/// Stored in role_links.conditions as JSONB.
/// Empty Vec = nobody qualifies.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleConditions {
    #[serde(default)]
    pub approved_batch_ids: Vec<i64>,
}
