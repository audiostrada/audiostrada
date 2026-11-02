//! TONE3000: the user signs in on their site (OAuth with PKCE, in the browser), then picks
//! tones there or from the lists of the API; their NAM models go into the amp library.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::i18n::tr;
use crate::nam_library::file_name;
use crate::trf;

const CLIENT_ID: &str = "t3k_pub_aQ5mJLm9KP2grSNnf1_SNF-8Ynf5PrgM";
const API: &str = "https://www.tone3000.com/api/v1";
const PORT: u16 = 47_803;
const WAIT: Duration = Duration::from_secs(600);

/// Where the browser comes back to; TONE3000 accepts this loopback address.
fn redirect_uri() -> String {
    format!("http://127.0.0.1:{PORT}/callback")
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Waiting,
    Downloading(String),
    Done { folder: PathBuf, files: usize },
    SignedIn,
    Failed(String),
}

/// A pick and download in progress.
#[derive(Debug)]
pub struct Session {
    status: Arc<Mutex<Status>>,
}

impl Session {
    pub fn status(&self) -> Status {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or(Status::Failed(tr("lost").into()))
    }
}

fn base64url(bytes: &[u8]) -> String {
    audiostrada_core::base64::encode(bytes)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_owned()
}

fn random(n: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn percent(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn unpercent(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The value of `key` in a query string.
fn query(text: &str, key: &str) -> Option<String> {
    text.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| unpercent(v))
    })
}

pub(crate) fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .http_status_as_error(false);
    #[cfg(not(target_os = "linux"))]
    let config = config.tls_config(
        ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .build(),
    );
    ureq::Agent::new_with_config(config.build())
}

/// What the browser page asks the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// Only sign in.
    SignIn,
    /// Pick any tone of the catalog, then download it.
    Select,
}

/// The tokens kept between runs, in the configuration folder (readable by the user only).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct Tokens {
    access: String,
    refresh: String,
    expires_at: u64,
}

