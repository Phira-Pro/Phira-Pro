//! Http client for Phira API.

mod model;
pub use model::*;

use std::{borrow::Cow, collections::HashMap, fmt, marker::PhantomData, sync::Arc};

use crate::{get_data, get_data_mut, save_data};
use anyhow::{anyhow, bail, Context, Result};
use arc_swap::ArcSwap;
use once_cell::sync::Lazy;
use prpr::scene::SimpleRecord;
use prpr_l10n::LANG_IDENTS;
use reqwest::{header, ClientBuilder, Method, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::debug;

pub static CLIENT_TOKEN: Lazy<ArcSwap<Option<String>>> = Lazy::new(|| ArcSwap::from_pointee(None));

static CLIENT: Lazy<ArcSwap<reqwest::Client>> = Lazy::new(|| ArcSwap::from_pointee(basic_client_builder().build().unwrap()));
// Pro requests add authentication per request and reuse connections.
static PRO_CLIENT: Lazy<ArcSwap<reqwest::Client>> = Lazy::new(|| ArcSwap::from_pointee(basic_client_builder().build().unwrap()));
static SCORE_CLIENT: Lazy<ArcSwap<reqwest::Client>> =
    Lazy::new(|| ArcSwap::from_pointee(basic_client_builder().redirect(score_redirect_policy()).build().unwrap()));

fn score_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::none()
}

pub struct Client;

/// 官方 API 地址：`config.api_url` 为空时的回退值。
pub const DEFAULT_API_URL: &str = "https://phira.5wyxi.com";
/// 官方 Web 前端地址：`config.web_url` 为空时的回退值。
pub const DEFAULT_WEB_URL: &str = "https://phira.moe";
/// 官方服务器状态页：`config.status_url` 为空时的回退值。
pub const DEFAULT_STATUS_URL: &str = "https://status.phira.cn";

/// 读取一个「可配置基础地址」：取配置值（去首尾空白与末尾 `/`），为空则回退到 `default`。
fn base_url(value: &str, default: &str) -> String {
    let url = value.trim().trim_end_matches('/');
    if url.is_empty() {
        default.to_owned()
    } else {
        url.to_owned()
    }
}

/// 当前使用的 Phira API 基础地址（登录、谱面等只读数据；成绩只上传到 Pro）。
/// 自建 / 私服改成 `config.api_url` 即可；改完需要重启生效。
pub fn api_url() -> String {
    base_url(&get_data().config.api_url, DEFAULT_API_URL)
}

/// Phira Pro：自服（`phira-pro-api`）基础地址；**留空 = 关闭全部自服功能**。
pub fn pro_api_url() -> String {
    get_data().config.pro_api_url.trim().trim_end_matches('/').to_owned()
}

/// 构造一个打到自服的请求（带官服 token 鉴权）。自服未配置时返回 `None`。
///
/// 复用独立连接池，并为每个请求附加当前 token，避免重复鉴权头。
pub fn pro_request(method: Method, path: impl AsRef<str>) -> Option<RequestBuilder> {
    let base = pro_api_url();
    if base.is_empty() {
        return None;
    }
    if method == Method::POST && is_score_upload(path.as_ref()) {
        return Some(pro_auth(SCORE_CLIENT.load().post(score_upload_url(&base).ok()?)));
    }
    let req = PRO_CLIENT.load().request(method, base + path.as_ref());
    Some(pro_auth(req))
}

fn pro_auth(mut req: RequestBuilder) -> RequestBuilder {
    if let Ok(locale) = header::HeaderValue::from_str(&client_locale()) {
        req = req.header(header::ACCEPT_LANGUAGE, locale);
    }
    if let Some(token) = CLIENT_TOKEN.load().as_ref() {
        if let Ok(value) = header::HeaderValue::from_str(&format!("Bearer {token}")) {
            req = req.header(header::AUTHORIZATION, value);
        }
    }
    req
}

