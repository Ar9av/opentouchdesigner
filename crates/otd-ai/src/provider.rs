//! The five providers, behind one call.
//!
//! Three of them are a URL and a key. OpenAI and OpenRouter speak the same
//! wire format — OpenRouter exists to be a drop-in for it — so they share a
//! request builder and differ only in host and headers. Anthropic's is close
//! but not the same: the system prompt is a top-level field rather than a
//! message, auth is `x-api-key` rather than a bearer token, and it wants a
//! version header. That is the whole difference, and it is small enough that
//! a trait would cost more than it saved.
//!
//! Two of them are not a URL at all. Claude Code and Codex are subprocesses
//! run against the login already on the machine, so somebody paying for one
//! of those subscriptions does not also have to buy API credit. Everything
//! they need is in [`crate::cli`]; [`complete`] dispatches to it and the rest
//! of the crate never learns the difference. [`Provider::needs_key`] is the
//! seam — a provider that does not need a key does not get a key field, an
//! env var, or a line in `keys.conf`.
//!
//! Everything here is blocking. The caller runs it on a worker thread, which
//! is the same rule every device in this codebase follows: nothing that can
//! wait on the network is allowed anywhere near a frame.

use std::time::Duration;

use crate::keys::{Key, Keys, redact};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provider {
    Anthropic,
    OpenAi,
    OpenRouter,
    /// The `claude` CLI, on this machine's login.
    ClaudeCode,
    /// The `codex` CLI, on this machine's login.
    Codex,
}

impl Provider {
    pub const ALL: &'static [Provider] = &[
        Provider::Anthropic,
        Provider::OpenAi,
        Provider::OpenRouter,
        Provider::ClaudeCode,
        Provider::Codex,
    ];

    /// Whether this provider is paid for with an API key.
    ///
    /// `false` means it runs a CLI that is already signed in, and every piece
    /// of key handling — the config file, the env var, the password field —
    /// should skip it rather than show an empty box nobody can fill.
    pub fn needs_key(&self) -> bool {
        !matches!(self, Provider::ClaudeCode | Provider::Codex)
    }

    /// The stable name used in the config file and in the UI.
    pub fn id(&self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
            Provider::OpenRouter => "openrouter",
            Provider::ClaudeCode => "claude-code",
            Provider::Codex => "codex",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::OpenAi => "OpenAI",
            Provider::OpenRouter => "OpenRouter",
            Provider::ClaudeCode => "Claude Code",
            Provider::Codex => "Codex",
        }
    }

    pub fn parse(name: &str) -> Option<Provider> {
        Provider::ALL
            .iter()
            .copied()
            .find(|p| p.id().eq_ignore_ascii_case(name.trim()))
    }

    /// The environment variable this provider's key is conventionally in, if
    /// it takes a key at all.
    pub fn env_var(&self) -> Option<&'static str> {
        match self {
            Provider::Anthropic => Some("ANTHROPIC_API_KEY"),
            Provider::OpenAi => Some("OPENAI_API_KEY"),
            Provider::OpenRouter => Some("OPENROUTER_API_KEY"),
            Provider::ClaudeCode | Provider::Codex => None,
        }
    }

    /// The URL this provider is spoken to over, if it is spoken to over one.
    /// `None` means it is a subprocess — see [`crate::cli`].
    pub fn endpoint(&self) -> Option<&'static str> {
        match self {
            Provider::Anthropic => Some("https://api.anthropic.com/v1/messages"),
            Provider::OpenAi => Some("https://api.openai.com/v1/chat/completions"),
            Provider::OpenRouter => Some("https://openrouter.ai/api/v1/chat/completions"),
            Provider::ClaudeCode | Provider::Codex => None,
        }
    }

    /// Where a user goes to get set up. Shown next to an empty key field,
    /// because "paste your API key" is useless if you have not got one — and
    /// for the CLI providers, where to go and install the thing.
    pub fn console_url(&self) -> &'static str {
        match self {
            Provider::Anthropic => "https://console.anthropic.com/settings/keys",
            Provider::OpenAi => "https://platform.openai.com/api-keys",
            Provider::OpenRouter => "https://openrouter.ai/keys",
            Provider::ClaudeCode => "https://docs.claude.com/en/docs/claude-code/overview",
            Provider::Codex => "https://developers.openai.com/codex/cli",
        }
    }

    /// A few models worth defaulting to, newest first. Free text is still
    /// accepted — this list is a convenience, not a whitelist, because it
    /// will be out of date before anybody reads it.
    pub fn models(&self) -> &'static [&'static str] {
        match self {
            Provider::Anthropic => &[
                "claude-sonnet-4-5-20250929",
                "claude-opus-4-1-20250805",
                "claude-haiku-4-5-20251001",
            ],
            Provider::OpenAi => &["gpt-5", "gpt-5-mini", "gpt-4.1", "gpt-4o"],
            Provider::OpenRouter => &[
                "anthropic/claude-sonnet-4.5",
                "openai/gpt-5",
                "google/gemini-2.5-pro",
                "meta-llama/llama-4-maverick",
            ],
            // Aliases rather than dated names: the CLI resolves them to
            // whatever is current, which is the point of using it.
            Provider::ClaudeCode => &["sonnet", "opus", "haiku"],
            // Empty first, and it means "whatever this CLI is configured to
            // use". Not politeness — Codex hard-fails on a model the
            // signed-in account has no access to, and which models those are
            // depends on the plan, so guessing a default breaks first use for
            // somebody. The CLI already knows; let it answer.
            Provider::Codex => &["", "gpt-5.1-codex", "gpt-5"],
        }
    }

    pub fn default_model(&self) -> &'static str {
        self.models()[0]
    }
}