fn token_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("audiostrada").join("tone3000.json"))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn load_tokens() -> Option<Tokens> {
    let text = std::fs::read_to_string(token_file()?).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_tokens(tokens: &Tokens) -> Result<(), String> {
    let file = token_file().ok_or(tr("no configuration folder"))?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string(tokens).map_err(|e| e.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(&file).map_err(|e| e.to_string())?;
    out.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

pub fn signed_in() -> bool {
    load_tokens().is_some()
}

pub fn sign_out() {
    if let Some(file) = token_file() {
        let _ = std::fs::remove_file(file);
    }
}

/// Reads a token answer and keeps it.
fn keep_tokens(text: &str) -> Result<String, String> {
    let token: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let field = |k: &str| token.get(k).and_then(|v| v.as_str()).map(str::to_owned);
    let access = field("access_token")
        .ok_or_else(|| trf!("TONE3000 sign-in failed: {text}", text = text))?;
    let tokens = Tokens {
        refresh: field("refresh_token").unwrap_or_default(),
        expires_at: now()
            + token
                .get("expires_in")
                .and_then(|v| v.as_u64())
                .unwrap_or(3600),
        access: access.clone(),
    };
    save_tokens(&tokens)?;
    Ok(access)
}

/// A valid access token, refreshed when it is about to expire.
fn access_token(agent: &ureq::Agent) -> Result<String, String> {
    let tokens = load_tokens().ok_or(tr("not signed in to TONE3000"))?;
    if now() + 60 < tokens.expires_at {
        return Ok(tokens.access);
    }
    let mut response = agent
        .post(format!("{API}/oauth/token"))
        .send_form([
            ("grant_type", "refresh_token"),
            ("refresh_token", tokens.refresh.as_str()),
            ("client_id", CLIENT_ID),
        ])
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    if status.as_u16() == 400 || status.as_u16() == 401 {
        sign_out();
        return Err(tr("the TONE3000 session has expired: sign in again").into());
    }
    keep_tokens(&text)
}

fn get_json(agent: &ureq::Agent, bearer: &str, url: &str) -> Result<serde_json::Value, String> {
    let mut response = agent
        .get(url)
        .header("Authorization", bearer)
        .call()
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    if status.as_u16() == 401 {
        sign_out();
        return Err(tr("the TONE3000 session has expired: sign in again").into());
    }
    if !status.is_success() {
        return Err(trf!("TONE3000 answered {status}", status = status));
    }
    serde_json::from_str(&text).map_err(|e| format!("{e}: {text}"))
}

/// Opens TONE3000 in the browser to sign in (and pick a tone with `Prompt::Select`, which is
/// then downloaded under `library/TONE3000`). `open` shows the URL (in the browser).
pub fn start(library: PathBuf, prompt: Prompt, open: impl FnOnce(&str)) -> Result<Session, String> {
    let listener = TcpListener::bind(("127.0.0.1", PORT)).map_err(|e| {
        trf!(
            "cannot listen on port {port} for TONE3000's answer: {error}",
            port = PORT,
            error = e
        )
    })?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let verifier = base64url(&random(32)?);
    let challenge = base64url(&Sha256::digest(verifier.as_bytes()));
    let state = base64url(&random(16)?);
    let redirect = redirect_uri();
    let url = format!(
        "{API}/oauth/authorize?client_id={}&redirect_uri={}&response_type=code&code_challenge={}\
         &code_challenge_method=S256&state={}{}",
        percent(CLIENT_ID),
        percent(&redirect),
        percent(&challenge),
        percent(&state),
        if prompt == Prompt::Select {
            "&prompt=select_tone"
        } else {
            ""
        },
    );
    open(&url);
    let status = Arc::new(Mutex::new(Status::Waiting));
    let shared = Arc::clone(&status);
    std::thread::Builder::new()
        .name("tone3000".into())
        .spawn(move || {
            let result = run(&listener, &verifier, &state, &redirect, &library, &shared);
            if let Err(e) = result {
                set(&shared, Status::Failed(e));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Session { status })
}

/// Downloads a tone with the stored sign-in (or reuses an earlier download).
pub fn download(library: PathBuf, tone: i64, title: String) -> Session {
    let status = Arc::new(Mutex::new(Status::Downloading(trf!(
        "Opening {title}",
        title = title
    ))));
    let shared = Arc::clone(&status);
    let spawned = std::thread::Builder::new()
        .name("tone3000".into())
        .spawn(move || {
            let folder = library.join("TONE3000").join(file_name(&title, "model"));
            let files = std::fs::read_dir(&folder).map_or(0, |d| d.flatten().count());
            let result = if files > 0 {
                set(&shared, Status::Done { folder, files });
                Ok(())
            } else {
                let agent = agent();
                access_token(&agent).and_then(|access| {
                    fetch_models(
                        &agent,
                        &format!("Bearer {access}"),
                        tone,
                        &title,
                        &library,
                        &shared,
                    )
                })
            };
            if let Err(e) = result {
                set(&shared, Status::Failed(e));
            }
        });
    if let Err(e) = spawned {
        set(&status, Status::Failed(e.to_string()));
    }
    Session { status }
}

fn set(status: &Arc<Mutex<Status>>, value: Status) {
    if let Ok(mut s) = status.lock() {
        *s = value;
    }
}

/// Waits for the browser to come back with a code (and the chosen tone).
fn callback(listener: &TcpListener) -> Result<String, String> {
    let deadline = Instant::now() + WAIT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(query) = answer(stream)? {
                    return Ok(query);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    return Err(
                        tr("no answer from TONE3000 (the browser page was not completed)").into(),
                    );
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Reads one request; returns its query if it is the callback, after answering the browser.
fn answer(mut stream: TcpStream) -> Result<Option<String>, String> {
    stream.set_nonblocking(false).map_err(|e| e.to_string())?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let target = line.split_whitespace().nth(1).unwrap_or("");
    let Some(rest) = target.strip_prefix("/callback") else {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        return Ok(None);
    };
    let body = format!(
        "<html><body style=\"font-family:sans-serif;background:#1e2126;color:#dde;\
         text-align:center;padding-top:15%\"><h2>{}</h2><p>{}</p></body></html>",
        tr("Back to AudioStrada"),
        tr("You can close this page and go back to AudioStrada.")
    );
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    Ok(Some(rest.trim_start_matches('?').to_owned()))
}

fn run(
    listener: &TcpListener,
    verifier: &str,
    state: &str,
    redirect: &str,
    library: &std::path::Path,
    status: &Arc<Mutex<Status>>,
) -> Result<(), String> {
    let answer = callback(listener)?;
    if let Some(error) = query(&answer, "error") {
        return Err(match error.as_str() {
            "access_denied" => tr("the TONE3000 sign-in was cancelled").into(),
            other => trf!("TONE3000 refused: {reason}", reason = other),
        });
    }
    if query(&answer, "canceled").as_deref() == Some("true") {
        return Err(tr("the TONE3000 page was closed").into());
    }
    if query(&answer, "state").as_deref() != Some(state) {
        return Err(tr("TONE3000's answer did not match the request").into());
    }
    let code = query(&answer, "code").ok_or(tr("TONE3000 did not sign in"))?;
    set(status, Status::Downloading(tr("Signing in").into()));
    let agent = agent();
    let mut response = agent
        .post(format!("{API}/oauth/token"))
        .send_form([
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("code_verifier", verifier),
            ("redirect_uri", redirect),
            ("client_id", CLIENT_ID),
        ])
        .map_err(|e| e.to_string())?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let access = keep_tokens(&text)?;
    let Some(tone) = query(&answer, "tone_id").and_then(|t| t.parse::<i64>().ok()) else {
        set(status, Status::SignedIn);
        return Ok(());
    };
    let bearer = format!("Bearer {access}");
    let title = get_json(&agent, &bearer, &format!("{API}/tones/{tone}"))
        .ok()
        .and_then(|t| t.get("title").and_then(|v| v.as_str()).map(str::to_owned))
        .unwrap_or_else(|| format!("Tone {tone}"));
    fetch_models(&agent, &bearer, tone, &title, library, status)
}

/// Downloads the NAM models (and impulses) of a tone into `library/TONE3000/<title>`.
fn fetch_models(
    agent: &ureq::Agent,
    bearer: &str,
    tone: i64,
    title: &str,
    library: &std::path::Path,
    status: &Arc<Mutex<Status>>,
) -> Result<(), String> {
    set(
        status,
        Status::Downloading(trf!("Listing the models of {title}", title = title)),
    );
    // A2 models are only listed when asked for; the default lists A1 and custom ones.
    let mut models = Vec::new();
    for filter in ["", "&architecture=2"] {
        let list = get_json(
            agent,
            bearer,
            &format!("{API}/models?tone_id={tone}&page_size=100{filter}"),
        )?;
        if let Some(data) = list.get("data").and_then(|d| d.as_array()) {
            for m in data {
                let id = m.get("id").and_then(|v| v.as_i64());
                if !models
                    .iter()
                    .any(|(i, _, _): &(Option<i64>, _, _)| *i == id)
                {
                    let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("model");
                    let url = m.get("model_url").and_then(|v| v.as_str());
                    if let Some(url) = url {
                        models.push((id, name.to_owned(), url.to_owned()));
                    }
                }
            }
        }
    }
    if models.is_empty() {
        return Err(trf!(
            "{title} has no model AudioStrada can download",
            title = title
        ));
    }
    // Downloaded aside first, so an interrupted download is never taken for a complete one.
    let folder = library.join("TONE3000").join(file_name(title, "model"));
    let part = library
        .join("TONE3000")
        .join(format!(".{}.part", file_name(title, "model")));
    let _ = std::fs::remove_dir_all(&part);
    std::fs::create_dir_all(&part).map_err(|e| e.to_string())?;
    let count = models.len();
    for (i, (_, name, url)) in models.into_iter().enumerate() {
        set(
            status,
            Status::Downloading(trf!(
                "Downloading {title}: {n} of {count}",
                title = title,
                n = i + 1,
                count = count
            )),
        );
        let mut response = agent
            .get(&url)
            .header("Authorization", bearer)
            .call()
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(trf!(
                "cannot download {name}: {status}",
                name = name,
                status = response.status()
            ));
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(200 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        let ext = if url.split('?').next().unwrap_or("").ends_with(".wav") {
            "wav"
        } else {
            "nam"
        };
        let mut path = part.join(format!("{}.{ext}", file_name(&name, "model")));
        if path.exists() {
            path = part.join(format!("{} ({}).{ext}", file_name(&name, "model"), i + 1));
        }
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    for file in std::fs::read_dir(&part)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        std::fs::rename(file.path(), folder.join(file.file_name())).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir(&part);
    set(
        status,
        Status::Done {
            folder,
            files: count,
        },
    );
    Ok(())
}

/// The lists of tones the free API offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum List {
    Trending,
    Latest,
    Favorites,
    Downloaded,
}

impl List {
    pub const ALL: [List; 4] = [
        List::Trending,
        List::Latest,
        List::Favorites,
        List::Downloaded,
    ];

    pub fn name(self) -> &'static str {
        match self {
            List::Trending => tr("Trending"),
            List::Latest => tr("Latest"),
            List::Favorites => tr("My favorites"),
            List::Downloaded => tr("My downloads"),
        }
    }

    /// The user's own lists come page by page; trending and latest are ten tones.
    pub fn paged(self) -> bool {
        matches!(self, List::Favorites | List::Downloaded)
    }

    fn path(self) -> &'static str {
        match self {
            List::Trending => "tones/trending",
            List::Latest => "tones/latest",
            List::Favorites => "tones/favorited",
            List::Downloaded => "tones/downloaded",
        }
    }
}

/// Tones of one request, and whether more pages follow.
pub type Page = (Vec<Tone>, bool);

/// A tone of a list, with what its card must show.
#[derive(Clone, Debug, PartialEq)]
pub struct Tone {
    pub id: i64,
    pub title: String,
    pub gear: String,
    pub format: String,
    pub license: String,
    pub creator: String,
    pub avatar: Option<String>,
    pub image: Option<String>,
    pub models: u64,
    pub downloads: u64,
    pub url: Option<String>,
}

impl Tone {
    fn parse(t: &serde_json::Value) -> Option<Tone> {
        let text = |v: Option<&serde_json::Value>| v.and_then(|v| v.as_str()).map(str::to_owned);
        let user = t.get("user");
        Some(Tone {
            id: t.get("id")?.as_i64()?,
            title: text(t.get("title")).unwrap_or_default(),
            gear: text(t.get("gear")).unwrap_or_default(),
            format: text(t.get("format")).unwrap_or_else(|| "nam".into()),
            license: text(t.get("license")).unwrap_or_default(),
            creator: text(user.and_then(|u| u.get("display_name")))
                .or_else(|| text(user.and_then(|u| u.get("username"))))
                .unwrap_or_default(),
            avatar: text(user.and_then(|u| u.get("avatar_url"))),
            image: t
                .get("images")
                .and_then(|i| i.as_array())
                .and_then(|i| text(i.first())),
            models: t.get("models_count").and_then(|v| v.as_u64()).unwrap_or(0),
            downloads: t
                .get("downloads_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            url: text(t.get("url")),
        })
    }
}

/// The signed-in user, shown with the lists.
#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub name: String,
    pub avatar: Option<String>,
}

/// A request answered in the background.
pub type Pending<T> = Arc<Mutex<Option<Result<T, String>>>>;

fn spawn<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Pending<T> {
    let result: Pending<T> = Arc::new(Mutex::new(None));
    let shared = Arc::clone(&result);
    let _ = std::thread::Builder::new()
        .name("tone3000".into())
        .spawn(move || {
            let value = work();
            if let Ok(mut r) = shared.lock() {
                *r = Some(value);
            }
        });
    result
}

/// Fetches a list of tones (only the given gear for the trending list).
/// Fetches a page of a list (`page` from 1), for one gear type when given (trending and the
/// user's lists filter by gear; latest does not).
pub fn list(which: List, gear: Option<&'static str>, page: u32) -> Pending<Page> {
    spawn(move || {
        let agent = agent();
        let bearer = format!("Bearer {}", access_token(&agent)?);
        let mut query = Vec::new();
        if which.paged() {
            query.push(format!("page={page}&page_size=50"));
        }
        if let (Some(gear), false) = (gear, which == List::Latest) {
            query.push(format!("gear={gear}"));
        }
        let mut url = format!("{API}/{}", which.path());
        if !query.is_empty() {
            url = format!("{url}?{}", query.join("&"));
        }
        let value = get_json(&agent, &bearer, &url)?;
        let tones = value
            .get("data")
            .and_then(|d| d.as_array())
            .map(|d| d.iter().filter_map(Tone::parse).collect())
            .unwrap_or_default();
        let pages = value
            .get("total_pages")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);
        Ok((tones, which.paged() && u64::from(page) < pages))
    })
}

pub fn user() -> Pending<User> {
    spawn(|| {
        let agent = agent();
        let bearer = format!("Bearer {}", access_token(&agent)?);
        let u = get_json(&agent, &bearer, &format!("{API}/user"))?;
        let text = |k: &str| u.get(k).and_then(|v| v.as_str()).map(str::to_owned);
        Ok(User {
            name: text("display_name")
                .or_else(|| text("username"))
                .unwrap_or_default(),
            avatar: text("avatar_url"),
        })
    })
}

/// Downloads an image (a tone's photo, an avatar) as RGBA, at most `size` pixels wide or high.
pub fn image(url: String, size: u32) -> Pending<([usize; 2], Vec<u8>)> {
    spawn(move || {
        let mut response = agent().get(&url).call().map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("{}", response.status()));
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(20 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        let picture = image::load_from_memory(&bytes)
            .map_err(|e| e.to_string())?
            .thumbnail(size, size)
            .to_rgba8();
        let dims = [picture.width() as usize, picture.height() as usize];
        Ok((dims, picture.into_raw()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_and_query_helpers() {
        // RFC 7636 appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            base64url(&Sha256::digest(verifier.as_bytes())),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(percent("a b/c"), "a%20b%2Fc");
        let q = "code=ab%2Fc&state=x+y&tone_id=42";
        assert_eq!(query(q, "code").as_deref(), Some("ab/c"));
        assert_eq!(query(q, "state").as_deref(), Some("x y"));
        assert_eq!(query(q, "tone_id").as_deref(), Some("42"));
        assert_eq!(query(q, "error"), None);
    }

    #[test]
    fn tones_from_the_api() {
        let t: serde_json::Value = serde_json::from_str(
            r#"{"id": 7, "title": "Plexi Crunch", "gear": "amp-cab", "format": "nam",
                "license": "cc-by", "images": ["https://img/x.jpg"], "models_count": 4,
                "user": {"username": "jo", "display_name": null, "avatar_url": null}}"#,
        )
        .unwrap();
        let tone = Tone::parse(&t).unwrap();
        assert_eq!((tone.id, tone.creator.as_str(), tone.models), (7, "jo", 4));
        assert_eq!(tone.image.as_deref(), Some("https://img/x.jpg"));
        assert_eq!(tone.gear, "amp-cab");
    }
}
