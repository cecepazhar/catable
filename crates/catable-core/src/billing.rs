//! Billing module for CATable — handles entitlement states, license key activation,
//! OTP authentication and checkout integration with GCC.

use crate::error::CatermError;
use crate::pro;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementState {
    pub tier: String, // "free" | "pro" | "pro_grace" | "free_needs_online_check"
    pub billing_type: String, // "free" | "monthly" | "yearly" | "lifetime"
    pub is_pro: bool,
    pub expires_at: Option<String>,
    pub grace_until: Option<String>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingPlan {
    pub plan_id: String,
    pub billing_type: String, // "monthly" | "yearly" | "lifetime"
    pub price_minor: u64,
    pub currency: String, // "IDR" | "USD"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutResult {
    pub checkout_id: String,
    pub checkout_url: String,
    pub completion_mode: String, // "poll" | "license_key"
}

pub fn get_effective_state() -> Result<EntitlementState, CatermError> {
    let st = pro::status()?;
    if st.is_pro {
        let (expires_at, billing_type) = match &st.entitlement {
            pro::Entitlement::Valid { expires_at, tier, .. } => {
                let dt = chrono::DateTime::from_timestamp(*expires_at, 0)
                    .map(|d| d.to_rfc3339());
                let b_type = if *expires_at > 2000000000 {
                    "lifetime".to_string()
                } else if tier == "team" {
                    "team".to_string()
                } else {
                    "yearly".to_string()
                };
                (dt, b_type)
            }
            _ => (None, "lifetime".to_string()),
        };

        Ok(EntitlementState {
            tier: "pro".to_string(),
            billing_type,
            is_pro: true,
            expires_at,
            grace_until: None,
            warning: None,
        })
    } else {
        Ok(EntitlementState {
            tier: "free".to_string(),
            billing_type: "free".to_string(),
            is_pro: false,
            expires_at: None,
            grace_until: None,
            warning: None,
        })
    }
}

pub fn request_otp(email: &str) -> Result<(), CatermError> {
    pro::resend_verification(email)
}

pub fn verify_otp(email: &str, otp_code: &str) -> Result<(), CatermError> {
    let clean_code = otp_code.trim();
    if clean_code.is_empty() {
        return Err(CatermError::Billing("OTP code cannot be empty".into()));
    }
    // Forward OTP/password login to GCC
    let _ = pro::login(email, clean_code)?;
    Ok(())
}

pub fn activate_license(license_key: &str) -> Result<pro::ProStatus, CatermError> {
    pro::activate_license_key(license_key)
}

pub fn create_checkout(plan_id: &str) -> Result<CheckoutResult, CatermError> {
    let checkout_id = format!("chk_{}_{}", plan_id, chrono::Utc::now().timestamp());
    let checkout_url = format!("https://gcc.fathforce.com/products/catable?plan={plan_id}");
    Ok(CheckoutResult {
        checkout_id,
        checkout_url,
        completion_mode: "poll".to_string(),
    })
}

pub fn poll_checkout(_checkout_id: &str) -> Result<String, CatermError> {
    let st = pro::status()?;
    if st.is_pro {
        Ok("paid".to_string())
    } else {
        // Attempt a background sync with GCC
        let _ = pro::sync();
        let fresh_st = pro::status()?;
        if fresh_st.is_pro {
            Ok("paid".to_string())
        } else {
            Ok("pending".to_string())
        }
    }
}

pub fn sign_out() -> Result<(), CatermError> {
    pro::logout()
}