/// One completion request. No conversation history: every ask is
/// self-contained, because the thing being edited — the patch — is the state,
/// and it is already in the prompt.
#[derive(Clone, Debug)]
pub struct Request {
    pub provider: Provider,
    pub model: String,
    pub system: String,
    pub user: String,
    /// A reference image to work from, if there is one. See [`crate::vision`].
    pub image: Option<crate::vision::Image>,
    pub max_tokens: u32,
    pub timeout: Duration,
}

impl Request {
    pub fn new(provider: Provider, model: impl Into<String>) -> Request {
        Request {
            provider,
            model: model.into(),
            system: String::new(),
            user: String::new(),
            image: None,
            max_tokens: 8192,
            // Long enough for a reasoning model on a cold start, short enough
            // that a hung provider does not leave a thread waiting all show.
            timeout: Duration::from_secs(120),
        }
    }

    pub fn system(mut self, text: impl Into<String>) -> Request {
        self.system = text.into();
        self
    }

    pub fn user(mut self, text: impl Into<String>) -> Request {
        self.user = text.into();
        self
    }

    /// Attach a reference image to work from.
    pub fn image(mut self, image: impl Into<Option<crate::vision::Image>>) -> Request {
        self.image = image.into();
        self
    }

    /// The user turn as content blocks, in whichever dialect was asked for.
    ///
    /// The image goes *before* the text in both. Anthropic say so outright,
    /// and it is the better order anyway: the instructions that follow are
    /// about the picture, and a model that has not seen it yet is being asked
    /// to remember a brief rather than apply one.
    fn content(&self, anthropic: bool) -> serde_json::Value {
        let Some(image) = &self.image else {
            return serde_json::Value::String(self.user.clone());
        };
        let block = match anthropic {
            true => serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": image.media_type(),
                    "data": image.base64(),
                },
            }),
            false => serde_json::json!({
                "type": "image_url",
                "image_url": { "url": image.data_uri() },
            }),
        };
        serde_json::json!([block, { "type": "text", "text": self.user }])
    }

    /// The JSON body for this provider.
    pub fn body(&self) -> serde_json::Value {
        match self.provider {
            Provider::Anthropic => serde_json::json!({
                "model": self.model,
                "max_tokens": self.max_tokens,
                "system": self.system,
                "messages": [{ "role": "user", "content": self.content(true) }],
            }),
            // No wire body: these are subprocesses, and the prompt goes on
            // stdin. See `cli::complete`.
            Provider::ClaudeCode | Provider::Codex => serde_json::Value::Null,
            // OpenAI and OpenRouter: the system prompt is a message.
            Provider::OpenAi | Provider::OpenRouter => serde_json::json!({
                "model": self.model,
                "max_completion_tokens": self.max_tokens,
                "messages": [
                    { "role": "system", "content": self.system },
                    { "role": "user", "content": self.content(false) },
                ],
            }),
        }
    }
}

