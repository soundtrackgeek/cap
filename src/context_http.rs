//! Retry transient context GET failures within the shared context deadline.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use capsule_core::context::{Cancellation, HttpClient, HttpRequest, HttpResponse};
use reqwest::blocking::Client;

const FIRST_ATTEMPT_LIMIT: Duration = Duration::from_secs(3);
// Also respects Nominatim's limit of at most one request per second.
const RETRY_INTERVAL: Duration = Duration::from_secs(1);

pub struct ContextHttpClient {
    client: Client,
    cancellation: Arc<dyn Cancellation>,
}

impl ContextHttpClient {
    pub fn new(cancellation: Arc<dyn Cancellation>) -> Result<Self, String> {
        Ok(Self {
            client: Client::builder().build().map_err(describe_error)?,
            cancellation,
        })
    }

    fn attempt(&self, request: &HttpRequest, timeout: Duration) -> Result<Attempt, reqwest::Error> {
        let mut builder = self.client.get(&request.url).timeout(timeout);
        for (key, value) in &request.headers {
            builder = builder.header(key, value);
        }
        let response = builder.send()?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .map(|value| {
                // An invalid server delay must not cause an immediate retry.
                value.to_str().ok().and_then(retry_after_delay)
            });
        Ok(Attempt {
            response: HttpResponse {
                status,
                // The core rejects non-success statuses without reading JSON.
                // Do not let an incomplete error page discard Retry-After.
                body: if (200..300).contains(&status) {
                    response.bytes()?.to_vec()
                } else {
                    Vec::new()
                },
            },
            retry_after,
        })
    }
}

struct Attempt {
    response: HttpResponse,
    retry_after: Option<Option<Duration>>,
}

fn retry_after_delay(value: &str) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let date = chrono::DateTime::parse_from_rfc2822(value).ok()?;
    Some(
        date.signed_duration_since(chrono::Utc::now())
            .to_std()
            .unwrap_or_default(),
    )
}

fn retry_delay(result: &Result<Attempt, reqwest::Error>, elapsed: Duration) -> Option<Duration> {
    let server_delay = match result {
        Ok(attempt) if matches!(attempt.response.status, 429 | 502 | 503 | 504) => {
            match attempt.retry_after {
                Some(delay) => delay?,
                None => Duration::ZERO,
            }
        }
        Err(error)
            if error.is_timeout()
                || error.is_connect()
                || error.is_request()
                || error.is_body() =>
        {
            Duration::ZERO
        }
        _ => return None,
    };
    Some(server_delay.max(RETRY_INTERVAL.saturating_sub(elapsed)))
}

fn describe_error(error: reqwest::Error) -> String {
    // Provider URLs can contain credentials. Preserve the cause, not the URL.
    let error = error.without_url();
    let mut message = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

impl HttpClient for ContextHttpClient {
    fn get(&self, request: HttpRequest, timeout: Duration) -> Result<HttpResponse, String> {
        let started = Instant::now();
        if self.cancellation.is_cancelled() {
            return Err("context capture cancelled".to_string());
        }
        if timeout.is_zero() {
            return Err("context capture deadline exceeded".to_string());
        }
        let first_timeout = if timeout >= RETRY_INTERVAL * 2 {
            FIRST_ATTEMPT_LIMIT.min(timeout / 2)
        } else {
            timeout
        };
        let mut result = self.attempt(&request, first_timeout);
        let mut attempts = 1;
        if let Some(delay) = retry_delay(&result, started.elapsed()) {
            let remaining = timeout.saturating_sub(started.elapsed());
            // Leave a useful request budget; never exceed a server's Retry-After.
            if remaining.saturating_sub(delay) >= Duration::from_millis(100) {
                let waiting = Instant::now();
                while waiting.elapsed() < delay {
                    if self.cancellation.is_cancelled() {
                        return Err("context capture cancelled".to_string());
                    }
                    std::thread::sleep(
                        delay
                            .saturating_sub(waiting.elapsed())
                            .min(Duration::from_millis(50)),
                    );
                }
                let remaining = timeout.saturating_sub(started.elapsed());
                if !remaining.is_zero() && !self.cancellation.is_cancelled() {
                    attempts += 1;
                    result = self.attempt(&request, remaining);
                }
            }
        }
        result.map(|attempt| attempt.response).map_err(|error| {
            format!(
                "{} (after {attempts} attempt(s), {:.1}s)",
                describe_error(error),
                started.elapsed().as_secs_f64()
            )
        })
    }
}
