//! Official SDK for [emaki](https://emaki.ai), the Japanese-language AI video
//! generator for Seedance 2.5 and Seedance 2.0.
//!
//! One API key runs the emaki models on your own credits: Seedance 2.5,
//! Seedance 2.0 (fast and mini), MiniMax H3, Veo 3.1 for video, and
//! Nano Banana 2, Seedream 5 and GPT Image 2 for stills. Create a key at
//! <https://emaki.ai/settings/apikeys> and set `EMAKI_API_KEY`.
//!
//! ```no_run
//! let clip = emaki::generate_video("a paper lantern drifting over a night river", &emaki::Params::new())?;
//! let img = emaki::generate_image("a ramen shop at dusk, isometric", &emaki::Params::new())?;
//! # Ok::<(), emaki::Error>(())
//! ```

use serde_json::{json, Map, Value};
use std::fmt;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// The service this crate talks to.
pub const BASE_URL: &str = "https://emaki.ai";
/// Where an API key is created.
pub const KEYS_URL: &str = "https://emaki.ai/settings/apikeys";
/// Default video model: Seedance 2.5.
pub const MODEL_VIDEO: &str = "seedance_2_5";
/// Default image model: the cheapest one.
pub const MODEL_IMAGE: &str = "nano_banana_2_lite";

/// What went wrong, split by what to do next.
#[derive(Debug)]
pub enum Error {
    /// No API key, or the key is invalid or deleted (HTTP 401).
    Auth(String),
    /// The account is out of credits (HTTP 402).
    Quota(String),
    /// The model unlocks after a credit purchase (HTTP 403).
    Plan(String),
    /// A generation on this account is already in progress (HTTP 409).
    Busy(String),
    /// The content filter refused the prompt or image. Reword it.
    Rejected(String),
    /// A parameter the API did not accept (HTTP 400).
    Validation(String),
    /// The run ended as failed; carries the failure code.
    Failed(String),
    /// The deadline passed; the run may still finish in your emaki library.
    Timeout,
    /// Transport failure or an unexpected answer.
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Auth(m) if m.contains("settings/apikeys") => write!(f, "emaki: {m}"),
            Error::Auth(m) => write!(f, "emaki: {m} (create a key at {KEYS_URL})"),
            Error::Quota(m) | Error::Plan(m) | Error::Busy(m) | Error::Validation(m) | Error::Other(m) => write!(f, "emaki: {m}"),
            Error::Rejected(m) => write!(f, "emaki: {m} (reword the prompt rather than retrying)"),
            Error::Failed(code) => write!(f, "emaki: generation failed ({code})"),
            Error::Timeout => write!(f, "emaki: deadline passed; the run may still finish in your library"),
        }
    }
}

impl std::error::Error for Error {}

/// Model parameters, sent as `parameters`. Leave a key out to use the site default.
#[derive(Clone, Debug, Default)]
pub struct Params(pub Map<String, Value>);

impl Params {
    pub fn new() -> Self {
        Self::default()
    }
    /// Set one parameter, e.g. `.set("duration", 8).set("resolution", "720p")`.
    pub fn set(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.0.insert(key.into(), value.into());
        self
    }
}

/// The state of one run, from `/api/video/status`.
#[derive(Clone, Debug)]
pub struct Status {
    pub id: String,
    /// "generating", "completed" or "failed".
    pub status: String,
    /// The finished file (video or image) once `status` is "completed".
    pub url: Option<String>,
    pub failure_code: Option<String>,
    /// The whole `data` object as the API sent it.
    pub raw: Value,
}

/// A client bound to one API key.
#[derive(Clone, Debug)]
pub struct Client {
    key: String,
    base: String,
    /// Time between status checks. Default 5 seconds.
    pub poll_every: Duration,
    /// Total wait for one run. Default 15 minutes; video can queue.
    pub timeout: Duration,
}

impl Client {
    pub fn new(api_key: impl Into<String>) -> Self {
        Client { key: api_key.into(), base: BASE_URL.into(), poll_every: Duration::from_secs(5), timeout: Duration::from_secs(900) }
    }

    /// Read the key from `EMAKI_API_KEY`.
    pub fn from_env() -> Result<Self, Error> {
        match std::env::var("EMAKI_API_KEY") {
            Ok(k) if !k.trim().is_empty() => Ok(Client::new(k.trim())),
            _ => Err(Error::Auth("an API key is required: set EMAKI_API_KEY".into())),
        }
    }

