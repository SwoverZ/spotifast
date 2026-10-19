//! Translation of song lyrics through a provider the listener configures.
//!
//! Only the words of the lines are sent, never the track's metadata. The API
//! key lives in its own owner-only file under the app's state directory, and
//! is never written to settings.json or logged.
//!
//! OpenRouter lists which of its models are free (an id ending in `:free`, or
//! zero prompt and completion prices). Google AI Studio does not mark a model
//! as free in its API: every key there is on the free tier within its rate
//! limits, so every generation model is offered.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::settings::TranslateProvider;

const OPENROUTER_API: &str = "https://openrouter.ai/api/v1";
const GOOGLE_API: &str = "https://generativelanguage.googleapis.com/v1beta";
/// A translation of a fixed set of lines does not change.
const CACHE_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Free models count tokens per minute, so a long song is translated in
/// batches rather than one request per line or one giant request.
const MAX_LINES_PER_REQUEST: usize = 60;

/// The languages the picker offers, as ISO 639-1 codes and their names in the
/// language itself, so a reader can find theirs whatever the app displays.
pub const LANGUAGES: &[(&str, &str)] = &[
    ("af", "Afrikaans"),
    ("ar", "العربية"),
    ("bg", "Български"),
    ("bn", "বাংলা"),
    ("ca", "Català"),
    ("cs", "Čeština"),
    ("da", "Dansk"),
    ("de", "Deutsch"),
    ("el", "Ελληνικά"),
    ("en", "English"),
    ("es", "Español"),
    ("et", "Eesti"),
    ("fa", "فارسی"),
    ("fi", "Suomi"),
    ("fr", "Français"),
    ("he", "עברית"),
    ("hi", "हिन्दी"),
    ("hr", "Hrvatski"),
    ("hu", "Magyar"),
    ("id", "Bahasa Indonesia"),
    ("is", "Íslenska"),
    ("it", "Italiano"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("lt", "Lietuvių"),
    ("lv", "Latviešu"),
    ("ms", "Bahasa Melayu"),
    ("nl", "Nederlands"),
    ("no", "Norsk"),
    ("pl", "Polski"),
    ("pt", "Português"),
    ("ro", "Română"),
    ("ru", "Русский"),
    ("sk", "Slovenčina"),
    ("sl", "Slovenščina"),
    ("sr", "Српски"),
    ("sv", "Svenska"),
    ("th", "ไทย"),
    ("tr", "Türkçe"),
    ("uk", "Українська"),
    ("vi", "Tiếng Việt"),
    ("zh", "中文"),
];

/// A model offered by the configured provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

/// One translation job: the words to translate and where they are for.
#[derive(Clone, Debug)]
pub struct Request {
    /// The track the answer is for, so a stale one is ignored.
    pub uri: String,
    pub lines: Vec<String>,
    /// ISO 639-1 code; empty follows the desktop language.
    pub language: String,
    pub provider: TranslateProvider,
    /// Empty picks the first free model the provider lists.
    pub model: Option<String>,
}

/// The language the picker's System entry resolves to, as an ISO 639-1 code.
pub fn system_code() -> String {
    crate::i18n::Locale::from_system()
        .tag()
        .split('-')
        .next()
        .unwrap_or("en")
        .to_string()
}

/// The language to translate into: `code` unless it is empty.
pub fn resolve_language(code: &str) -> String {
    let code = code.trim();
    if code.is_empty() {
        system_code()
    } else {
        code.to_string()
    }
}

/// The name the picker shows for `code`, or the code itself when unknown.
pub fn language_name(code: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|(each, _)| *each == code)
        .map(|(_, name)| *name)
}

fn language_wording(code: &str) -> String {
    match language_name(code) {
        Some(name) => format!("{name} (ISO 639-1: {code})"),
        None => format!("the language with ISO 639-1 code {code}"),
    }
}

