use std::collections::HashMap;

use serde_json::Value;

use crate::error::AppError;
use crate::models::condition::RoleConditions;

pub struct BatchSummary {
    pub id: i64,
    pub name: String,
    /// Retired in the admin panel. A retired group is only listed while a
    /// role still has it ticked (see `routes::plugin::get_config`).
    pub retired: bool,
}

/// The dashboard refuses a schema whose option labels pass 100 characters
/// or whose multi-select offers more than 100 options (the RoleLogic API's
/// plugin-schema validation), and then the settings never open.
const MAX_OPTIONS: usize = 100;
const MAX_LABEL: usize = 100;

fn option_label(b: &BatchSummary) -> String {
    let suffix = if b.retired { " (retired)" } else { "" };
    let room = MAX_LABEL - suffix.chars().count();
    let name: String = if b.name.chars().count() > room {
        b.name.chars().take(room - 1).collect::<String>() + "…"
    } else {
        b.name.clone()
    };
    format!("{name}{suffix}")
}

pub fn build_config_schema(
    guild_id: &str,
    conditions: &RoleConditions,
    batches: &[BatchSummary],
    base_url: &str,
) -> Value {
    let admin_url = format!("{base_url}/admin?guild_id={guild_id}");
    let redeem_url = format!("{base_url}/verify?guild={guild_id}");
    let offered: Vec<&BatchSummary> = batches.iter().take(MAX_OPTIONS).collect();

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
                        "value": format!("Open the [admin panel]({admin_url}) and click **+ New code group**. A code group is a bundle of codes that share the same rules, like \"Kickstarter Backers\" or \"Podcast Episode 47\".")
                    },
                    {
                        "type": "display",
                        "key": "step_2",
                        "label": "Step 2 — Generate codes",
                        "value": "Right after you create a group, choose how many codes to make, or paste your own (like PODCODE for a podcast). Every code gets a link to copy and a QR code to download."
                    },
                    {
                        "type": "display",
                        "key": "step_3",
                        "label": "Step 3 — Tick the group below, save, and share",
                        "value": format!("Tick the group under **Approved code groups** and click Save. Then share your redemption page: {redeem_url}\nAnyone who signs in and enters a code from a ticked group gets this role.")
                    }
                ]
            },
            batches_section(guild_id, &offered, base_url),
            {
                "title": "Links",
                "fields": [
                    {
                        "type": "display",
                        "key": "admin_link",
                        "label": "Admin panel (for you)",
                        "value": format!("Create code groups, generate codes and see who redeemed: [Open the admin panel]({admin_url})")
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
            "approved_batch_ids": conditions
                .approved_batch_ids
                .iter()
                // Only ids the picker offers: the dashboard refuses to save a
                // value that isn't one of its options.
                .filter(|id| offered.iter().any(|b| b.id == **id))
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
        }
    })
}

fn batches_section(guild_id: &str, batches: &[&BatchSummary], base_url: &str) -> Value {
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
                    "value": format!("Open the [admin panel]({admin_url}) and click **+ New code group**, then come back here and tick it.")
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
                "description": "A retired group stays listed while it's ticked, so the people who redeemed its codes keep the role. Untick it to take the role away from them.",
                "options": batches.iter().map(|b| serde_json::json!({
                    "label": option_label(b),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(id: i64, name: &str, retired: bool) -> BatchSummary {
        BatchSummary {
            id,
            name: name.into(),
            retired,
        }
    }

    fn options(schema: &Value) -> Vec<(String, String)> {
        schema["sections"][1]["fields"][0]["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                (
                    o["label"].as_str().unwrap().to_string(),
                    o["value"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn a_ticked_retired_group_stays_a_valid_option() {
        let conditions = RoleConditions {
            approved_batch_ids: vec![7, 9],
        };
        let batches = [
            batch(9, "Podcast Ep. 47", false),
            batch(7, "Kickstarter Backers", true),
        ];
        let s = build_config_schema("1", &conditions, &batches, "https://x/referral-code-role");
        assert_eq!(
            options(&s),
            [
                ("Podcast Ep. 47".to_string(), "9".to_string()),
                ("Kickstarter Backers (retired)".to_string(), "7".to_string()),
            ]
        );
        // Every saved value is one of the options, or the dashboard's save fails.
        assert_eq!(
            s["values"]["approved_batch_ids"],
            serde_json::json!(["7", "9"])
        );
    }

    #[test]
    fn labels_fit_the_dashboards_limit() {
        let long = "A".repeat(140);
        let label = option_label(&batch(1, &long, true));
        assert_eq!(label.chars().count(), MAX_LABEL);
        assert!(label.ends_with("… (retired)"));
        assert_eq!(option_label(&batch(1, "Short", false)), "Short");
    }

    #[test]
    fn links_are_markdown_the_dashboard_draws() {
        let s = build_config_schema(
            "42",
            &RoleConditions::default(),
            &[],
            "https://x/referral-code-role",
        );
        let step1 = s["sections"][0]["fields"][0]["value"].as_str().unwrap();
        assert!(
            step1.contains("[admin panel](https://x/referral-code-role/admin?guild_id=42)"),
            "{step1}"
        );
        let step3 = s["sections"][0]["fields"][2]["value"].as_str().unwrap();
        assert!(
            step3.contains("https://x/referral-code-role/verify?guild=42\n"),
            "{step3}"
        );
    }
}
