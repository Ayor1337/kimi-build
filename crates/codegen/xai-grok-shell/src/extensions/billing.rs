//! `x.ai/billing` extension handler.
//!
//! Fetches the authenticated user's Kimi Build quota (weekly limit, per-window
//! limits such as the 5h cap, and the booster wallet) from
//! `GET {proxy}/usages` and maps it onto the billing shape the pager renders.
//! Used by the pager/desktop to display credits and usage.

use agent_client_protocol as acp;
use serde::{Deserialize, Serialize};

use super::{ExtResult, to_raw_response};
use crate::agent::MvpAgent;

/// Billing period cycle identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingCycle {
    pub year: i32,
    pub month: i32,
}

/// Cent value from the billing API (USD cents).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cent {
    /// proto3 JSON omits zero-valued scalars, so a `$0` Cent arrives as `{}`;
    /// default to 0 rather than failing the whole parse.
    #[serde(default)]
    pub val: i64,
}

/// A usage period (weekly or monthly) from the newer credits config.
///
/// `start`/`end` are RFC 3339 timestamps. `period_type` is the proto enum name
/// (e.g. `USAGE_PERIOD_TYPE_WEEKLY`); kept so callers can distinguish weekly
/// vs monthly cycles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePeriod {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub period_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
}

/// Usage summary for one past billing period.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingPeriodUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_cycle: Option<BillingCycle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub included_used: Option<Cent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_demand_used: Option<Cent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_used: Option<Cent>,
}

/// One extra quota window from the Kimi `/usages` payload (e.g. the 5h limit),
/// rendered as its own line in the `/usage` summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaRow {
    pub label: String,
    pub used: i64,
    pub limit: i64,
    /// Ready-to-display reset hint (e.g. "resets in 2d 3h"), if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_hint: Option<String>,
}

/// Current billing configuration for Grok Build coding credits.
///
/// Carries both the newer credits-config fields (`credit_usage_percent`,
/// `current_period`) and the deprecated `GrokBuildBillingConfig` fields
/// (`monthly_limit`, `used`, `billing_period_*`). Consumers should prefer the
/// new fields and fall back to the deprecated ones, so the same struct works
/// against both the new `GetGrokCreditsConfig` and the legacy
/// `GetGrokBuildBillingConfig` backend responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingConfig {
    /// Included credit usage as a percentage of the allowance (0.0–100.0).
    /// Preferred over deriving from `monthly_limit`/`used`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credit_usage_percent: Option<f64>,
    /// Current usage period (weekly or monthly). Preferred over
    /// `billing_period_start`/`billing_period_end`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period: Option<UsagePeriod>,
    /// Deprecated: included monthly credit budget. Use `credit_usage_percent`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_limit: Option<Cent>,
    /// Deprecated: credits used this period. Use `credit_usage_percent`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<Cent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_demand_cap: Option<Cent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_demand_used: Option<Cent>,
    /// Remaining prepaid (purchased) credit balance, positive — the "bought
    /// credits" the user has topped up. Populated from the credits config
    /// (`GetGrokCreditsConfig.prepaid_balance`); absent in the legacy billing
    /// shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepaid_balance: Option<Cent>,
    /// Whether this user is on unified usage billing (shared weekly/monthly
    /// pool). From `GrokCreditsConfig.is_unified_billing_user`, which billing
    /// sets from remote settings `unified_consumer_billing_enabled`. `None` when
    /// absent (legacy `GetGrokBuildBillingConfig` shape or older servers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_unified_billing_user: Option<bool>,
    /// Deprecated: use `current_period.start`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_period_start: Option<String>,
    /// Deprecated: use `current_period.end`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_period_end: Option<String>,
    /// Extra quota windows from the Kimi `/usages` payload (e.g. the 5h
    /// limit). Empty for providers without per-window quotas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quota_rows: Vec<QuotaRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<BillingPeriodUsage>,
}

/// Top-level response (primarily from `GET /rest/grok/credits` + auto-topup-rule).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingConfigResponse {
    pub config: Option<BillingConfig>,
    /// Whether on-demand credit usage is enabled. When `false`, the pager
    /// should hide on-demand controls. Populated from `RemoteSettings`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_demand_enabled: Option<bool>,
    /// User-friendly subscription tier name (e.g. "SuperGrok Heavy").
    /// Populated from `RemoteSettings` so the pager can update its cached
    /// tier on every billing fetch without an extra request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_tier: Option<String>,
}