/// Whether an API key has been saved. Cheap enough for the settings page.
pub fn key_present(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

/// The saved API key, if there is one. Never log the result.
pub fn read_key(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

/// Free models the provider offers with `key`.
pub async fn free_models(
    http: &reqwest::Client,
    provider: TranslateProvider,
    key: &str,
) -> Result<Vec<ModelInfo>, String> {
    match provider {
        TranslateProvider::OpenRouter => openrouter_models(http, key, OPENROUTER_API).await,
        TranslateProvider::GoogleStudio => google_models(http, key, GOOGLE_API).await,
    }
}

/// Translates `lines` into `language`, one batch at a time.
pub async fn translate_lines(
    http: &reqwest::Client,
    provider: TranslateProvider,
    key: &str,
    model: &str,
    language: &str,
    lines: &[String],
) -> Result<Vec<Option<String>>, String> {
    let mut translated = vec![None; lines.len()];
    for (batch, chunk) in lines.chunks(MAX_LINES_PER_REQUEST).enumerate() {
        let answers = match provider {
            TranslateProvider::OpenRouter => {
                openrouter_translate(http, key, model, language, chunk, OPENROUTER_API).await?
            }
            TranslateProvider::GoogleStudio => {
                google_translate(http, key, model, language, chunk, GOOGLE_API).await?
            }
        };
        let offset = batch * MAX_LINES_PER_REQUEST;
        for (index, text) in answers {
            if let Some(slot) = translated.get_mut(offset + index) {
                *slot = Some(text);
            }
        }
    }
    Ok(translated)
}

// ---- OpenRouter -------------------------------------------------------------

#[derive(Deserialize)]
struct OpenRouterModels {
    #[serde(default)]
    data: Vec<OpenRouterModel>,
}

#[derive(Deserialize)]
struct OpenRouterModel {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    pricing: OpenRouterPricing,
}

#[derive(Deserialize, Default)]
struct OpenRouterPricing {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
}

fn parse_openrouter_models(body: &str) -> Result<Vec<ModelInfo>, String> {
    let answer: OpenRouterModels = serde_json::from_str(body)
        .map_err(|_| "OpenRouter sent an unexpected answer".to_string())?;
    let mut models: Vec<ModelInfo> = answer
        .data
        .into_iter()
        .filter(is_free_model)
        .map(|model| ModelInfo {
            name: if model.name.trim().is_empty() {
                model.id.clone()
            } else {
                model.name
            },
            id: model.id,
        })
        .collect();
    models.sort_by_key(|model| model.name.to_lowercase());
    Ok(models)
}

fn is_free_model(model: &OpenRouterModel) -> bool {
    if model.id.ends_with(":free") {
        return true;
    }
    let zero = |price: &Option<String>| {
        price
            .as_deref()
            .and_then(|price| price.trim().parse::<f64>().ok())
            == Some(0.0)
    };
    zero(&model.pricing.prompt) && zero(&model.pricing.completion)
}

async fn openrouter_models(
    http: &reqwest::Client,
    key: &str,
    api: &str,
) -> Result<Vec<ModelInfo>, String> {
    let response = http
        .get(format!("{api}/models"))
        .bearer_auth(key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|error| error.without_url().to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(http_error("OpenRouter", status));
    }
    let body = response
        .text()
        .await
        .map_err(|_| "OpenRouter sent an unexpected answer".to_string())?;
    parse_openrouter_models(&body)
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    content: String,
}

async fn openrouter_translate(
    http: &reqwest::Client,
    key: &str,
    model: &str,
    language: &str,
    lines: &[String],
    api: &str,
) -> Result<Vec<(usize, String)>, String> {
    let body = serde_json::json!({
        "model": model,
        "temperature": 0,
        "messages": [
            { "role": "system", "content": system_prompt(language) },
            { "role": "user", "content": numbered_lines(lines) },
        ],
    });
    let answer = with_retry("OpenRouter", || {
        http.post(format!("{api}/chat/completions"))
            .bearer_auth(key)
            .header("Accept", "application/json")
            .json(&body)
    })
    .await?;
    let answer: ChatResponse = serde_json::from_value(answer)
        .map_err(|_| "OpenRouter sent an unexpected answer".to_string())?;
    let content = answer
        .choices
        .first()
        .map(|choice| choice.message.content.as_str())
        .ok_or("OpenRouter returned no translation")?;
    parse_translations(content, lines.len())
}

// ---- Google AI Studio -------------------------------------------------------

#[derive(Deserialize)]
struct GoogleModels {
    #[serde(default)]
    models: Vec<GoogleModel>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleModel {
    name: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
}

fn parse_google_models(body: &str) -> Result<Vec<ModelInfo>, String> {
    let answer: GoogleModels = serde_json::from_str(body)
        .map_err(|_| "Google AI Studio sent an unexpected answer".to_string())?;
    let mut models: Vec<ModelInfo> = answer
        .models
        .into_iter()
        .filter(|model| {
            model
                .supported_generation_methods
                .iter()
                .any(|method| method == "generateContent")
        })
        .filter_map(|model| {
            let id = model.name.strip_prefix("models/")?.to_string();
            Some(ModelInfo {
                name: model.display_name.unwrap_or_else(|| id.clone()),
                id,
            })
        })
        .collect();
    models.sort_by_key(|model| model.name.to_lowercase());
    Ok(models)
}

async fn google_models(
    http: &reqwest::Client,
    key: &str,
    api: &str,
) -> Result<Vec<ModelInfo>, String> {
    let response = http
        .get(format!("{api}/models"))
        .header("x-goog-api-key", key)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|error| error.without_url().to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(http_error("Google AI Studio", status));
    }
    let body = response
        .text()
        .await
        .map_err(|_| "Google AI Studio sent an unexpected answer".to_string())?;
    parse_google_models(&body)
}

#[derive(Deserialize)]
struct GoogleResponse {
    #[serde(default)]
    candidates: Vec<GoogleCandidate>,
}

#[derive(Deserialize)]
struct GoogleCandidate {
    content: GoogleContent,
}

#[derive(Deserialize)]
struct GoogleContent {
    #[serde(default)]
    parts: Vec<GooglePart>,
}

#[derive(Deserialize)]
struct GooglePart {
    #[serde(default)]
    text: Option<String>,
}

async fn google_translate(
    http: &reqwest::Client,
    key: &str,
    model: &str,
    language: &str,
    lines: &[String],
    api: &str,
) -> Result<Vec<(usize, String)>, String> {
    let prompt = format!("{}\n\n{}", system_prompt(language), numbered_lines(lines));
    let body = serde_json::json!({
        "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
        "generationConfig": { "temperature": 0, "responseMimeType": "application/json" },
    });
    let answer = with_retry("Google AI Studio", || {
        http.post(format!("{api}/models/{model}:generateContent"))
            // The key travels in the header, never the URL: URLs are logged.
            .header("x-goog-api-key", key)
            .header("Accept", "application/json")
            .json(&body)
    })
    .await?;
    let answer: GoogleResponse = serde_json::from_value(answer)
        .map_err(|_| "Google AI Studio sent an unexpected answer".to_string())?;
    let content = answer
        .candidates
        .first()
        .map(|candidate| {
            candidate
                .content
                .parts
                .iter()
                .filter_map(|part| part.text.as_deref())
                .collect::<String>()
        })
        .filter(|content| !content.trim().is_empty())
        .ok_or("Google AI Studio returned no translation")?;
    parse_translations(&content, lines.len())
}

// ---- answers ----------------------------------------------------------------

fn system_prompt(language: &str) -> String {
    format!(
        "You translate song lyrics into {}. Translate every line faithfully and keep names and the meaning. \
         Answer with only a JSON array of objects like {{\"n\":0,\"t\":\"translated line\"}}, one for every input line, \
         in the same order. Do not add comments or explanations.",
        language_wording(language)
    )
}

fn numbered_lines(lines: &[String]) -> String {
    let entries: Vec<serde_json::Value> = lines
        .iter()
        .enumerate()
        .map(|(n, text)| serde_json::json!({ "n": n, "t": text }))
        .collect();
    serde_json::Value::Array(entries).to_string()
}

#[derive(Deserialize)]
struct TranslationEntry {
    #[serde(alias = "index")]
    n: usize,
    #[serde(alias = "text", alias = "translation")]
    t: String,
}

/// The translated lines the model answered, by their index in the request.
fn parse_translations(raw: &str, expected: usize) -> Result<Vec<(usize, String)>, String> {
    let trimmed = raw.trim();
    let start = trimmed
        .find('[')
        .ok_or("the model did not answer with a list of lines")?;
    let end = trimmed
        .rfind(']')
        .ok_or("the model did not answer with a list of lines")?;
    let list = &trimmed[start..=end];
    let mut answers = if let Ok(entries) = serde_json::from_str::<Vec<TranslationEntry>>(list) {
        entries
            .into_iter()
            .filter(|entry| entry.n < expected && !entry.t.trim().is_empty())
            .map(|entry| (entry.n, entry.t))
            .collect::<Vec<_>>()
    } else if let Ok(lines) = serde_json::from_str::<Vec<String>>(list) {
        lines
            .into_iter()
            .take(expected)
            .enumerate()
            .filter(|(_, text)| !text.trim().is_empty())
            .collect()
    } else {
        return Err("the model's answer was not a list of translated lines".into());
    };
    if answers.is_empty() {
        return Err("the model returned no translated lines".into());
    }
    answers.sort_by_key(|(index, _)| *index);
    answers.dedup_by_key(|(index, _)| *index);
    Ok(answers)
}

fn http_error(provider: &str, status: reqwest::StatusCode) -> String {
    match status {
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            format!("{provider} rejected the API key")
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            format!("{provider} is rate limiting requests, try again in a minute")
        }
        _ => format!("{provider} answered {status}"),
    }
}

/// Sends a request, waiting out one rate limit or server fault. `build` runs
/// again for the retry, so no request body is cloned.
async fn with_retry(
    provider: &str,
    mut build: impl FnMut() -> reqwest::RequestBuilder,
) -> Result<serde_json::Value, String> {
    for attempt in 0..2 {
        let response = build()
            .send()
            .await
            .map_err(|error| error.without_url().to_string())?;
        let status = response.status();
        let retryable =
            status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
        if retryable && attempt == 0 {
            let wait = retry_after(&response)
                .unwrap_or(Duration::from_secs(2))
                .min(Duration::from_secs(20));
            tokio::time::sleep(wait).await;
            continue;
        }
        if !status.is_success() {
            return Err(http_error(provider, status));
        }
        return response
            .json()
            .await
            .map_err(|_| format!("{provider} sent an unexpected answer"));
    }
    Err(format!("{provider} did not answer"))
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

// ---- cache ------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct Cached {
    lines: Vec<Option<String>>,
}

fn cache_path(cache_dir: &Path, request: &Request, model: &str, language: &str) -> PathBuf {
    cache_dir.join(format!(
        "translation-{}.json",
        cache_key(request, model, language)
    ))
}

fn cache_key(request: &Request, model: &str, language: &str) -> String {
    let mut digest = Sha1::new();
    digest.update(request.uri.as_bytes());
    digest.update(b"\x1f");
    digest.update(language.as_bytes());
    digest.update(b"\x1f");
    digest.update(request.provider.label().as_bytes());
    digest.update(b"\x1f");
    digest.update(model.as_bytes());
    for line in &request.lines {
        digest.update(b"\x1f");
        digest.update(line.as_bytes());
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The cached translation for this request, while it is fresh.
pub fn cached(
    cache_dir: &Path,
    request: &Request,
    model: &str,
    language: &str,
) -> Option<Vec<Option<String>>> {
    let path = cache_path(cache_dir, request, model, language);
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    if modified.elapsed().unwrap_or(CACHE_LIFETIME) >= CACHE_LIFETIME {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<Cached>(&text)
        .ok()
        .map(|cached| cached.lines)
        .filter(|lines| lines.len() == request.lines.len())
}

/// Remembers a translation. Queued from a background task; never the UI thread.
pub fn store(
    cache_dir: &Path,
    request: &Request,
    model: &str,
    language: &str,
    lines: &[Option<String>],
) {
    let _ = std::fs::create_dir_all(cache_dir);
    let Ok(text) = serde_json::to_string(&Cached {
        lines: lines.to_vec(),
    }) else {
        return;
    };
    let _ = std::fs::write(cache_path(cache_dir, request, model, language), text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrouter_free_models_are_the_ones_marked_free() {
        let body = r#"{
            "data": [
                {"id": "paid/model", "name": "Paid", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
                {"id": "open/free:free", "name": "Free by name", "pricing": {"prompt": "0", "completion": "0"}},
                {"id": "open/zero", "name": "Zero priced", "pricing": {"prompt": "0", "completion": "0"}},
                {"id": "open/partial", "name": "Partial", "pricing": {"prompt": "0", "completion": "0.1"}}
            ]
        }"#;
        let models = parse_openrouter_models(body).unwrap();
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, vec!["open/free:free", "open/zero"]);
    }

    #[test]
    fn google_models_keep_only_generation_models() {
        let body = r#"{
            "models": [
                {"name": "models/gemini-2.5-flash", "displayName": "Gemini 2.5 Flash",
                 "supportedGenerationMethods": ["generateContent", "countTokens"]},
                {"name": "models/text-embedding-004", "displayName": "Embedding",
                 "supportedGenerationMethods": ["embedContent"]},
                {"name": "models/gemini-2.5-pro", "displayName": "Gemini 2.5 Pro",
                 "supportedGenerationMethods": ["generateContent"]}
            ]
        }"#;
        let models = parse_google_models(body).unwrap();
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, vec!["gemini-2.5-flash", "gemini-2.5-pro"]);
    }

    #[test]
    fn translations_parse_inside_code_fences() {
        let answer = "```json\n[{\"n\": 1, \"t\": \"Hola\"}, {\"n\": 0, \"t\": \"Adiós\"}]\n```";
        let parsed = parse_translations(answer, 2).unwrap();
        assert_eq!(
            parsed,
            vec![(0, "Adiós".to_string()), (1, "Hola".to_string())]
        );
    }

    #[test]
    fn translations_accept_a_plain_list_and_ignore_unknown_indices() {
        let parsed = parse_translations("[\"One\", \"Two\"]", 2).unwrap();
        assert_eq!(parsed, vec![(0, "One".to_string()), (1, "Two".to_string())]);
        assert!(parse_translations("[{\"n\": 9, \"t\": \"Far\"}]", 2).is_err());
        assert!(parse_translations("no list here", 2).is_err());
    }

    #[test]
    fn the_cache_key_covers_the_language_provider_model_and_lines() {
        let request = Request {
            uri: "spotify:track:one".into(),
            lines: vec!["Hello".into()],
            language: "es".into(),
            provider: TranslateProvider::OpenRouter,
            model: None,
        };
        let key = cache_key(&request, "model-one", "es");
        assert_eq!(key, cache_key(&request, "model-one", "es"));
        assert_ne!(key, cache_key(&request, "model-two", "es"));
        assert_ne!(key, cache_key(&request, "model-one", "de"));
        let mut other = request.clone();
        other.lines = vec!["Goodbye".into()];
        assert_ne!(key, cache_key(&other, "model-one", "es"));
    }

    #[test]
    fn an_empty_language_follows_the_system() {
        assert_eq!(resolve_language("  "), system_code());
        assert_eq!(resolve_language("ja"), "ja");
    }

    #[test]
    fn the_key_never_goes_in_the_url() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            use std::io::{Read, Write};
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"data\":[]}",
                )
                .unwrap();
            String::from_utf8_lossy(&buffer[..read]).into_owned()
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = reqwest::Client::new();
        runtime.block_on(async {
            let api = format!("http://{address}");
            let models = openrouter_models(&client, "secret-key", &api)
                .await
                .unwrap();
            assert!(models.is_empty());
        });
        let request = server.join().unwrap();
        let request_line = request.lines().next().unwrap_or_default();
        assert!(request_line.starts_with("GET /models "));
        assert!(!request_line.contains("secret-key"));
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer secret-key")
        );
    }

    #[test]
    fn only_the_lines_are_sent_to_the_provider() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            use std::io::{Read, Write};
            let mut buffer = [0u8; 8192];
            let read = stream.read(&mut buffer).unwrap();
            let body = "{\"choices\":[{\"message\":{\"content\":\"[{\\\"n\\\":0,\\\"t\\\":\\\"Hola\\\"}]\"}}]}";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&buffer[..read]).into_owned()
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = reqwest::Client::new();
        let lines = vec!["Hello".to_string()];
        let translated = runtime.block_on(async {
            let api = format!("http://{address}");
            openrouter_translate(&client, "secret-key", "model", "es", &lines, &api)
                .await
                .unwrap()
        });
        assert_eq!(translated, vec![(0, "Hola".to_string())]);
        let request = server.join().unwrap();
        assert!(request.contains("Hello"));
        assert!(!request.contains("spotify:track"));
    }
}