fn score_upload_url(base: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(base.trim()).context("未配置有效的 Phira Pro 成绩服务器")?;
    let host = url.host_str().unwrap_or_default().trim_end_matches('.');
    if !matches!(url.scheme(), "http" | "https") || host.is_empty() || !url.username().is_empty() || url.password().is_some() {
        bail!("无效的 Phira Pro 成绩服务器地址");
    }
    for domain in ["5wyxi.com", "phira.cn", "phira.moe"] {
        if host == domain || host.ends_with(&format!(".{domain}")) {
            bail!("Phira Pro 已禁止向官服上传成绩，请配置自建服务器");
        }
    }
    let path = format!("{}/play/upload", url.path().trim_end_matches('/'));
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn is_score_upload(path: &str) -> bool {
    let path = path.split(['?', '#']).next().unwrap_or_default().trim_end_matches('/');
    path == "/play/upload" || reqwest::Url::parse(path).is_ok_and(|url| url.path().trim_end_matches('/').ends_with("/play/upload"))
}

/// Scores have a dedicated destination and never follow HTTP redirects.
/// An unavailable Pro server is an upload error, with no official-server fallback.
pub fn pro_score_upload<T: Serialize>(data: &T) -> Result<RequestBuilder> {
    let url = score_upload_url(&pro_api_url())?;
    Ok(pro_auth(SCORE_CLIENT.load().post(url)).json(data))
}

pub fn pro_get(path: impl AsRef<str>) -> Option<RequestBuilder> {
    pro_request(Method::GET, path)
}

/// 当前使用的 Phira 网页前端地址（谱面页 / 用户页 / 合集页 / 条款链接等）。
pub fn web_url() -> String {
    base_url(&get_data().config.web_url, DEFAULT_WEB_URL)
}

/// 当前使用的服务器状态页地址。
pub fn status_url() -> String {
    base_url(&get_data().config.status_url, DEFAULT_STATUS_URL)
}

pub fn basic_client_builder() -> ClientBuilder {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if let Some(_cid) = attempt.url().as_str().strip_prefix("anys://") {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    let mut builder = reqwest::ClientBuilder::new().redirect(policy);
    if get_data().accept_invalid_cert {
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder
}

fn client_locale() -> String {
    get_data().language.clone().unwrap_or(LANG_IDENTS[0].to_string())
}

fn build_client(access_token: Option<&str>) -> Result<Arc<reqwest::Client>> {
    CLIENT_TOKEN.store(access_token.map(str::to_owned).into());
    let mut headers = header::HeaderMap::new();
    headers.append(header::ACCEPT_LANGUAGE, header::HeaderValue::from_str(&client_locale())?);
    if let Some(token) = access_token {
        let mut auth_value = header::HeaderValue::from_str(&format!("Bearer {token}"))?;
        auth_value.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_value);
    }
    Ok(basic_client_builder().default_headers(headers).build()?.into())
}

pub fn set_access_token_sync(access_token: Option<&str>) -> Result<()> {
    CLIENT.store(build_client(access_token)?);
    PRO_CLIENT.store(Arc::new(basic_client_builder().build()?));
    SCORE_CLIENT.store(Arc::new(basic_client_builder().redirect(score_redirect_policy()).build()?));
    Ok(())
}

async fn set_access_token(access_token: &str) -> Result<()> {
    CLIENT.store(build_client(Some(access_token))?);
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ErrorCode(Cow<'static, str>);

macro_rules! error_code {
    ($($name:ident => $status:expr),* $(,)?) => {
        $(
            pub const $name: ErrorCode = ErrorCode(Cow::Borrowed(stringify!($name)));
        )*
    };
}

#[allow(dead_code)]
impl ErrorCode {
    error_code! {
        INVALID_INPUT => StatusCode::BAD_REQUEST,
        UNAUTHENTICATED => StatusCode::UNAUTHORIZED,
        EXPIRED => StatusCode::UNAUTHORIZED,
        PERMISSION_DENIED => StatusCode::FORBIDDEN,
        USER_BANNED => StatusCode::FORBIDDEN,
        PENDING_DELETE_REQUEST => StatusCode::FORBIDDEN,
        RATE_LIMITED => StatusCode::TOO_MANY_REQUESTS,
        NOT_FOUND => StatusCode::NOT_FOUND,
        CONFLICT => StatusCode::CONFLICT,
        NOT_MODIFIED => StatusCode::NOT_MODIFIED,
        NOT_IMPLEMENTED => StatusCode::NOT_IMPLEMENTED,
        STORAGE_UNAVAILABLE => StatusCode::SERVICE_UNAVAILABLE,
        INTERNAL_SERVER_ERROR => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ErrorCode({})", self.0)
    }
}

impl std::error::Error for ErrorCode {}

pub async fn recv_raw(request: RequestBuilder) -> Result<Response> {
    let response = request.send().await?;
    if !response.status().is_success() {
        let status = response.status().as_str().to_owned();
        let text = response.text().await.context("failed to receive text")?;
        if let Ok(what) = serde_json::from_str::<serde_json::Value>(&text) {
            let detail = what.get("error").and_then(|it| it.as_str()).unwrap_or("unknown error");
            let mut err = anyhow!("request failed (HTTP {status}): {detail}");
            if let Some(code) = what.get("code").and_then(|it| it.as_str()) {
                err = err.context(ErrorCode(Cow::Owned(code.to_owned())));
            }
            return Err(err);
        }
        bail!("request failed ({status}): {text}");
    }
    Ok(response)
}

#[derive(Serialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum LoginParams<'a> {
    Password {
        email: &'a str,
        password: &'a str,
        cancel_delete_request: bool,
    },
    RefreshToken {
        #[serde(rename = "refreshToken")]
        token: &'a str,
        cancel_delete_request: bool,
    },
}

/// A freshly minted token pair returned by every login endpoint.
#[cfg(feature = "hykb")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginResp {
    id: i32,
    token: String,
    refresh_token: String,
}

/// Response of `POST /login/hykb`: either an immediate login or a pending choice.
#[cfg(feature = "hykb")]
#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum HykbLoginResp {
    Ok {
        #[serde(flatten)]
        login: LoginResp,
    },
    NeedChoice {
        hykb_token: String,
    },
}

/// Outcome of a HYKB login attempt surfaced to the UI.
#[cfg(feature = "hykb")]
pub enum HykbLoginOutcome {
    /// The HYKB account was already bound; the user is now logged in.
    LoggedIn,
    /// First time we see this HYKB account; the user must register or claim.
    NeedChoice { hykb_token: String },
}

impl Client {
    #[inline]
    pub fn get(path: impl AsRef<str>) -> RequestBuilder {
        Self::request(Method::GET, path)
    }

    #[inline]
    pub fn post<T: Serialize>(path: impl AsRef<str>, data: &T) -> RequestBuilder {
        Self::request(Method::POST, path).json(data)
    }

    #[inline]
    pub fn delete(path: impl AsRef<str>) -> RequestBuilder {
        Self::request(Method::DELETE, path)
    }

    pub fn request(method: Method, path: impl AsRef<str>) -> RequestBuilder {
        if method == Method::POST && is_score_upload(path.as_ref()) {
            // Also guard generic/legacy callers. An invalid configuration
            // creates a failed builder, never an official request.
            return match score_upload_url(&pro_api_url()) {
                Ok(url) => pro_auth(SCORE_CLIENT.load().post(url)),
                Err(_) => SCORE_CLIENT.load().post("Pro score server is not configured"),
            };
        }
        CLIENT.load().request(method, api_url() + path.as_ref())
    }

    pub fn clear_cache<T: Object + 'static>(id: i32) -> Result<bool> {
        let map = obtain_map_cache::<T>();
        let mut guard = map.lock().unwrap();
        let Some(actual_map) = guard.downcast_mut::<ObjectMap<T>>() else {
            unreachable!()
        };
        Ok(actual_map.pop(&id).is_some())
    }

    pub async fn load<T: Object + 'static>(id: i32) -> Result<Arc<T>> {
        {
            let map = obtain_map_cache::<T>();
            let mut guard = map.lock().unwrap();
            let Some(actual_map) = guard.downcast_mut::<ObjectMap<T>>() else {
                unreachable!()
            };
            if let Some(value) = actual_map.get(&id) {
                return Ok(Arc::clone(value));
            }
            drop(guard);
            drop(map);
        }
        Self::fetch(id).await
    }

    pub async fn fetch<T: Object + 'static>(id: i32) -> Result<Arc<T>> {
        Self::fetch_opt(id).await?.ok_or_else(|| anyhow!("entry not found"))
    }

    pub async fn fetch_opt<T: Object + 'static>(id: i32) -> Result<Option<Arc<T>>> {
        let value = Client::fetch_inner::<T>(id).await?;
        let Some(value) = value else { return Ok(None) };
        let value = Arc::new(value);
        let map = obtain_map_cache::<T>();
        let mut guard = map.lock().unwrap();
        let Some(actual_map) = guard.downcast_mut::<ObjectMap<T>>() else {
            unreachable!()
        };
        actual_map.put(id, Arc::clone(&value));
        Ok(Some(value))
    }

    async fn fetch_inner<T: Object>(id: i32) -> Result<Option<T>> {
        let resp = Self::get(format!("/{}/{id}", T::QUERY_PATH)).send().await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_str().to_owned();
            let text = resp.text().await.context("failed to receive text")?;
            if let Ok(what) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(detail) = what["error"].as_str() {
                    bail!("request failed ({status}): {detail}");
                }
            }
            bail!("request failed ({status}): {text}");
        }
        Ok(Some(resp.json().await?))
    }

    pub fn query<T: Object>() -> QueryBuilder<T> {
        QueryBuilder {
            queries: HashMap::new(),
            page: None,
            suffix: "",
            _phantom: PhantomData,
        }
    }

    pub async fn register(email: &str, username: &str, password: &str) -> Result<()> {
        recv_raw(Self::post(
            "/register",
            &json!({
                "email": email,
                "name": username,
                "password": password,
            }),
        ))
        .await?;
        Ok(())
    }

    pub async fn login(params: LoginParams<'_>) -> Result<()> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct FullLoginParams<'a> {
            #[serde(flatten)]
            inner: LoginParams<'a>,
            #[serde(rename = "clientVersion")]
            client_version: &'static str,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Resp {
            id: i32,
            token: String,
            refresh_token: String,
        }
        let resp: Resp = recv_raw(Self::post(
            "/login",
            &FullLoginParams {
                inner: params,
                client_version: env!("CARGO_PKG_VERSION"),
            },
        ))
        .await?
        .json()
        .await?;

        Self::store_login(resp.id, resp.token, resp.refresh_token).await?;
        Ok(())
    }

    /// Persist a freshly minted token pair and wire it into the HTTP client.
    /// Shared by every login entry point (password, refresh, HYKB). `_id` is
    /// kept in the signature so callers can pass the account id even though it
    /// is no longer needed here (the native anti-addiction bridge that used it
    /// is gone).
    async fn store_login(_id: i32, token: String, refresh_token: String) -> Result<()> {
        set_access_token(&token).await?;
        get_data_mut().tokens = Some((token, refresh_token));
        save_data()?;
        Ok(())
    }

    #[cfg(feature = "hykb")]
    /// Entry point for HYKB (好游快爆) login. The verified `(uid, access_token)`
    /// come from the native SDK. Either logs the user straight in (account already
    /// bound) or returns a short-lived `hykb_token` for the register/claim step.
    pub async fn login_hykb(uid: i64, access_token: &str) -> Result<HykbLoginOutcome> {
        let resp: HykbLoginResp = recv_raw(Self::post(
            "/login/hykb",
            &json!({
                "hykbUid": uid,
                "accessToken": access_token,
            }),
        ))
        .await?
        .json()
        .await?;
        match resp {
            HykbLoginResp::Ok { login } => {
                Self::store_login(login.id, login.token, login.refresh_token).await?;
                Ok(HykbLoginOutcome::LoggedIn)
            }
            HykbLoginResp::NeedChoice { hykb_token } => Ok(HykbLoginOutcome::NeedChoice { hykb_token }),
        }
    }

    #[cfg(feature = "hykb")]
    /// New player: create a fresh Phira account bound to the pending HYKB identity,
    /// using the username chosen by the player.
    pub async fn login_hykb_register(hykb_token: &str, username: &str) -> Result<()> {
        let resp: LoginResp = recv_raw(Self::post(
            "/login/hykb/register",
            &json!({
                "hykbToken": hykb_token,
                "nick": username,
            }),
        ))
        .await?
        .json()
        .await?;
        Self::store_login(resp.id, resp.token, resp.refresh_token).await?;
        Ok(())
    }

    #[cfg(feature = "hykb")]
    /// Legacy migration: bind the pending HYKB identity to an existing email account
    /// after verifying its email + password.
    pub async fn login_hykb_claim(hykb_token: &str, email: &str, password: &str) -> Result<()> {
        let resp: LoginResp = recv_raw(Self::post(
            "/login/hykb/claim",
            &json!({
                "hykbToken": hykb_token,
                "email": email,
                "password": password,
            }),
        ))
        .await?
        .json()
        .await?;
        Self::store_login(resp.id, resp.token, resp.refresh_token).await?;
        Ok(())
    }

    #[cfg(feature = "hykb")]
    /// Bind a HYKB account to the currently logged-in account.
    pub async fn bind_hykb(uid: i64, access_token: &str) -> Result<()> {
        recv_raw(Self::post(
            "/me/bind-hykb",
            &json!({
                "hykbUid": uid,
                "accessToken": access_token,
            }),
        ))
        .await?;
        Ok(())
    }

    #[cfg(feature = "hykb")]
    /// Unbind the HYKB account from the current account.
    pub async fn unbind_hykb() -> Result<()> {
        recv_raw(Self::post("/me/unbind-hykb", &())).await?;
        Ok(())
    }

    #[cfg(feature = "hykb")]
    /// Request transferring the current HYKB-only account onto an existing email
    /// account. Sends a confirmation email to `email`; the move happens only once
    /// the user clicks the link. Returns Ok even when the email is unregistered
    /// (the server intentionally does not reveal whether it exists).
    pub async fn transfer_request(email: &str) -> Result<()> {
        recv_raw(Self::post("/me/transfer-request", &json!({ "email": email }))).await?;
        Ok(())
    }

    pub async fn get_me() -> Result<User> {
        // Accounts not bound to a HYKB account are valid: anti-addiction is
        // covered by a native HYKB login performed at sign-in (used for the
        // SDK's enforcement, not bound to the account), and the player may
        // bind HYKB later from the profile page.
        Ok(recv_raw(Self::get("/me")).await?.json().await?)
    }

    pub async fn best_record(id: i32) -> Result<SimpleRecord> {
        Ok(recv_raw(Self::get(format!("/record/best/{id}"))).await?.json().await?)
    }

    pub async fn upload_file(name: &str, bytes: Vec<u8>) -> Result<String> {
        #[derive(Deserialize)]
        struct Resp {
            id: String,
        }
        let resp: Resp = recv_raw(Self::request(Method::POST, format!("/upload/{name}")).body(bytes))
            .await?
            .json()
            .await?;
        Ok(resp.id)
    }

    /// Returns `Some(modified)` (the `Last-Modified` header) if the terms have
    /// been updated since `modified`, or `None` if unchanged. Uses HEAD so the
    /// ~9 KB body is never downloaded — change detection relies solely on the
    /// `Last-Modified` header.
    pub async fn fetch_terms(modified: Option<&str>) -> Result<Option<String>> {
        let mut req = CLIENT.load().head(format!("{}/terms/{}.txt", api_url(), client_locale()));
        if let Some(modified) = modified {
            req = req.header(header::IF_MODIFIED_SINCE, header::HeaderValue::from_str(modified)?);
        }
        let resp = req.send().await?;
        if resp.status() == StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        if !resp.status().is_success() {
            bail!("failed to fetch terms: {:?}", resp.status());
        }
        let new_modified = resp
            .headers()
            .get(header::LAST_MODIFIED)
            .and_then(|it| it.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("invalid last-modified header"))?;
        debug!("{new_modified} {modified:?}");
        if Some(new_modified.as_str()) == modified {
            // That mother fucker qiniu does not return NOT_MODIFIED
            return Ok(None);
        }
        Ok(Some(new_modified))
    }
}