/// Pull the assistant's text out of whichever shape came back.
pub fn extract_text(provider: Provider, body: &serde_json::Value) -> Result<String, String> {
    // A provider that returns 200 with an error object in it is common
    // enough to check for first, whatever the shape.
    if let Some(error) = body.get("error") {
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(message.to_string());
    }

    let text = match provider {
        Provider::Anthropic => body
            .get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default(),
        // Never reached: `complete` sends these to `cli` before there is a
        // body to read. Explicit rather than swept into the arm below, so
        // that a wire shape is never invented for a provider that has none.
        Provider::ClaudeCode | Provider::Codex => {
            return Err(format!("{} has no wire format", provider.label()));
        }
        Provider::OpenAi | Provider::OpenRouter => body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or_default()
            .to_string(),
    };

    if text.trim().is_empty() {
        // A refusal or a length stop reads as an empty reply; say which.
        let reason = finish_reason(provider, body);
        return Err(match reason {
            Some(r) => format!("the model returned nothing (stopped: {r})"),
            None => "the model returned nothing".into(),
        });
    }
    Ok(text)
}

fn finish_reason(provider: Provider, body: &serde_json::Value) -> Option<String> {
    let value = match provider {
        Provider::Anthropic => body.get("stop_reason"),
        _ => body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|c| c.get("finish_reason")),
    };
    value.and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Send a request and return the assistant's text.
///
/// Blocking. Call it from a worker thread.
pub fn complete(request: &Request, key: &Key, keys: &Keys) -> Result<String, String> {
    // The two that are not a URL. Dispatched before anything HTTP-shaped
    // happens, so no key, agent, or endpoint is consulted for them.
    if !request.provider.needs_key() {
        return crate::cli::complete(request);
    }

    let Some(endpoint) = request.provider.endpoint() else {
        return Err(format!("{} has no endpoint", request.provider.label()));
    };
    if key.is_empty() {
        return Err(format!(
            "no API key for {} — paste one, or set {}",
            request.provider.label(),
            request
                .provider
                .env_var()
                .unwrap_or("it in the environment")
        ));
    }

    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(request.timeout))
        // Handle non-2xx ourselves. The default turns a 401 into an error
        // that has thrown the body away, and the body is where the provider
        // says *which* thing was wrong — worth far more than the number.
        .http_status_as_error(false)
        .build()
        .new_agent();

    let mut call = agent.post(endpoint);
    call = match request.provider {
        Provider::Anthropic => call
            .header("x-api-key", key.expose())
            .header("anthropic-version", "2023-06-01"),
        _ => call.header("authorization", &format!("Bearer {}", key.expose())),
    };
    if request.provider == Provider::OpenRouter {
        // OpenRouter attributes traffic by these, and asks that clients send
        // them. Neither identifies the user.
        call = call
            .header("http-referer", "https://github.com/Ar9av/opentouchdesigner")
            .header("x-title", "OpenTouchDesigner");
    }

    let result = call.send_json(request.body());

    // Every path out of here goes through `redact`: providers quote the
    // request back in error bodies often enough to matter.
    let mut response =
        result.map_err(|e| redact(&format!("could not reach the provider: {e}"), keys))?;
    let status = response.status().as_u16();

    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| redact(&format!("could not read the reply: {e}"), keys))?;

    if !(200..300).contains(&status) {
        return Err(redact(&explain_status(status, &text), keys));
    }

    let body: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| redact(&format!("the reply was not JSON: {e}"), keys))?;
    extract_text(request.provider, &body).map_err(|e| redact(&e, keys))
}