/// Auto top-up configuration (from GetAutoTopupRule).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTopupRule {
    /// proto3 JSON omits `false`, so a disabled rule arrives without this field;
    /// default to `false` rather than failing the parse (which would otherwise
    /// keep a stale cached rule in the pager).
    #[serde(default)]
    pub enabled: bool,
    pub min_before_hitting_sl: Option<Cent>,
    pub topup_amount: Option<Cent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_amount_per_month: Option<Cent>,
}

/// Wrapper for the auto top-up rule response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetAutoTopupRuleResponse {
    #[serde(default)]
    pub rule: Option<AutoTopupRule>,
}

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "x.ai/billing" => {
            tracing::info!("handling billing config request");
            handle_get_billing(agent).await
        }
        "x.ai/auto-topup-rule" => {
            tracing::info!("handling auto top-up rule request");
            handle_get_auto_topup_rule(agent).await
        }
        _ => Err(acp::Error::method_not_found()),
    }
}

/// Structured context for unified-log entries from a successful billing fetch.
///
/// Keeps history to a count + the most recent period so `~/.grok/logs/unified.jsonl`
/// stays useful without dumping unbounded period arrays.
fn billing_unified_log_ctx(billing: &BillingConfigResponse) -> serde_json::Value {
    let history_len = billing
        .config
        .as_ref()
        .map(|c| c.history.len())
        .unwrap_or(0);
    let latest_history = billing
        .config
        .as_ref()
        .and_then(|c| c.history.last())
        .and_then(|p| serde_json::to_value(p).ok());

    let mut config_value = billing
        .config
        .as_ref()
        .and_then(|c| serde_json::to_value(c).ok())
        .unwrap_or(serde_json::Value::Null);
    if let Some(obj) = config_value.as_object_mut() {
        // Drop full history array; surface length + latest entry instead.
        obj.remove("history");
        obj.insert("historyLen".into(), serde_json::json!(history_len));
        if let Some(latest) = latest_history {
            obj.insert("latestHistory".into(), latest);
        }
    }

    serde_json::json!({
        "config": config_value,
        "onDemandEnabled": billing.on_demand_enabled,
        "subscriptionTier": billing.subscription_tier,
    })
}

async fn handle_get_billing(agent: &MvpAgent) -> ExtResult {
    let auth = super::auth_gate::require_xai_auth(
        &agent.auth_manager,
        "Authentication required to fetch billing data",
        "Billing data requires auth with Kimi. Run `grok login` to authenticate.",
    )?;

    let proxy_base = agent.cli_chat_proxy_base_url();
    let base = proxy_base.trim_end_matches('/');

    // Kimi Build quota: `GET /usages` (weekly limit, per-window limits,
    // booster wallet). Mirrors the official kimi CLI's managed-usage fetch:
    // only `Authorization` + `Accept` headers.
    let usages_url = format!("{}/usages", base);
    let resp = crate::http::shared_client()
        .get(&usages_url)
        .header("Authorization", format!("Bearer {}", &auth.key))
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "billing: upstream request failed");
            xai_grok_telemetry::unified_log::warn(
                "billing: upstream request failed",
                None,
                Some(serde_json::json!({ "error": e.to_string() })),
            );
            acp::Error::internal_error().data(format!("Failed to fetch billing data: {e}"))
        })?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(status, url = %usages_url, "billing: upstream error");

        let detail = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
            .unwrap_or_else(|| format!("HTTP {status}"));

        xai_grok_telemetry::unified_log::warn(
            "billing: upstream error",
            None,
            Some(serde_json::json!({
                "status": status,
                "detail": detail,
            })),
        );

        return Err(acp::Error::internal_error().data(format!("Billing service error: {detail}")));
    }

    let payload: serde_json::Value = resp.json().await.map_err(|e| {
        tracing::error!(error = %e, "billing: failed to parse response");
        xai_grok_telemetry::unified_log::warn(
            "billing: failed to parse response",
            None,
            Some(serde_json::json!({ "error": e.to_string() })),
        );
        acp::Error::internal_error().data(format!("Failed to parse billing data: {e}"))
    })?;

    let billing = billing_from_kimi_usages(&payload);

    // Every prompt / /usage / poll path hits `x.ai/billing`; log the fetched
    // quota snapshot so support can correlate limit UX with real balances.
    xai_grok_telemetry::unified_log::info(
        "billing: fetched credits config",
        None,
        Some(billing_unified_log_ctx(&billing)),
    );

    to_raw_response(&billing)
}