#[cfg(test)]
mod score_upload_tests {
    use super::*;

    #[test]
    fn scores_reject_official_hosts_and_invalid_destinations() {
        for base in [
            "",
            "https://phira.5wyxi.com",
            "https://PHIRA.5WYXI.COM.",
            "https://api.phira.cn:443",
            "https://phira.moe.",
            "https://sub.phira.moe",
            "ftp://api.phira.pro",
            "https://user:pass@api.phira.pro",
        ] {
            assert!(score_upload_url(base).is_err(), "{base}");
        }
    }

    #[test]
    fn scores_use_only_the_configured_pro_origin() {
        assert_eq!(score_upload_url("https://api.phira.pro/").unwrap().as_str(), "https://api.phira.pro/play/upload");
        assert_eq!(score_upload_url(" http://127.0.0.1:8080/api/?old=1#x ").unwrap().as_str(), "http://127.0.0.1:8080/api/play/upload");
        assert!(is_score_upload("/play/upload/?retry=1"));
        assert!(is_score_upload("https://phira.5wyxi.com/play/upload"));
        assert!(!is_score_upload("/record/query/10"));
    }

    #[tokio::test]
    async fn score_upload_does_not_forward_its_body_on_redirect() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::Duration,
        };
        let source = TcpListener::bind("127.0.0.1:0").unwrap();
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let url = format!("http://{}/play/upload", source.local_addr().unwrap());
        let location = format!("http://{}/play/upload", target.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut connection, _) = source.accept().unwrap();
            connection.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut bytes = [0; 4096];
            let n = connection.read(&mut bytes).unwrap();
            assert!(String::from_utf8_lossy(&bytes[..n]).starts_with("POST /play/upload "));
            write!(connection, "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let response = reqwest::Client::builder()
            .redirect(score_redirect_policy())
            .build()
            .unwrap()
            .post(score_upload_url(url.trim_end_matches("/play/upload")).unwrap())
            .body("score-token")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
        server.join().unwrap();
        assert_eq!(target.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    }
}