/// Turn a failed response into something worth reading.
///
/// The provider's own message first when there is one — "invalid x-api-key"
/// and "your credit balance is too low" are different problems that both
/// arrive as 401-shaped disappointment — then what to do about it.
fn explain_status(code: u16, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message").or(Some(e)))
                .map(|m| match m.as_str() {
                    Some(s) => s.to_string(),
                    None => m.to_string(),
                })
                .or_else(|| v.get("message").and_then(|m| m.as_str()).map(String::from))
        })
        .unwrap_or_else(|| body.chars().take(200).collect());

    let advice = match code {
        401 | 403 => "check the key, and that it is for this provider",
        404 => "check the model name — this provider may not have it",
        413 => "the request was too large; try a smaller network or prompt",
        429 => "rate limited or out of credit; wait, or check billing",
        500..=599 => "the provider is having trouble; try again",
        _ => "the provider refused the request",
    };
    if detail.trim().is_empty() {
        format!("{code}: {advice}")
    } else {
        format!("{code}: {} — {advice}", detail.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_and_openai_bodies_differ_where_they_have_to() {
        let anthropic = Request::new(Provider::Anthropic, "claude-sonnet-4-5-20250929")
            .system("be terse")
            .user("hello");
        let body = anthropic.body();
        // Anthropic takes the system prompt as a field, not a message.
        assert_eq!(body["system"], "be terse");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["max_tokens"], 8192);

        let openai = Request::new(Provider::OpenAi, "gpt-5")
            .system("be terse")
            .user("hello");
        let body = openai.body();
        assert!(body.get("system").is_none());
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["content"], "hello");
    }

    #[test]
    fn openrouter_is_openai_on_the_wire() {
        let a = Request::new(Provider::OpenRouter, "x")
            .system("s")
            .user("u");
        let b = Request::new(Provider::OpenAi, "x").system("s").user("u");
        assert_eq!(a.body(), b.body());
        assert_ne!(a.provider.endpoint(), b.provider.endpoint());
    }

    #[test]
    fn each_shape_of_reply_is_understood() {
        let anthropic = serde_json::json!({
            "content": [
                { "type": "thinking", "thinking": "hmm" },
                { "type": "text", "text": "the answer" }
            ],
            "stop_reason": "end_turn"
        });
        assert_eq!(
            extract_text(Provider::Anthropic, &anthropic).unwrap(),
            "the answer"
        );

        let openai = serde_json::json!({
            "choices": [{ "message": { "content": "the answer" }, "finish_reason": "stop" }]
        });
        assert_eq!(
            extract_text(Provider::OpenAi, &openai).unwrap(),
            "the answer"
        );
        assert_eq!(
            extract_text(Provider::OpenRouter, &openai).unwrap(),
            "the answer"
        );
    }

    #[test]
    fn a_two_hundred_carrying_an_error_is_still_an_error() {
        // Both providers do this, and treating it as an empty reply would
        // report "the model returned nothing" for an expired key.
        let body = serde_json::json!({ "error": { "message": "insufficient_quota" } });
        let e = extract_text(Provider::OpenAi, &body).unwrap_err();
        assert!(e.contains("insufficient_quota"), "{e}");
    }

    #[test]
    fn an_empty_reply_says_why_it_was_empty() {
        let body = serde_json::json!({
            "choices": [{ "message": { "content": "" }, "finish_reason": "length" }]
        });
        let e = extract_text(Provider::OpenAi, &body).unwrap_err();
        assert!(e.contains("length"), "{e}");
    }

    #[test]
    fn a_failure_reports_what_the_provider_said_not_just_the_number() {
        // "invalid x-api-key" and "your credit balance is too low" are
        // different problems that both arrive as 401-shaped disappointment.
        let body = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;
        let text = explain_status(401, body);
        assert!(text.contains("invalid x-api-key"), "{text}");
        assert!(text.contains("check the key"), "{text}");

        let openai = r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota"}}"#;
        assert!(explain_status(429, openai).contains("exceeded your current quota"));

        // A body that is not JSON at all — an HTML error page from a proxy —
        // still has to produce something a person can act on.
        let html = explain_status(503, "<html>Service Unavailable</html>");
        assert!(html.contains("503"), "{html}");
        assert!(html.contains("try again"), "{html}");

        assert!(explain_status(404, "").contains("model"));
    }

    #[test]
    fn provider_names_round_trip() {
        for p in Provider::ALL {
            assert_eq!(Provider::parse(p.id()), Some(*p));
            assert_eq!(Provider::parse(&p.id().to_uppercase()), Some(*p));
            assert!(!p.models().is_empty());
            assert!(p.console_url().starts_with("https://"));
        }
        assert_eq!(Provider::parse("gemini"), None);
    }

    #[test]
    fn a_key_provider_has_somewhere_to_send_the_key() {
        // The two halves of the enum, stated as the invariant that keeps
        // `complete` honest: needing a key and having an endpoint are the
        // same question asked twice, and nothing may answer them differently.
        for p in Provider::ALL {
            assert_eq!(
                p.needs_key(),
                p.endpoint().is_some(),
                "{p:?} disagrees with itself"
            );
            assert_eq!(p.needs_key(), p.env_var().is_some(), "{p:?}");
            if p.needs_key() {
                assert!(p.endpoint().unwrap().starts_with("https://"));
                assert!(!p.default_model().is_empty(), "{p:?}");
            }
        }
        assert!(!Provider::ClaudeCode.needs_key());
        assert!(!Provider::Codex.needs_key());
    }

    #[test]
    fn codex_defaults_to_whatever_its_cli_is_configured_for() {
        // Empty is deliberate and `cli::complete` reads it as "pass no
        // -m flag". Codex hard-fails on a model the account cannot use, and
        // the plan decides which those are, so there is no safe guess to
        // make from here.
        assert_eq!(Provider::Codex.default_model(), "");
        // Claude Code has no such problem: the aliases always resolve.
        assert_eq!(Provider::ClaudeCode.default_model(), "sonnet");
    }

    /// A tiny real PNG, so the image path is exercised with bytes rather than
    /// a stub that cannot be decoded.
    fn image() -> crate::vision::Image {
        let buffer = image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(buffer)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        crate::vision::Image::decode(&png.into_inner()).unwrap()
    }

    #[test]
    fn without_an_image_the_user_turn_stays_a_plain_string() {
        // The shape three providers have always taken, and the one thing that
        // must not change for anybody not using this feature.
        let request = Request::new(Provider::Anthropic, "m").user("hello");
        assert_eq!(request.body()["messages"][0]["content"], "hello");
        let openai = Request::new(Provider::OpenAi, "m").user("hello");
        assert_eq!(openai.body()["messages"][1]["content"], "hello");
    }

    #[test]
    fn an_image_goes_out_in_each_providers_own_dialect() {
        let request = Request::new(Provider::Anthropic, "m")
            .user("rebuild this")
            .image(image());
        let content = &request.body()["messages"][0]["content"];
        // Image first: the instructions after it are about the picture.
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["source"]["type"], "base64");
        assert_eq!(content[0]["source"]["media_type"], "image/jpeg");
        assert!(!content[0]["source"]["data"].as_str().unwrap().is_empty());
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[1]["text"], "rebuild this");

        // OpenAI takes the same picture as a data URI, and OpenRouter is
        // OpenAI on the wire here too.
        for provider in [Provider::OpenAi, Provider::OpenRouter] {
            let request = Request::new(provider, "m")
                .user("rebuild this")
                .image(image());
            let content = &request.body()["messages"][1]["content"];
            assert_eq!(content[0]["type"], "image_url");
            let url = content[0]["image_url"]["url"].as_str().unwrap();
            assert!(url.starts_with("data:image/jpeg;base64,"), "{url:.40}");
            assert_eq!(content[1]["text"], "rebuild this");
        }
    }

    #[test]
    fn a_cli_provider_has_no_wire_format() {
        // Rather than quietly borrowing OpenAI's, which would produce a body
        // that looks sendable and is not.
        let request = Request::new(Provider::ClaudeCode, "sonnet")
            .system("s")
            .user("u");
        assert!(request.body().is_null());
        let body = serde_json::json!({ "choices": [] });
        assert!(extract_text(Provider::Codex, &body).is_err());
    }
}
