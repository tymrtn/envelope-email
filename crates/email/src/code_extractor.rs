// Copyright (c) 2026 Tyler Martin
// Licensed under FSL-1.1-ALv2 (see LICENSE)

//! Verification code extraction from email bodies.
//!
//! Scans plain-text and HTML bodies for common verification/OTP code
//! patterns. Returns the first code found (4-8 digits), preserving
//! leading zeros.

use regex::Regex;

/// Extract a verification code from email text and optional HTML body.
///
/// Checks patterns in priority order:
/// 1. Explicit label (verification/confirmation/security code)
/// 2. OTP-style (one-time password, 2FA)
/// 3. HTML-prominent (bold, table cell)
/// 4. Fallback (isolated 4-8 digit number on its own line)
///
/// Returns the first code found as a String (preserving leading zeros).
pub fn extract_code(text: &str, html: Option<&str>) -> Option<String> {
    // 1. Explicit label pattern
    let explicit = Regex::new(
        r"(?i)(?:verification|confirmation|security|auth(?:entication)?|login)\s*(?:code|number|pin)\s*(?:is|:)?\s*(\d{4,8})"
    ).unwrap();
    if let Some(caps) = explicit.captures(text) {
        return Some(caps[1].to_string());
    }

    // 2. OTP-style pattern
    let otp = Regex::new(
        r"(?i)(?:one.time|OTP|2FA|two.factor)\s*(?:code|password|passcode|pin)\s*(?:is|:)?\s*(\d{4,8})"
    ).unwrap();
    if let Some(caps) = otp.captures(text) {
        return Some(caps[1].to_string());
    }

    // 3. HTML-prominent: check bold or table-cell codes in HTML
    if let Some(html_body) = html {
        let html_patterns = Regex::new(
            r"(?i)<(?:strong|b)>(\d{4,8})</(?:strong|b)>|<td[^>]*>(\d{4,8})</td>"
        ).unwrap();
        if let Some(caps) = html_patterns.captures(html_body) {
            // Return whichever group matched (strong/b or td)
            let code = caps.get(1).or_else(|| caps.get(2)).unwrap();
            return Some(code.as_str().to_string());
        }
    }

    // 4. Fallback: isolated 4-8 digit number on its own line
    let fallback = Regex::new(r"(?m)^\s*(\d{4,8})\s*$").unwrap();
    if let Some(caps) = fallback.captures(text) {
        return Some(caps[1].to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Explicit label patterns ────────────────────────────────────

    #[test]
    fn explicit_verification_code() {
        let text = "Your verification code is 847291";
        assert_eq!(extract_code(text, None), Some("847291".to_string()));
    }

    #[test]
    fn explicit_confirmation_code_with_colon() {
        let text = "Confirmation code: 1234";
        assert_eq!(extract_code(text, None), Some("1234".to_string()));
    }

    #[test]
    fn explicit_security_code() {
        let text = "Your security code is 00482913";
        assert_eq!(extract_code(text, None), Some("00482913".to_string()));
    }

    #[test]
    fn explicit_auth_pin() {
        let text = "Enter your authentication pin: 9012";
        assert_eq!(extract_code(text, None), Some("9012".to_string()));
    }

    #[test]
    fn explicit_login_code() {
        let text = "Your login code is 556677";
        assert_eq!(extract_code(text, None), Some("556677".to_string()));
    }

    // ── OTP-style patterns ─────────────────────────────────────────

    #[test]
    fn otp_code() {
        let text = "Your OTP code is 482910";
        assert_eq!(extract_code(text, None), Some("482910".to_string()));
    }

    #[test]
    fn two_factor_passcode() {
        let text = "Your two-factor passcode: 7731";
        assert_eq!(extract_code(text, None), Some("7731".to_string()));
    }

    #[test]
    fn one_time_password() {
        let text = "Use your one-time password 123456 to log in.";
        assert_eq!(extract_code(text, None), Some("123456".to_string()));
    }

    // ── HTML-prominent patterns ────────────────────────────────────

    #[test]
    fn html_strong_code() {
        let text = "Check your email for the code.";
        let html = Some("<p>Your code is <strong>904821</strong></p>");
        assert_eq!(extract_code(text, html), Some("904821".to_string()));
    }

    #[test]
    fn html_bold_code() {
        let text = "We sent you a code.";
        let html = Some("<p>Code: <b>5544</b></p>");
        assert_eq!(extract_code(text, html), Some("5544".to_string()));
    }

    #[test]
    fn html_td_code() {
        let text = "Verify your account.";
        let html = Some(r#"<table><tr><td class="code">77889900</td></tr></table>"#);
        assert_eq!(extract_code(text, html), Some("77889900".to_string()));
    }

    // ── Fallback: isolated digit line ──────────────────────────────

    #[test]
    fn fallback_isolated_line() {
        let text = "Hello,\n\nPlease use the following:\n\n  829104\n\nThanks!";
        assert_eq!(extract_code(text, None), Some("829104".to_string()));
    }

    #[test]
    fn fallback_preserves_leading_zeros() {
        let text = "Your code:\n0042\n";
        assert_eq!(extract_code(text, None), Some("0042".to_string()));
    }

    // ── No match ───────────────────────────────────────────────────

    #[test]
    fn no_code_in_text() {
        let text = "Hello, this is a normal email with no codes.";
        assert_eq!(extract_code(text, None), None);
    }

    #[test]
    fn short_number_not_matched() {
        // 3 digits is below the 4-digit minimum
        let text = "Your code is 123";
        assert_eq!(extract_code(text, None), None);
    }

    #[test]
    fn long_number_not_matched() {
        // 9 digits exceeds the 8-digit maximum
        let text = "Your code is 123456789";
        assert_eq!(extract_code(text, None), None);
    }

    // ── Priority ordering ──────────────────────────────────────────

    #[test]
    fn explicit_label_takes_priority_over_fallback() {
        let text = "Your verification code is 111111\n\n222222\n";
        assert_eq!(extract_code(text, None), Some("111111".to_string()));
    }

    #[test]
    fn otp_takes_priority_over_html() {
        let text = "Your OTP code is 333333";
        let html = Some("<strong>444444</strong>");
        assert_eq!(extract_code(text, html), Some("333333".to_string()));
    }
}
