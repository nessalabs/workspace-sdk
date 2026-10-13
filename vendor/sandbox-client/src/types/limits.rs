use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitsResponse {
    pub ok: bool,
    #[serde(rename = "type")]
    pub type_: String,
    pub access_tier: Option<String>,
    pub blocked_reason: Option<String>,
    pub current_limits: Option<LimitsFieldsCurrentLimits>,
    pub standard_limits: Option<LimitsFieldsCurrentLimits>,
    pub trial_limits: Option<LimitsFieldsCurrentLimits>,
    pub upgrade_effects: Option<HashMap<String, serde_json::Value>>,
    pub can_start: bool,
    pub checkout_required: Option<bool>,
    pub start_blocked_reason: Option<String>,
    pub contact_message: Option<String>,
    pub active_sandboxes: u64,
    pub active_states: Option<Vec<String>>,
    pub max_active_sandboxes: u64,
    pub max_creation_requests_per_minute: Option<u64>,
    pub max_creation_requests_per_day: Option<u64>,
    pub start_limits: Option<LimitsFieldsStartLimits>,
    pub starts: Option<LimitsFieldsStarts>,
    pub credit_balance_hours: Option<f64>,
    pub pack_balance_hours: Option<f64>,
    pub pack_balance_dollars: Option<f64>,
    pub has_payment_history: Option<bool>,
    pub package: Option<HashMap<String, serde_json::Value>>,
    pub subscription_quota_seconds: Option<f64>,
    pub subscription_remaining_seconds: Option<f64>,
    pub pack_balance_seconds: Option<f64>,
    pub credit_purchased_seconds: Option<f64>,
    pub credit_used_seconds: Option<f64>,
    pub live_usage_seconds: Option<f64>,
    pub credit_seconds_per_dollar: Option<f64>,
    pub billing_status: String,
    pub subscription_status: Option<String>,
    pub subscription_cancel_at_period_end: Option<bool>,
    pub has_subscription: Option<bool>,
    pub subscription_trial_ends_at: Option<String>,
    pub subscription_current_period_end: Option<String>,
    pub credit_balance_seconds: Option<f64>,
    pub team_id: Option<String>,
    pub team_role: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for LimitsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LimitsResponse").finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitsFieldsCurrentLimits {
    pub active_sandboxes: Option<u64>,
    pub creation_rate_per_minute: Option<u64>,
    pub creation_requests_per_day: Option<u64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for LimitsFieldsCurrentLimits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LimitsFieldsCurrentLimits")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitsFieldsStartLimits {
    pub per_minute: Option<u64>,
    pub per_hour: Option<u64>,
    pub per_day: Option<u64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for LimitsFieldsStartLimits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LimitsFieldsStartLimits")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitsFieldsStarts {
    pub unlimited: Option<bool>,
    pub minute: Option<StartWindowUsage>,
    pub hour: Option<StartWindowUsage>,
    pub day: Option<StartWindowUsage>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for LimitsFieldsStarts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LimitsFieldsStarts").finish_non_exhaustive()
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartWindowUsage {
    pub limit: Option<u64>,
    pub used: Option<u64>,
    pub remaining: Option<u64>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
impl fmt::Debug for StartWindowUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StartWindowUsage").finish_non_exhaustive()
    }
}