// ── Kimi `/usages` payload mapping ──────────────────────────────────────

/// Kimi booster-wallet amounts are fixed-point with 6 fractional digits.
const FIXED_POINT_CENTS: f64 = 1e6;

/// Truncating number-or-numeric-string coercion (mirrors the official CLI's
/// `toInt`).
fn to_i64(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::Number(n) => n.as_f64().filter(|f| f.is_finite()).map(|f| f as i64),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok().map(|f| f as i64),
        _ => None,
    }
}

fn obj_get<'v>(v: &'v serde_json::Value, key: &str) -> Option<&'v serde_json::Value> {
    v.as_object().and_then(|o| o.get(key))
}

fn first_present<'v>(
    v: &'v serde_json::Value,
    keys: &[&str],
) -> Option<&'v serde_json::Value> {
    keys.iter().find_map(|k| obj_get(v, k))
}

/// Format seconds as the official CLI does: `2d 3h 5m` (seconds only when no
/// larger unit is present).
fn format_duration(total_seconds: i64) -> String {
    if total_seconds <= 0 {
        return "0s".to_string();
    }
    let days = total_seconds / 86400;
    let hours = total_seconds % 86400 / 3600;
    let minutes = total_seconds % 3600 / 60;
    let secs = total_seconds % 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if parts.is_empty() && secs > 0 {
        parts.push(format!("{secs}s"));
    }
    parts.join(" ")
}

