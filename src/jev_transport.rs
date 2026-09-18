//! Opt-in hosted transport for TypeSafe Jev.
//!
//! This module is intentionally narrow: it accepts an already-reviewed JSON payload,
//! sends it only to the pinned TypeSafe HTTPS endpoint, and returns the raw JSON response.
//! It never reads repository context and never grants consequence authority.

use serde_json::Value;
use std::time::{Duration, Instant};

pub(crate) const JEV_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const RESPONSE_LIMIT_BYTES: u64 = 2_000_000;
const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_MAX_RETRIES: u32 = 2;
const BACKOFF_INITIAL_MS: u64 = 500;
const BACKOFF_MAX_MS: u64 = 5_000;
const RETRY_AFTER_MAX_MS: u64 = 60_000;
const TOTAL_BUDGET_MAX_MS: u64 = 120_000;

#[derive(Debug, Clone, Copy)]
pub(crate) struct TransportOptions {
    pub timeout_ms: u64,
    pub max_retries: u32,
}

impl Default for TransportOptions {
    fn default() -> Self {
        Self {
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }
}

impl TransportOptions {
    pub(crate) fn validate(self) -> Result<Self, ProviderError> {
        if !(100..=60_000).contains(&self.timeout_ms) {
            return Err(ProviderError::configuration(
                "provider-timeout-invalid",
                "--timeout-ms must be between 100 and 60000",
            ));
        }
        if self.max_retries > 5 {
            return Err(ProviderError::configuration(
                "provider-retries-invalid",
                "--max-retries must be between 0 and 5",
            ));
        }
        let attempts = u64::from(self.max_retries) + 1;
        let mut backoff = 0_u64;
        for retry in 0..self.max_retries {
            backoff = backoff.saturating_add(backoff_ms(retry));
        }
        if self
            .timeout_ms
            .saturating_mul(attempts)
            .saturating_add(backoff)
            > TOTAL_BUDGET_MAX_MS
        {
            return Err(ProviderError::configuration(
                "provider-budget-invalid",
                "timeout/retry settings exceed the 120000ms total transport budget",
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderRun {
    pub response: Value,
    pub attempts: u32,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderError {
    pub code: &'static str,
    pub message: String,
    pub status: Option<u16>,
    pub attempts: u32,
}

impl ProviderError {
    fn configuration(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
            attempts: 0,
        }
    }

    fn runtime(
        code: &'static str,
        message: impl Into<String>,
        status: Option<u16>,
        attempts: u32,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            status,
            attempts,
        }
    }
}

pub(crate) fn api_key_from_env() -> Result<String, ProviderError> {
    let value = std::env::var("TYPESAFE_API_KEY").map_err(|_| {
        ProviderError::configuration(
            "provider-credentials-missing",
            "TYPESAFE_API_KEY is required for hosted Jev evaluation",
        )
    })?;
    validate_api_key(&value)?;
    Ok(value)
}

fn validate_api_key(value: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || value.len() > 8_192
        || value.trim() != value
        || value.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(ProviderError::configuration(
            "provider-credentials-invalid",
            "TYPESAFE_API_KEY is empty, malformed, or contains whitespace/control characters",
        ));
    }
    Ok(())
}

fn retryable_status(status: u16) -> bool {
    status == 408 || status == 429 || (500..=599).contains(&status)
}

fn status_code(status: u16, body: &str) -> &'static str {
    match status {
        401 => "provider-authentication",
        402 => "provider-quota",
        408 => "provider-timeout",
        422 => "provider-request-rejected",
        429 if body.to_ascii_lowercase().contains("quota")
            || body.to_ascii_lowercase().contains("credit")
            || body.to_ascii_lowercase().contains("billing") =>
        {
            "provider-quota"
        }
        429 => "provider-rate-limited",
        529 => "provider-overloaded",
        500..=599 => "provider-server-error",
        _ => "provider-http-error",
    }
}

fn status_message(status: u16) -> &'static str {
    match status {
        401 => "TypeSafe rejected the API credentials",
        402 => "TypeSafe reported unavailable quota or billing capacity",
        408 => "TypeSafe timed out the request",
        422 => "TypeSafe rejected the request payload",
        429 => "TypeSafe rate-limited or quota-limited the request",
        529 => "TypeSafe is temporarily overloaded",
        500..=599 => "TypeSafe returned a server error",
        _ => "TypeSafe returned a non-success HTTP status",
    }
}

fn backoff_ms(retry_index: u32) -> u64 {
    BACKOFF_INITIAL_MS
        .saturating_mul(1_u64 << retry_index.min(10))
        .min(BACKOFF_MAX_MS)
}

fn retry_after_ms(
    retry_after_ms: Option<&str>,
    retry_after: Option<&str>,
    retry_index: u32,
) -> u64 {
    if let Some(value) = retry_after_ms
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value <= RETRY_AFTER_MAX_MS)
    {
        return value;
    }
    if let Some(seconds) = retry_after
        .and_then(|value| value.trim().parse::<u64>().ok())
        .and_then(|seconds| seconds.checked_mul(1_000))
        .filter(|value| *value <= RETRY_AFTER_MAX_MS)
    {
        return seconds;
    }
    backoff_ms(retry_index)
}

fn transport_error(error: &ureq::Error, attempts: u32) -> ProviderError {
    let (code, message) = match error {
        ureq::Error::Timeout(_) => ("provider-timeout", "TypeSafe request timed out"),
        ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Io(_) => (
            "provider-connection",
            "Could not establish or maintain a connection to TypeSafe",
        ),
        ureq::Error::Tls(_)
        | ureq::Error::TlsRequired
        | ureq::Error::Rustls(_)
        | ureq::Error::Pem(_) => (
            "provider-tls",
            "TLS validation or negotiation with TypeSafe failed",
        ),
        _ => ("provider-transport", "TypeSafe transport failed"),
    };
    ProviderError::runtime(code, message, None, attempts)
}

fn bounded_elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

pub(crate) fn execute(
    payload: &Value,
    api_key: &str,
    options: TransportOptions,
) -> Result<ProviderRun, ProviderError> {
    validate_api_key(api_key)?;
    let options = options.validate()?;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(options.timeout_ms)))
        .https_only(true)
        .http_status_as_error(false)
        .max_redirects(0)
        // Do not implicitly route a secret-bearing request through HTTP(S)_PROXY.
        .proxy(None)
        .build();
    let agent = config.new_agent();
    let authorization = format!("Bearer {api_key}");
    let started = Instant::now();

    for attempt_index in 0..=options.max_retries {
        let attempts = attempt_index + 1;
        let call = agent
            .post(JEV_ENDPOINT)
            .header("Authorization", &authorization)
            .header("Accept", "application/json")
            .send_json(payload);

        match call {
            Ok(mut response) => {
                let status = response.status().as_u16();
                let retry_after_ms_header = response
                    .headers()
                    .get("retry-after-ms")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let retry_after_header = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                let body = response
                    .body_mut()
                    .with_config()
                    .limit(RESPONSE_LIMIT_BYTES)
                    .read_to_string()
                    .map_err(|_| {
                        ProviderError::runtime(
                            "provider-response-read",
                            "TypeSafe response could not be read within the configured limit",
                            Some(status),
                            attempts,
                        )
                    })?;

                if (200..=299).contains(&status) {
                    let response: Value = serde_json::from_str(&body).map_err(|_| {
                        ProviderError::runtime(
                            "provider-malformed-response",
                            "TypeSafe returned a non-JSON or malformed JSON success response",
                            Some(status),
                            attempts,
                        )
                    })?;
                    return Ok(ProviderRun {
                        response,
                        attempts,
                        elapsed_ms: bounded_elapsed_ms(started),
                    });
                }

                if retryable_status(status) && attempt_index < options.max_retries {
                    let delay = retry_after_ms(
                        retry_after_ms_header.as_deref(),
                        retry_after_header.as_deref(),
                        attempt_index,
                    );
                    let elapsed = bounded_elapsed_ms(started);
                    if elapsed
                        .saturating_add(delay)
                        .saturating_add(options.timeout_ms)
                        <= TOTAL_BUDGET_MAX_MS
                    {
                        std::thread::sleep(Duration::from_millis(delay));
                        continue;
                    }
                }

                return Err(ProviderError::runtime(
                    status_code(status, &body),
                    status_message(status),
                    Some(status),
                    attempts,
                ));
            }
            Err(error) => {
                if attempt_index < options.max_retries {
                    let delay = backoff_ms(attempt_index);
                    let elapsed = bounded_elapsed_ms(started);
                    if elapsed
                        .saturating_add(delay)
                        .saturating_add(options.timeout_ms)
                        <= TOTAL_BUDGET_MAX_MS
                    {
                        std::thread::sleep(Duration::from_millis(delay));
                        continue;
                    }
                }
                return Err(transport_error(&error, attempts));
            }
        }
    }

    Err(ProviderError::runtime(
        "provider-transport",
        "TypeSafe transport exhausted its bounded attempt loop",
        None,
        options.max_retries + 1,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_defaults_match_documented_typesafe_client_defaults() {
        let options = TransportOptions::default();
        assert_eq!(options.timeout_ms, 10_000);
        assert_eq!(options.max_retries, 2);
        assert!(options.validate().is_ok());
    }

    #[test]
    fn invalid_transport_budgets_fail_before_network() {
        assert!(TransportOptions { timeout_ms: 99, max_retries: 0 }.validate().is_err());
        assert!(TransportOptions { timeout_ms: 60_000, max_retries: 5 }.validate().is_err());
        assert!(TransportOptions { timeout_ms: 10_000, max_retries: 6 }.validate().is_err());
    }

    #[test]
    fn retry_classification_is_bounded_and_deterministic() {
        assert!(retryable_status(408));
        assert!(retryable_status(429));
        assert!(retryable_status(500));
        assert!(retryable_status(529));
        assert!(!retryable_status(401));
        assert!(!retryable_status(422));
        assert_eq!(retry_after_ms(Some("1200"), None, 0), 1200);
        assert_eq!(retry_after_ms(None, Some("2"), 0), 2000);
        assert_eq!(retry_after_ms(Some("999999"), None, 1), 1000);
    }

    #[test]
    fn status_diagnostics_distinguish_auth_quota_rate_limit_and_overload() {
        assert_eq!(status_code(401, ""), "provider-authentication");
        assert_eq!(status_code(402, ""), "provider-quota");
        assert_eq!(status_code(429, "quota exhausted"), "provider-quota");
        assert_eq!(status_code(429, "slow down"), "provider-rate-limited");
        assert_eq!(status_code(529, ""), "provider-overloaded");
    }

    #[test]
    fn malformed_keys_are_rejected_without_echoing_secret_material() {
        for key in ["", " abc", "abc ", "abc def", "abc\ndef"] {
            let error = validate_api_key(key).unwrap_err();
            assert_eq!(error.code, "provider-credentials-invalid");
            assert!(!error.message.contains(key));
        }
    }
}
