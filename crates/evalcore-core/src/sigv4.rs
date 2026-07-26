//! AWS Signature Version 4 signing, for `auth: sigv4` targets.
//!
//! Deliberately hand-rolled over `sha2` + `hmac` rather than pulling
//! `aws-config`: EvalCore needs *signing*, not credential *resolution*.
//! `aws-config`'s chain (env → profile → IMDS → assumed role) would put a
//! network round-trip and a clock read behind every target build, which is in
//! tension with the determinism rule and with `api_key_env`'s "secrets are
//! named environment variables" model. Credentials here are named env vars like
//! every other secret in EvalCore.
//!
//! [`SigV4Credentials::sign_post`] reads the clock exactly once, at the call
//! site; the signing itself is the pure [`sign_request`], which takes the
//! timestamp as a parameter and is therefore testable against the AWS
//! specification's own fixtures.

use anyhow::{bail, Context};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// AWS credentials plus the region and service a signature is scoped to.
/// Resolved from named environment variables by the target factory; the values
/// never appear in YAML, in a cache identity, or in an error message.
pub struct SigV4Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Present for temporary credentials (assumed roles, SSO). Sent as
    /// `x-amz-security-token` and signed with the rest.
    pub session_token: Option<String>,
    pub region: String,
    pub service: String,
}

impl SigV4Credentials {
    /// Sign a POST and return the headers to attach. Reads the clock — the one
    /// place this module does — and delegates to the pure [`sign_request`].
    pub(crate) fn sign_post(
        &self,
        url: &str,
        extra_headers: &[(&'static str, &'static str)],
        payload: &[u8],
    ) -> anyhow::Result<Vec<(String, String)>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("system clock is before the Unix epoch; cannot sign an AWS request")?
            .as_secs();
        sign_request(self, url, extra_headers, payload, now)
    }
}

/// The signed headers for one request, as a pure function of its inputs and the
/// timestamp. Implements the SigV4 "Authorization header" flow: canonical
/// request → string to sign → derived signing key → signature.
pub(crate) fn sign_request(
    credentials: &SigV4Credentials,
    url: &str,
    extra_headers: &[(&'static str, &'static str)],
    payload: &[u8],
    unix_seconds: u64,
) -> anyhow::Result<Vec<(String, String)>> {
    let (host, canonical_uri, canonical_query) = split_url(url)?;
    let timestamp = Timestamp::from_unix(unix_seconds);
    let amz_date = timestamp.amz_date();
    let date_stamp = timestamp.date_stamp();

    // Headers participating in the signature, lowercased and sorted. Every
    // header we actually send must be here, or AWS rejects the signature.
    let payload_hash = hex(&Sha256::digest(payload));
    let mut headers: Vec<(String, String)> = vec![
        ("content-type".into(), "application/json".into()),
        ("host".into(), host.clone()),
        ("x-amz-content-sha256".into(), payload_hash.clone()),
        ("x-amz-date".into(), amz_date.clone()),
    ];
    for (name, value) in extra_headers {
        headers.push((name.to_ascii_lowercase(), (*value).to_string()));
    }
    if let Some(token) = &credentials.session_token {
        headers.push(("x-amz-security-token".into(), token.clone()));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let signed_headers = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_headers = headers
        .iter()
        .map(|(name, value)| format!("{name}:{}\n", value.trim()))
        .collect::<String>();

    let canonical_request = format!(
        "POST\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let scope = format!(
        "{date_stamp}/{}/{}/aws4_request",
        credentials.region, credentials.service
    );
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes()))
    );

    // The signing key is derived by chained HMACs down the credential scope.
    let mut key = hmac(
        format!("AWS4{}", credentials.secret_access_key).as_bytes(),
        date_stamp.as_bytes(),
    );
    for part in [
        credentials.region.as_bytes(),
        credentials.service.as_bytes(),
        b"aws4_request",
    ] {
        key = hmac(&key, part);
    }
    let signature = hex(&hmac(&key, string_to_sign.as_bytes()));

    let mut signed = vec![
        (
            "authorization".to_string(),
            format!(
                "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, \
                 Signature={signature}",
                credentials.access_key_id
            ),
        ),
        ("x-amz-date".to_string(), amz_date),
        ("x-amz-content-sha256".to_string(), payload_hash),
    ];
    if let Some(token) = &credentials.session_token {
        signed.push(("x-amz-security-token".to_string(), token.clone()));
    }
    Ok(signed)
}

/// Split a URL into (host, canonical URI path, canonical query string).
/// SigV4 canonicalization requires the path percent-encoded and the query
/// sorted by name; EvalCore only ever signs paths it builds itself, so the
/// path passes through and the query is sorted as-is.
fn split_url(url: &str) -> anyhow::Result<(String, String, String)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .with_context(|| format!("cannot sign {url}: not an http(s) URL"))?;
    let (authority, path_and_query) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        bail!("cannot sign {url}: no host");
    }
    let (path, query) = match path_and_query.split_once('?') {
        Some((path, query)) => (path, query),
        None => (path_and_query, ""),
    };
    let mut pairs: Vec<&str> = query.split('&').filter(|p| !p.is_empty()).collect();
    pairs.sort_unstable();
    Ok((
        authority.to_string(),
        if path.is_empty() { "/" } else { path }.to_string(),
        pairs.join("&"),
    ))
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    // `new_from_slice` only fails for key sizes HMAC-SHA256 accepts any of.
    let mut mac =
        <HmacSha256 as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        // Writing to a String never fails.
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// UTC calendar time, derived from a Unix timestamp without pulling a date
/// library in. SigV4 needs exactly two renderings: `YYYYMMDDTHHMMSSZ` and
/// `YYYYMMDD`.
struct Timestamp {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

impl Timestamp {
    /// Howard Hinnant's `civil_from_days`, the standard branch-free conversion
    /// from a day count to a proleptic Gregorian date.
    fn from_unix(seconds: u64) -> Self {
        let days = (seconds / 86_400) as i64;
        let time_of_day = seconds % 86_400;

        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted_month = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
        let month = if shifted_month < 10 {
            shifted_month + 3
        } else {
            shifted_month - 9
        } as u32;

        Self {
            year: year + i64::from(month <= 2),
            month,
            day,
            hour: (time_of_day / 3600) as u32,
            minute: (time_of_day % 3600 / 60) as u32,
            second: (time_of_day % 60) as u32,
        }
    }