/// Reset hint + machine-usable reset timestamp for one usage row.
///
/// Accepts `reset_at`-style RFC 3339 strings and `reset_in`-style second
/// counts. Returns `(display_hint, reset_at_rfc3339)`.
fn reset_hint_from(raw: &serde_json::Value) -> (Option<String>, Option<String>) {
    const AT_KEYS: &[&str] = &["reset_at", "resetAt", "reset_time", "resetTime"];
    if let Some(v) = first_present(raw, AT_KEYS).and_then(|v| v.as_str())
        && !v.is_empty()
    {
        let hint = match chrono::DateTime::parse_from_rfc3339(v) {
            Ok(dt) => {
                let secs = (dt.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds();
                if secs <= 0 {
                    "reset".to_string()
                } else {
                    format!("resets in {}", format_duration(secs))
                }
            }
            Err(_) => format!("resets at {v}"),
        };
        return (Some(hint), Some(v.to_string()));
    }
    const IN_KEYS: &[&str] = &["reset_in", "resetIn", "ttl", "window"];
    if let Some(secs) = first_present(raw, IN_KEYS).and_then(to_i64)
        && secs > 0
    {
        let end = (chrono::Utc::now() + chrono::Duration::seconds(secs)).to_rfc3339();
        return (
            Some(format!("resets in {}", format_duration(secs))),
            Some(end),
        );
    }
    (None, None)
}

/// One usage row (`usage` summary or a `limits[].detail`): label, used,
/// limit, and reset info. `None` when both used and limit are absent.
struct ParsedUsageRow {
    label: String,
    used: i64,
    limit: i64,
    reset_hint: Option<String>,
    reset_at: Option<String>,
}

fn parse_usage_row(raw: &serde_json::Value, default_label: &str) -> Option<ParsedUsageRow> {
    if !raw.is_object() {
        return None;
    }
    let limit = obj_get(raw, "limit").and_then(to_i64);
    let mut used = obj_get(raw, "used").and_then(to_i64);
    if used.is_none()
        && let (Some(remaining), Some(limit)) =
            (obj_get(raw, "remaining").and_then(to_i64), limit)
    {
        used = Some(limit - remaining);
    }
    if used.is_none() && limit.is_none() {
        return None;
    }
    let label = first_present(raw, &["name", "title"])
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(default_label)
        .to_string();
    let (reset_hint, reset_at) = reset_hint_from(raw);
    Some(ParsedUsageRow {
        label,
        used: used.unwrap_or(0),
        limit: limit.unwrap_or(0),
        reset_hint,
        reset_at,
    })
}

/// Label for a `limits[]` entry: explicit name/title/scope, else derived from
/// the window (`5h limit`, `30m limit`, `7d limit`).
fn limit_label(
    item: &serde_json::Value,
    detail: &serde_json::Value,
    window: &serde_json::Value,
    idx: usize,
) -> String {
    for key in ["name", "title", "scope"] {
        if let Some(v) = obj_get(item, key)
            .or_else(|| obj_get(detail, key))
            .and_then(|v| v.as_str())
            && !v.is_empty()
        {
            return v.to_string();
        }
    }
    let duration = obj_get(window, "duration")
        .or_else(|| obj_get(item, "duration"))
        .or_else(|| obj_get(detail, "duration"))
        .and_then(to_i64);
    let time_unit = obj_get(window, "timeUnit")
        .or_else(|| obj_get(item, "timeUnit"))
        .or_else(|| obj_get(detail, "timeUnit"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if let Some(duration) = duration {
        if time_unit.contains("MINUTE") {
            if duration >= 60 && duration % 60 == 0 {
                return format!("{}h limit", duration / 60);
            }
            return format!("{duration}m limit");
        }
        if time_unit.contains("HOUR") {
            return format!("{duration}h limit");
        }
        if time_unit.contains("DAY") {
            return format!("{duration}d limit");
        }
        return format!("{duration}s limit");
    }
    format!("Limit #{}", idx + 1)
}

/// Fixed-point (1e6) → cents, rounding up sub-cent remainders to 1.
fn fixed_point_to_cents(value: i64) -> i64 {
    let cents = value as f64 / FIXED_POINT_CENTS;
    if cents > 0.0 && cents < 1.0 {
        1
    } else {
        cents.round() as i64
    }
}

/// `monthlyChargeLimit` / `monthlyUsed`: `{priceInCents, currency}` → cents.
fn parse_money_cents(raw: Option<&serde_json::Value>) -> Option<i64> {
    obj_get(raw?, "priceInCents").and_then(to_i64)
}

/// Booster wallet → `(prepaid_balance_cents, charge_limit_cents, charge_used_cents,
/// charge_limit_enabled)`.
fn parse_booster_wallet(raw: &serde_json::Value) -> Option<(i64, i64, i64, bool)> {
    let balance = obj_get(raw, "balance")?;
    if obj_get(balance, "type").and_then(|v| v.as_str()) != Some("BOOSTER") {
        return None;
    }
    let amount = obj_get(balance, "amount").and_then(to_i64)?;
    if amount <= 0 {
        return None;
    }
    let amount_left = obj_get(balance, "amountLeft")
        .and_then(to_i64)
        .map(fixed_point_to_cents)
        .unwrap_or(0);
    let limit = parse_money_cents(obj_get(raw, "monthlyChargeLimit")).unwrap_or(0);
    let used = parse_money_cents(obj_get(raw, "monthlyUsed")).unwrap_or(0);
    let enabled = obj_get(raw, "monthlyChargeLimitEnabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some((amount_left, limit, used, enabled))
}

/// Map the Kimi `/usages` payload onto the billing shape the pager renders.
///
/// - summary `usage` → weekly usage percent + next reset
/// - `limits[]` → extra quota rows (5h limit etc.)
/// - `boosterWallet` → prepaid credits + pay-as-you-go (monthly charge limit)
fn billing_from_kimi_usages(payload: &serde_json::Value) -> BillingConfigResponse {
    let summary = obj_get(payload, "usage").and_then(|u| parse_usage_row(u, "Weekly limit"));

    let mut quota_rows = Vec::new();
    if let Some(serde_json::Value::Array(items)) = obj_get(payload, "limits") {
        for (idx, item) in items.iter().enumerate() {
            let detail = obj_get(item, "detail").unwrap_or(item);
            let window = obj_get(item, "window").cloned().unwrap_or(serde_json::json!({}));
            let label = limit_label(item, detail, &window, idx);
            if let Some(row) = parse_usage_row(detail, &label) {
                quota_rows.push(QuotaRow {
                    label: row.label,
                    used: row.used,
                    limit: row.limit,
                    reset_hint: row.reset_hint,
                });
            }
        }
    }

    let (prepaid, charge_limit, charge_used, charge_enabled) = obj_get(payload, "boosterWallet")
        .and_then(parse_booster_wallet)
        .unwrap_or((0, 0, 0, false));

    let (usage_pct, current_period, monthly_limit, used) = match &summary {
        Some(row) => {
            let pct = if row.limit > 0 {
                Some(row.used as f64 / row.limit as f64 * 100.0)
            } else {
                None
            };
            let period_type = if row.label.to_lowercase().contains("month") {
                "USAGE_PERIOD_TYPE_MONTHLY"
            } else {
                "USAGE_PERIOD_TYPE_WEEKLY"
            };
            let period = UsagePeriod {
                period_type: Some(period_type.to_string()),
                start: None,
                end: row.reset_at.clone(),
            };
            (
                pct,
                Some(period),
                (row.limit > 0).then_some(Cent { val: row.limit }),
                Some(Cent { val: row.used }),
            )
        }
        None => (None, None, None, None),
    };

    BillingConfigResponse {
        config: Some(BillingConfig {
            credit_usage_percent: usage_pct,
            current_period,
            monthly_limit,
            used,
            on_demand_cap: (charge_enabled && charge_limit > 0).then_some(Cent {
                val: charge_limit,
            }),
            on_demand_used: (charge_enabled && charge_limit > 0)
                .then_some(Cent { val: charge_used }),
            prepaid_balance: (prepaid > 0).then_some(Cent { val: prepaid }),
            is_unified_billing_user: None,
            billing_period_start: None,
            billing_period_end: None,
            quota_rows,
            history: vec![],
        }),
        on_demand_enabled: Some(charge_enabled),
        subscription_tier: None,
    }
}

async fn handle_get_auto_topup_rule(agent: &MvpAgent) -> ExtResult {
    let _ = super::auth_gate::require_xai_auth(
        &agent.auth_manager,
        "Authentication required to fetch auto top-up rule",
        "Auto top-up data requires auth with Kimi. Run `grok login` to authenticate.",
    )?;

    // Kimi has no auto top-up concept; report a definitive "no rule" instead
    // of hitting a non-existent endpoint on every poll.
    to_raw_response(&GetAutoTopupRuleResponse { rule: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_topup_disabled_rule_omits_enabled_field() {
        // proto3 JSON omits `false` / `0`, so a disabled rule arrives without
        // `enabled` (and zero Cents as `{}`). It must still deserialize (as
        // disabled) rather than erroring — otherwise the pager keeps a stale
        // cached rule.
        let json = serde_json::json!({
            "rule": { "topupAmount": {"val": 500}, "minBeforeHittingSl": {} }
        });
        let resp: GetAutoTopupRuleResponse = serde_json::from_value(json).unwrap();
        let rule = resp.rule.expect("rule present");
        assert!(!rule.enabled);
        assert_eq!(rule.topup_amount.unwrap().val, 500);
        assert_eq!(rule.min_before_hitting_sl.unwrap().val, 0);
    }

    #[test]
    fn billing_config_response_deserializes_from_backend_json() {
        let json = serde_json::json!({
            "config": {
                "monthlyLimit": {"val": 2000},
                "used": {"val": 1234},
                "onDemandCap": {"val": 500},
                "billingPeriodStart": "2025-04-01T00:00:00Z",
                "billingPeriodEnd": "2025-05-01T00:00:00Z",
                "history": [
                    {
                        "billingCycle": {"year": 2025, "month": 3},
                        "includedUsed": {"val": 1800},
                        "onDemandUsed": {"val": 0},
                        "totalUsed": {"val": 1800}
                    }
                ]
            }
        });
        let resp: BillingConfigResponse = serde_json::from_value(json).unwrap();
        let config = resp.config.unwrap();
        assert_eq!(config.monthly_limit.unwrap().val, 2000);
        assert_eq!(config.used.unwrap().val, 1234);
        assert_eq!(config.on_demand_cap.unwrap().val, 500);
        assert_eq!(
            config.billing_period_start.as_deref(),
            Some("2025-04-01T00:00:00Z")
        );
        assert_eq!(config.history.len(), 1);
        let period = &config.history[0];
        let cycle = period.billing_cycle.as_ref().unwrap();
        assert_eq!(cycle.year, 2025);
        assert_eq!(cycle.month, 3);
        assert_eq!(period.included_used.as_ref().unwrap().val, 1800);
        assert_eq!(period.total_used.as_ref().unwrap().val, 1800);
    }

    #[test]
    fn billing_unified_log_ctx_includes_credits_and_collapses_history() {
        let resp = BillingConfigResponse {
            config: Some(BillingConfig {
                credit_usage_percent: Some(42.5),
                current_period: Some(UsagePeriod {
                    period_type: Some("USAGE_PERIOD_TYPE_WEEKLY".into()),
                    start: Some("2025-04-01T00:00:00Z".into()),
                    end: Some("2025-04-08T00:00:00Z".into()),
                }),
                monthly_limit: Some(Cent { val: 2000 }),
                used: Some(Cent { val: 850 }),
                on_demand_cap: Some(Cent { val: 500 }),
                on_demand_used: Some(Cent { val: 0 }),
                prepaid_balance: Some(Cent { val: 100 }),
                is_unified_billing_user: Some(true),
                billing_period_start: None,
                billing_period_end: None,
                quota_rows: vec![],
                history: vec![
                    BillingPeriodUsage {
                        billing_cycle: Some(BillingCycle {
                            year: 2025,
                            month: 2,
                        }),
                        included_used: Some(Cent { val: 1000 }),
                        on_demand_used: Some(Cent { val: 0 }),
                        total_used: Some(Cent { val: 1000 }),
                    },
                    BillingPeriodUsage {
                        billing_cycle: Some(BillingCycle {
                            year: 2025,
                            month: 3,
                        }),
                        included_used: Some(Cent { val: 1800 }),
                        on_demand_used: Some(Cent { val: 0 }),
                        total_used: Some(Cent { val: 1800 }),
                    },
                ],
            }),
            on_demand_enabled: Some(true),
            subscription_tier: Some("SuperGrok".into()),
        };
        let ctx = billing_unified_log_ctx(&resp);
        assert_eq!(ctx["onDemandEnabled"], true);
        assert_eq!(ctx["subscriptionTier"], "SuperGrok");
        let config = ctx["config"].as_object().expect("config object");
        assert!(
            config.get("history").is_none(),
            "full history must be collapsed"
        );
        assert_eq!(config["historyLen"], 2);
        assert_eq!(
            config["latestHistory"]["billingCycle"]["month"], 3,
            "latest history period retained"
        );
        assert_eq!(config["creditUsagePercent"], 42.5);
        assert_eq!(config["prepaidBalance"]["val"], 100);
    }

    #[test]
    fn billing_config_response_roundtrips_through_json() {
        let config = BillingConfig {
            credit_usage_percent: None,
            current_period: None,
            monthly_limit: Some(Cent { val: 5000 }),
            used: Some(Cent { val: 123 }),
            on_demand_cap: Some(Cent { val: 0 }),
            on_demand_used: Some(Cent { val: 50 }),
            prepaid_balance: Some(Cent { val: 750 }),
            is_unified_billing_user: None,
            billing_period_start: Some("2025-04-01T00:00:00Z".to_string()),
            billing_period_end: Some("2025-05-01T00:00:00Z".to_string()),
            quota_rows: vec![],
            history: vec![BillingPeriodUsage {
                billing_cycle: Some(BillingCycle {
                    year: 2025,
                    month: 3,
                }),
                included_used: Some(Cent { val: 4500 }),
                on_demand_used: Some(Cent { val: 100 }),
                total_used: Some(Cent { val: 4600 }),
            }],
        };
        let resp = BillingConfigResponse {
            config: Some(config),
            on_demand_enabled: None,
            subscription_tier: None,
        };
        let json = serde_json::to_value(&resp).unwrap();
        let roundtripped: BillingConfigResponse = serde_json::from_value(json).unwrap();
        let rt_config = roundtripped.config.unwrap();
        assert_eq!(rt_config.monthly_limit.unwrap().val, 5000);
        assert_eq!(rt_config.used.unwrap().val, 123);
        assert_eq!(rt_config.prepaid_balance.unwrap().val, 750);
        assert_eq!(rt_config.history.len(), 1);
    }

    #[test]
    fn billing_config_response_handles_null_config() {
        let json = serde_json::json!({"config": null});
        let resp: BillingConfigResponse = serde_json::from_value(json).unwrap();
        assert!(resp.config.is_none());
    }

    #[test]
    fn billing_config_response_handles_empty_history() {
        let json = serde_json::json!({
            "config": {
                "monthlyLimit": {"val": 1000},
                "used": {"val": 0}
            }
        });
        let resp: BillingConfigResponse = serde_json::from_value(json).unwrap();
        let config = resp.config.unwrap();
        assert_eq!(config.monthly_limit.unwrap().val, 1000);
        assert!(config.history.is_empty());
    }

    #[test]
    fn billing_config_serializes_camel_case() {
        let config = BillingConfig {
            credit_usage_percent: None,
            current_period: None,
            monthly_limit: Some(Cent { val: 100 }),
            used: None,
            on_demand_cap: None,
            on_demand_used: None,
            prepaid_balance: None,
            is_unified_billing_user: None,
            billing_period_start: None,
            billing_period_end: None,
            quota_rows: vec![],
            history: vec![],
        };
        let json = serde_json::to_value(&config).unwrap();
        assert!(json.get("monthlyLimit").is_some());
        // Fields with None are skipped
        assert!(json.get("creditUsagePercent").is_none());
        assert!(json.get("currentPeriod").is_none());
        assert!(json.get("used").is_none());
        assert!(json.get("onDemandCap").is_none());
        assert!(json.get("onDemandUsed").is_none());
        assert!(json.get("prepaidBalance").is_none());
        assert!(json.get("billingPeriodStart").is_none());
        // Empty history is skipped
        assert!(json.get("history").is_none());
    }

    #[test]
    fn billing_config_deserializes_credits_config_shape() {
        // Newer `GetGrokCreditsConfig` response: percentage-based usage,
        // a typed current period, and history keyed by `period`.
        let json = serde_json::json!({
            "config": {
                "creditUsagePercent": 42.5,
                "currentPeriod": {
                    "type": "USAGE_PERIOD_TYPE_WEEKLY",
                    "start": "2026-06-01T00:00:00Z",
                    "end": "2026-06-08T00:00:00Z"
                },
                "onDemandCap": {"val": 5000},
                "onDemandUsed": {"val": 300},
                "prepaidBalance": {"val": 1250},
                "isUnifiedBillingUser": true,
                "productUsage": [
                    {"product": "PRODUCT_GROK_BUILD", "usagePercent": 61.2}
                ],
                "history": [
                    {
                        "period": {
                            "type": "USAGE_PERIOD_TYPE_WEEKLY",
                            "start": "2026-05-25T00:00:00Z",
                            "end": "2026-06-01T00:00:00Z"
                        },
                        "onDemandUsed": {"val": 120}
                    }
                ]
            }
        });
        let resp: BillingConfigResponse = serde_json::from_value(json).unwrap();
        let config = resp.config.unwrap();
        assert_eq!(config.credit_usage_percent, Some(42.5));
        let period = config.current_period.as_ref().unwrap();
        assert_eq!(
            period.period_type.as_deref(),
            Some("USAGE_PERIOD_TYPE_WEEKLY")
        );
        assert_eq!(period.end.as_deref(), Some("2026-06-08T00:00:00Z"));
        // Deprecated fields are absent in the credits shape.
        assert!(config.monthly_limit.is_none());
        assert!(config.billing_period_end.is_none());
        assert_eq!(config.on_demand_cap.unwrap().val, 5000);
        assert_eq!(config.on_demand_used.unwrap().val, 300);
        // Bought (prepaid) credit balance is parsed from the credits config.
        assert_eq!(config.prepaid_balance.unwrap().val, 1250);
        assert_eq!(config.is_unified_billing_user, Some(true));
        // productUsage is still unused by the CLI billing surface.
        assert_eq!(config.history.len(), 1);
        assert_eq!(config.history[0].on_demand_used.as_ref().unwrap().val, 120);
    }

    #[test]
    fn cent_serializes_as_val_field() {
        let c = Cent { val: 4299 };
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json, serde_json::json!({"val": 4299}));
    }

    // ── Kimi `/usages` mapping ────────────────────────────────────────

    #[test]
    fn kimi_usages_full_payload_maps_all_sections() {
        let reset_at = (chrono::Utc::now() + chrono::Duration::days(2)).to_rfc3339();
        let payload = serde_json::json!({
            "usage": { "limit": 1000, "used": 250, "reset_at": reset_at },
            "limits": [
                {
                    "window": { "duration": 5, "timeUnit": "USAGE_TIME_UNIT_HOUR" },
                    "detail": { "limit": 100, "remaining": 60, "reset_in": 3600 }
                },
                {
                    "scope": "Weekly bonus",
                    "detail": { "limit": "500", "used": "50" }
                }
            ],
            "boosterWallet": {
                "balance": { "type": "BOOSTER", "amount": 20_000_000_000i64, "amountLeft": 12_340_000_000i64 },
                "monthlyChargeLimit": { "priceInCents": 5000, "currency": "USD" },
                "monthlyUsed": { "priceInCents": 1234, "currency": "USD" },
                "monthlyChargeLimitEnabled": true
            }
        });
        let resp = billing_from_kimi_usages(&payload);
        let config = resp.config.unwrap();

        // Weekly summary.
        assert_eq!(config.credit_usage_percent, Some(25.0));
        let period = config.current_period.unwrap();
        assert_eq!(period.period_type.as_deref(), Some("USAGE_PERIOD_TYPE_WEEKLY"));
        assert_eq!(period.end.as_deref(), Some(reset_at.as_str()));
        assert_eq!(config.monthly_limit.unwrap().val, 1000);
        assert_eq!(config.used.unwrap().val, 250);

        // Per-window rows: window-derived label + remaining→used fallback.
        assert_eq!(config.quota_rows.len(), 2);
        let five_h = &config.quota_rows[0];
        assert_eq!(five_h.label, "5h limit");
        assert_eq!(five_h.used, 40);
        assert_eq!(five_h.limit, 100);
        assert_eq!(five_h.reset_hint.as_deref(), Some("resets in 1h"));
        // scope label + string-coerced numbers.
        let bonus = &config.quota_rows[1];
        assert_eq!(bonus.label, "Weekly bonus");
        assert_eq!(bonus.used, 50);
        assert_eq!(bonus.limit, 500);

        // Booster wallet → prepaid credits + pay-as-you-go charge limit.
        assert_eq!(config.prepaid_balance.unwrap().val, 12340);
        assert_eq!(config.on_demand_cap.unwrap().val, 5000);
        assert_eq!(config.on_demand_used.unwrap().val, 1234);
        assert_eq!(resp.on_demand_enabled, Some(true));
    }

    #[test]
    fn kimi_usages_minimal_payload_tolerates_missing_sections() {
        let resp = billing_from_kimi_usages(&serde_json::json!({}));
        let config = resp.config.unwrap();
        assert!(config.credit_usage_percent.is_none());
        assert!(config.current_period.is_none());
        assert!(config.quota_rows.is_empty());
        assert!(config.prepaid_balance.is_none());
        assert!(config.on_demand_cap.is_none());
    }

    #[test]
    fn kimi_usages_zero_balance_wallet_yields_no_prepaid() {
        let payload = serde_json::json!({
            "boosterWallet": {
                "balance": { "type": "BOOSTER", "amount": 0, "amountLeft": 0 }
            }
        });
        let resp = billing_from_kimi_usages(&payload);
        assert!(resp.config.unwrap().prepaid_balance.is_none());
    }

    #[test]
    fn kimi_limit_label_minute_windows() {
        let detail = serde_json::json!({});
        // 300 minutes → "5h limit"; 45 minutes → "45m limit".
        let item = serde_json::json!({ "window": { "duration": 300, "timeUnit": "MINUTE" } });
        assert_eq!(limit_label(&item, &detail, &item["window"], 0), "5h limit");
        let item = serde_json::json!({ "window": { "duration": 45, "timeUnit": "MINUTE" } });
        assert_eq!(limit_label(&item, &detail, &item["window"], 0), "45m limit");
        // Day windows and the numbered fallback.
        let item = serde_json::json!({ "window": { "duration": 7, "timeUnit": "DAY" } });
        assert_eq!(limit_label(&item, &detail, &item["window"], 0), "7d limit");
        assert_eq!(limit_label(&detail, &detail, &detail, 1), "Limit #2");
    }

    #[test]
    fn kimi_reset_hint_past_time_reports_reset() {
        let past = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        let raw = serde_json::json!({ "reset_at": past });
        let (hint, at) = reset_hint_from(&raw);
        assert_eq!(hint.as_deref(), Some("reset"));
        assert_eq!(at.as_deref(), Some(past.as_str()));
    }
}