    /// Point the client at another host (tests, proxies).
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into().trim_end_matches('/').into();
        self
    }

    /// Start a run and return its id without waiting.
    pub fn submit(&self, prompt: &str, model: &str, params: &Params) -> Result<String, Error> {
        if prompt.trim().is_empty() {
            return Err(Error::Validation("prompt is required".into()));
        }
        let (code, body) = self.post("/api/video/ai", json!({ "prompt": prompt, "model": model, "parameters": Value::Object(params.0.clone()) }))?;
        parse_submit(code, &body)
    }

    /// Look up a run.
    pub fn status(&self, id: &str) -> Result<Status, Error> {
        let (code, body) = self.post("/api/video/status", json!({ "videoId": id }))?;
        parse_status(code, &body)
    }

    /// Credits left on the account. Free to call.
    pub fn credits(&self) -> Result<i64, Error> {
        let (code, body) = self.post("/api/user/get-user-credits", json!({}))?;
        parse_credits(code, &body)
    }

    /// Submit, wait, and return the URL of the finished file.
    pub fn generate(&self, prompt: &str, model: &str, params: &Params) -> Result<String, Error> {
        let id = self.submit(prompt, model, params)?;
        self.wait(&id)
    }

    /// Wait for a submitted run and return its URL.
    pub fn wait(&self, id: &str) -> Result<String, Error> {
        let deadline = Instant::now() + self.timeout;
        while Instant::now() < deadline {
            sleep(self.poll_every);
            let st = match self.status(id) {
                Ok(s) => s,
                Err(e @ Error::Auth(_)) => return Err(e),
                Err(_) => continue, // a dropped poll is not a failed run
            };
            if let Some(url) = finished(&st)? {
                return Ok(url);
            }
        }
        Err(Error::Timeout)
    }

    fn post(&self, path: &str, body: Value) -> Result<(u16, Value), Error> {
        let resp = ureq::post(&format!("{}{path}", self.base))
            .set("Authorization", &format!("Bearer {}", self.key))
            .set("User-Agent", concat!("emaki-rust/", env!("CARGO_PKG_VERSION")))
            .send_json(body);
        match resp {
            Ok(r) => {
                let code = r.status();
                Ok((code, r.into_json().unwrap_or(Value::Null)))
            }
            Err(ureq::Error::Status(code, r)) => Ok((code, r.into_json().unwrap_or(Value::Null))),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }
}

/// `Some(url)` when the run is done, an error when it failed, `None` while it runs.
pub fn finished(st: &Status) -> Result<Option<String>, Error> {
    match st.status.as_str() {
        "completed" | "succeeded" => match &st.url {
            Some(u) => Ok(Some(u.clone())),
            None => Ok(None),
        },
        "failed" => Err(Error::Failed(st.failure_code.clone().unwrap_or_else(|| "unknown".into()))),
        _ => Ok(None),
    }
}

/// Map an HTTP status and error body to the error that says what to do next.
pub fn classify(code: u16, body: &Value) -> Error {
    let msg = body.get("error").or_else(|| body.get("message")).and_then(Value::as_str).unwrap_or("request failed").to_string();
    let tag = body.get("code").and_then(Value::as_str).unwrap_or("");
    match code {
        401 => Error::Auth(msg),
        402 => Error::Quota(msg),
        403 => Error::Plan(msg),
        409 => Error::Busy(msg),
        _ if tag == "content_cooldown" || tag == "content_blocked" => Error::Rejected(msg),
        400 => Error::Validation(msg),
        // The credits endpoint answers HTTP 200 with code -1.
        200 if msg.to_lowercase().contains("api key") || msg.to_lowercase().contains("unauthorized") => Error::Auth(msg),
        _ => Error::Other(format!("HTTP {code}: {msg}")),
    }
}

/// Read the answer of `POST /api/video/ai`.
pub fn parse_submit(code: u16, body: &Value) -> Result<String, Error> {
    if code == 200 && body.get("success").and_then(Value::as_bool) == Some(true) {
        if let Some(id) = body.pointer("/data/id").and_then(Value::as_str) {
            return Ok(id.into());
        }
    }
    Err(classify(code, body))
}

/// Read the answer of `POST /api/video/status`.
pub fn parse_status(code: u16, body: &Value) -> Result<Status, Error> {
    if code != 200 || body.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(classify(code, body));
    }
    let d = body.get("data").cloned().unwrap_or(Value::Null);
    let s = |k: &str| d.get(k).and_then(Value::as_str).map(String::from);
    Ok(Status { id: s("id").unwrap_or_default(), status: s("status").unwrap_or_default(), url: s("videoUrl"), failure_code: s("failureCode"), raw: d })
}

/// Read the answer of `POST /api/user/get-user-credits`.
pub fn parse_credits(code: u16, body: &Value) -> Result<i64, Error> {
    if code == 200 && body.get("code").and_then(Value::as_i64) == Some(0) {
        if let Some(n) = body.pointer("/data/remainingCredits").and_then(Value::as_i64) {
            return Ok(n);
        }
    }
    Err(classify(code, body))
}

