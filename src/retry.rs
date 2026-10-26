//! Explicit bounded status retries before accepting a successful response.
use anyhow::{Result, bail};
use reqwest::{
    StatusCode,
    header::{HeaderMap, RETRY_AFTER},
};
use std::time::{Duration, SystemTime};

pub fn delay(
    status: StatusCode,
    headers: &HeaderMap,
    used: u32,
    maximum: u32,
    now: SystemTime,
) -> Result<Option<Duration>> {
    if used >= maximum || !matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504 | 529) {
        return Ok(None);
    }
    let values = headers.get_all(RETRY_AFTER);
    let mut values = values.iter();
    let delay = if let Some(value) = values.next() {
        if values.next().is_some() {
            bail!(
                "HTTP {} has multiple Retry-After values; no retry sent",
                status.as_u16()
            );
        }
        let text = value.to_str().ok().map(str::trim).unwrap_or("");
        if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
            let seconds = text
                .parse::<u64>()
                .ok()
                .filter(|s| *s <= 30)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "HTTP {} Retry-After exceeds the 30s wait limit; no retry sent",
                        status.as_u16()
                    )
                })?;
            Duration::from_secs(seconds)
        } else {
            let date = httpdate::parse_http_date(text).map_err(|_| {
                anyhow::anyhow!(
                    "HTTP {} has invalid Retry-After; no retry sent",
                    status.as_u16()
                )
            })?;
            date.duration_since(now).unwrap_or(Duration::ZERO)
        }
    } else {
        Duration::from_millis(250 * (1u64 << used.min(2)))
    };
    if delay > Duration::from_secs(30) {
        bail!(
            "HTTP {} Retry-After exceeds the 30s wait limit; no retry sent",
            status.as_u16()
        );
    }
    Ok(Some(delay))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_backoff_seconds_and_all_http_date_forms() {
        let now = httpdate::parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT").unwrap();
        let mut h = HeaderMap::new();
        for used in 0..3 {
            assert_eq!(
                delay(StatusCode::SERVICE_UNAVAILABLE, &h, used, 3, now).unwrap(),
                Some(Duration::from_millis(250 << used))
            );
        }
        for (s, seconds) in [
            ("0", 0),
            ("30", 30),
            ("0002", 2),
            ("Sun, 06 Nov 1994 08:49:39 GMT", 2),
            ("Sunday, 06-Nov-94 08:49:39 GMT", 2),
            ("Sun Nov  6 08:49:39 1994", 2),
            ("Sun, 06 Nov 1994 08:49:35 GMT", 0),
        ] {
            h.insert(RETRY_AFTER, s.parse().unwrap());
            assert_eq!(
                delay(StatusCode::TOO_MANY_REQUESTS, &h, 0, 1, now).unwrap(),
                Some(Duration::from_secs(seconds)),
                "{s}"
            );
        }
        for s in [
            "31",
            "9999999999999999999999999999",
            "Sun, 06 Nov 1994 08:50:08 GMT",
            "+1",
            "-1",
            "1.5",
            "secret header",
        ] {
            h.insert(RETRY_AFTER, s.parse().unwrap());
            let error = delay(StatusCode::SERVICE_UNAVAILABLE, &h, 0, 1, now)
                .unwrap_err()
                .to_string();
            assert!(!error.contains(s));
        }
        h.insert(RETRY_AFTER, "0".parse().unwrap());
        h.append(RETRY_AFTER, "1".parse().unwrap());
        assert!(delay(StatusCode::SERVICE_UNAVAILABLE, &h, 0, 1, now).is_err());
    }
    #[test]
    fn disabled_exhausted_and_permanent_errors_do_not_retry_or_parse_headers() {
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, "invalid secret".parse().unwrap());
        for code in [200, 400, 401, 403, 404, 408, 409, 501] {
            assert!(
                delay(
                    StatusCode::from_u16(code).unwrap(),
                    &h,
                    0,
                    3,
                    SystemTime::now()
                )
                .unwrap()
                .is_none()
            );
        }
        for code in [429, 500, 502, 503, 504, 529] {
            for (used, max) in [(0, 0), (1, 1), (3, 3)] {
                assert!(
                    delay(
                        StatusCode::from_u16(code).unwrap(),
                        &h,
                        used,
                        max,
                        SystemTime::now()
                    )
                    .unwrap()
                    .is_none()
                );
            }
        }
    }
}
