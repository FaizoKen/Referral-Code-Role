use std::collections::HashMap;

use serde_json::Value;

use crate::error::AppError;
use crate::models::condition::RoleConditions;

pub struct BatchSummary {
    pub id: i64,
    pub name: String,
}

pub fn build_config_schema(
    guild_id: &str,
    conditions: &RoleConditions,
    batches: &[BatchSummary],
    base_url: &str,
) -> Value {
    let admin_url = format!("{base_url}/admin?guild_id={guild_id}");
    let redeem_url = format!("{base_url}/verify");

    serde_json::json!({
        "version": 1,
        "name": "Referral Code Role",
        "description": "Hand out redemption codes (Kickstarter rewards, podcast shout-outs, event wristbands, QR flyers). Members who redeem a valid code get this role automatically.",
        "sections": [
            {
                "title": "Quick start (3 steps)",
                "fields": [
                    {
                        "type": "display",
                        "key": "step_1",
                        "label": "Step 1 — Create a code group",
                        "value": format!("Open the admin panel and click \"+ New code group\". A code group is a bundle of codes that share the same rules (e.g. \"Kickstarter Tier 2\", \"Podcast Episode 47\"). Admin panel: {admin_url}")
                    },
                    {
                        "type": "display",
                        "key": "step_2",
                        "label": "Step 2 — Generate codes",
                        "value": "Inside the group, click \"Generate codes\" to create one or many. You can also paste your own (e.g. PODCODE for a podcast). Each code shows a Copy URL button and a QR download."
                    },
                    {
                        "type": "display",
                        "key": "step_3",
                        "label": "Step 3 — Approve the group below & share",
                        "value": format!("Tick the group in the picker below. Then share your redemption page with users: {redeem_url} . When they sign in and enter a code from an approved group, they get this role.")
                    }
                ]
            },
            batches_section(guild_id, batches, base_url),
            {
                "title": "Links",
                "fields": [
                    {
                        "type": "display",
                        "key": "admin_link",
                        "label": "Admin panel (for you)",
                        "value": format!("Manage code groups, generate codes, view stats: {admin_url}")
                    },
                    {
                        "type": "display",
                        "key": "redeem_link",
                        "label": "Redemption page (for members)",
                        "value": format!("Send this link to anyone who has a code: {redeem_url}")
                    }
                ]
            }
        ],
        "values": {
            "approved_batch_ids": conditions.approved_batch_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>()
        }
    })
}

fn batches_section(guild_id: &str, batches: &[BatchSummary], base_url: &str) -> Value {
    if batches.is_empty() {
        let admin_url = format!("{base_url}/admin?guild_id={guild_id}");
        return serde_json::json!({
            "title": "Approved code groups",
            "description": "No code groups yet. Create one in the admin panel — it'll appear here as a checkbox. Tick it to make codes from that group grant this role.",
            "fields": [
                {
                    "type": "display",
                    "key": "no_batches",
                    "label": "Get started",
                    "value": format!("Open the admin panel and click \"+ New code group\": {admin_url}")
                }
            ]
        });
    }

    serde_json::json!({
        "title": "Approved code groups",
        "description": "Tick the groups whose codes should grant this role. Leave them all unticked and nobody gets the role.",
        "fields": [
            {
                "type": "multi_select",
                "key": "approved_batch_ids",
                "label": "Groups that grant this role",
                "description": "Need a new group? Use the admin panel link below.",
                "options": batches.iter().map(|b| serde_json::json!({
                    "label": b.name,
                    "value": b.id.to_string()
                })).collect::<Vec<_>>()
            }
        ]
    })
}

pub fn parse_config(config: &HashMap<String, Value>) -> Result<RoleConditions, AppError> {
    let ids = config
        .get("approved_batch_ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .and_then(|s| s.parse::<i64>().ok())
                        .or_else(|| v.as_i64())
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(RoleConditions {
        approved_batch_ids: ids,
    })
}
