use std::collections::HashSet;

use crate::models::condition::RoleConditions;

/// True if any redeemed batch is in the approved set.
pub fn qualifies(conditions: &RoleConditions, user_redeemed_batches: &HashSet<i64>) -> bool {
    if conditions.approved_batch_ids.is_empty() {
        return false;
    }
    conditions
        .approved_batch_ids
        .iter()
        .any(|id| user_redeemed_batches.contains(id))
}