    fn date_stamp(&self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }

    fn amz_date(&self) -> String {
        format!(
            "{}T{:02}{:02}{:02}Z",
            self.date_stamp(),
            self.hour,
            self.minute,
            self.second
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials() -> SigV4Credentials {
        // The example credentials from the AWS SigV4 test suite.
        SigV4Credentials {
            access_key_id: "AKIDEXAMPLE".into(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            session_token: None,
            region: "us-east-1".into(),
            service: "bedrock".into(),
        }
    }

    /// 2015-08-30T12:36:00Z — the timestamp the AWS documentation's worked
    /// examples use, so the rendering is checkable against the spec.
    const AWS_EXAMPLE_UNIX: u64 = 1_440_938_160;

    #[test]
    fn renders_the_two_timestamp_formats_aws_requires() {
        let stamp = Timestamp::from_unix(AWS_EXAMPLE_UNIX);
        assert_eq!(stamp.date_stamp(), "20150830");
        assert_eq!(stamp.amz_date(), "20150830T123600Z");
        // Epoch and a leap day, to pin the civil-date conversion.
        assert_eq!(Timestamp::from_unix(0).amz_date(), "19700101T000000Z");
        assert_eq!(
            Timestamp::from_unix(1_709_164_800).date_stamp(),
            "20240229",
            "2024 is a leap year"
        );
    }

    #[test]
    fn signature_is_deterministic_and_scoped() {
        let signed = sign_request(
            &credentials(),
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/m/invoke",
            &[],
            b"{}",
            AWS_EXAMPLE_UNIX,
        )
        .unwrap();
        let header = |name: &str| {
            signed
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| panic!("{name} must be signed"))
        };
        let authorization = header("authorization");
        assert!(
            authorization
                .contains("Credential=AKIDEXAMPLE/20150830/us-east-1/bedrock/aws4_request"),
            "got: {authorization}"
        );
        assert!(
            authorization
                .contains("SignedHeaders=content-type;host;x-amz-content-sha256;x-amz-date"),
            "signed headers are lowercased and sorted; got: {authorization}"
        );
        assert_eq!(header("x-amz-date"), "20150830T123600Z");
        // SHA-256 of "{}" — pins that the payload, not a placeholder, is hashed.
        assert_eq!(
            header("x-amz-content-sha256"),
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );

        // Same inputs, same signature: signing is pure given the timestamp.
        let again = sign_request(
            &credentials(),
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/m/invoke",
            &[],
            b"{}",
            AWS_EXAMPLE_UNIX,
        )
        .unwrap();
        assert_eq!(signed, again);
    }

    #[test]
    fn payload_and_timestamp_both_change_the_signature() {
        let url = "https://bedrock-runtime.us-east-1.amazonaws.com/model/m/invoke";
        let base = sign_request(&credentials(), url, &[], b"{}", AWS_EXAMPLE_UNIX).unwrap();
        let other_payload =
            sign_request(&credentials(), url, &[], b"{\"a\":1}", AWS_EXAMPLE_UNIX).unwrap();
        let other_time =
            sign_request(&credentials(), url, &[], b"{}", AWS_EXAMPLE_UNIX + 60).unwrap();
        assert_ne!(base, other_payload, "the body is signed");
        assert_ne!(base, other_time, "the timestamp is signed");
    }

    #[test]
    fn session_token_is_sent_and_signed() {
        let mut credentials = credentials();
        credentials.session_token = Some("session-token-value".into());
        let signed = sign_request(
            &credentials,
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/m/invoke",
            &[],
            b"{}",
            AWS_EXAMPLE_UNIX,
        )
        .unwrap();
        assert!(signed
            .iter()
            .any(|(n, v)| n == "x-amz-security-token" && v == "session-token-value"));
        let authorization = &signed.iter().find(|(n, _)| n == "authorization").unwrap().1;
        assert!(
            authorization.contains("x-amz-security-token"),
            "a temporary credential must sign its token; got: {authorization}"
        );
    }

    #[test]
    fn canonicalizes_host_path_and_query() {
        let (host, path, query) =
            split_url("https://bedrock-runtime.us-east-1.amazonaws.com/model/anthropic.x/invoke")
                .unwrap();
        assert_eq!(host, "bedrock-runtime.us-east-1.amazonaws.com");
        assert_eq!(path, "/model/anthropic.x/invoke");
        assert_eq!(query, "");
        // A bare host still canonicalizes to "/", and query pairs sort.
        assert_eq!(split_url("https://x.aws").unwrap().1, "/");
        assert_eq!(split_url("https://x.aws/p?b=2&a=1").unwrap().2, "a=1&b=2");
        assert!(split_url("ftp://x.aws/p").is_err());
    }
}