#[must_use]
pub struct QueryBuilder<T> {
    queries: HashMap<Cow<'static, str>, Cow<'static, str>>,
    page: Option<u64>,
    suffix: &'static str,
    _phantom: PhantomData<T>,
}

impl<T: Object> QueryBuilder<T> {
    pub fn query(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<Cow<'static, str>>) -> Self {
        self.queries.insert(key.into(), value.into());
        self
    }

    #[inline]
    pub fn order(self, order: impl Into<Cow<'static, str>>) -> Self {
        self.query("order", order)
    }

    #[inline]
    pub fn tags(self, tags: impl Into<Cow<'static, str>>) -> Self {
        self.query("tags", tags)
    }

    #[inline]
    pub fn search(self, search: impl Into<Cow<'static, str>>) -> Self {
        self.query("search", search)
    }

    #[inline]
    pub fn page_num(self, page_num: u64) -> Self {
        self.query("pageNum", page_num.to_string())
    }

    #[inline]
    pub fn suffix(mut self, suffix: &'static str) -> Self {
        self.suffix = suffix;
        self
    }

    pub fn page(mut self, page: u64) -> Self {
        self.page = Some(page);
        self
    }

    pub async fn send(mut self) -> Result<(Vec<T>, u64)> {
        self.queries.insert("page".into(), (self.page.unwrap_or(0) + 1).to_string().into());
        #[derive(Deserialize)]
        struct PagedResult<T> {
            count: u64,
            results: Vec<T>,
        }
        let res: PagedResult<T> = recv_raw(Client::get(format!("/{}{}", T::QUERY_PATH, self.suffix)).query(&self.queries))
            .await?
            .json()
            .await?;
        Ok((res.results, res.count))
    }
}