/// Generate a clip with Seedance 2.5 (key from `EMAKI_API_KEY`) and return its URL.
pub fn generate_video(prompt: &str, params: &Params) -> Result<String, Error> {
    generate_video_with(prompt, MODEL_VIDEO, params)
}

/// Generate a clip with a given video model and return its URL.
pub fn generate_video_with(prompt: &str, model: &str, params: &Params) -> Result<String, Error> {
    Client::from_env()?.generate(prompt, model, params)
}

/// Generate an image with the cheapest image model and return its URL.
pub fn generate_image(prompt: &str, params: &Params) -> Result<String, Error> {
    generate_image_with(prompt, MODEL_IMAGE, params)
}

/// Generate an image with a given image model and return its URL.
pub fn generate_image_with(prompt: &str, model: &str, params: &Params) -> Result<String, Error> {
    let mut c = Client::from_env()?;
    c.poll_every = Duration::from_secs(3);
    c.generate(prompt, model, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submit_success_and_errors() {
        let ok = json!({"success": true, "data": {"id": "v1", "status": "generating", "message": "Video generation task started"}});
        assert_eq!(parse_submit(200, &ok).unwrap(), "v1");
        let bad = json!({"error": "Invalid API key. Create one at https://emaki.ai/settings/apikeys"});
        assert!(matches!(parse_submit(401, &bad), Err(Error::Auth(_))));
        assert!(matches!(parse_submit(402, &json!({"error": "Insufficient credits", "required_credits": 90})), Err(Error::Quota(_))));
        assert!(matches!(parse_submit(403, &json!({"error": "buy credits first"})), Err(Error::Plan(_))));
        assert!(matches!(parse_submit(409, &json!({"error": "Video generation is already in progress"})), Err(Error::Busy(_))));
        assert!(matches!(parse_submit(429, &json!({"error": "wait", "code": "content_cooldown", "retryAfterMinutes": 10})), Err(Error::Rejected(_))));
        assert!(matches!(parse_submit(400, &json!({"error": "blocked", "code": "content_blocked"})), Err(Error::Rejected(_))));
        match parse_submit(400, &json!({"error": "Missing required parameters: prompt and model"})) {
            Err(Error::Validation(m)) => assert!(m.contains("prompt")),
            e => panic!("{e:?}"),
        }
        assert!(matches!(parse_submit(500, &json!({"error": "Internal server error"})), Err(Error::Other(_))));
    }

    #[test]
    fn status_and_credits() {
        let done = json!({"success": true, "data": {"id": "v1", "status": "completed", "videoUrl": "https://cdn/x.mp4"}});
        let st = parse_status(200, &done).unwrap();
        assert_eq!(finished(&st).unwrap().as_deref(), Some("https://cdn/x.mp4"));
        let run = parse_status(200, &json!({"success": true, "data": {"id": "v1", "status": "generating", "videoUrl": null}})).unwrap();
        assert!(finished(&run).unwrap().is_none());
        let failed = parse_status(200, &json!({"success": true, "data": {"id": "v1", "status": "failed", "failureCode": "content_policy"}})).unwrap();
        assert!(matches!(finished(&failed), Err(Error::Failed(c)) if c == "content_policy"));
        assert!(matches!(parse_status(404, &json!({"error": "Video not found"})), Err(Error::Other(_))));
        let cr = json!({"code": 0, "message": "ok", "data": {"remainingCredits": 7, "hasPurchasedCredits": false}});
        assert_eq!(parse_credits(200, &cr).unwrap(), 7);
        let no = json!({"code": -1, "message": "Invalid API key. Create one at https://emaki.ai/settings/apikeys"});
        assert!(matches!(parse_credits(200, &no), Err(Error::Auth(_))));
    }

    #[test]
    fn params_and_empty_prompt() {
        let p = Params::new().set("duration", 8).set("resolution", "720p");
        assert_eq!(p.0["duration"], json!(8));
        assert!(matches!(Client::new("sk-x").submit(" ", MODEL_VIDEO, &p), Err(Error::Validation(_))));
    }

    /// Live: EMAKI_LIVE=1 EMAKI_API_KEY=... cargo test -- --ignored
    #[test]
    #[ignore]
    fn live_smoke() {
        let c = Client::from_env().unwrap();
        assert!(c.credits().unwrap() >= 0);
        let wrong = Client::new("sk-wrong-key-for-test");
        assert!(matches!(wrong.submit("test", MODEL_IMAGE, &Params::new()), Err(Error::Auth(_))));
    }
}
