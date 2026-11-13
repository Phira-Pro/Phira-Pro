mod chart;
pub use chart::*;

mod collection;
pub use collection::*;

mod event;
pub use event::*;

mod message;
pub use message::*;

mod record;
pub use record::*;

mod user;
pub use user::*;

use super::{basic_client_builder, api_url, send_raw, Client, CLIENT_TOKEN};
use crate::{
    dir, get_data,
    images::{THUMBNAIL_HEIGHT, THUMBNAIL_WIDTH},
};
use anyhow::{Context, Result};
use bytes::Bytes;
use image::DynamicImage;
use lru::LruCache;
use once_cell::sync::Lazy;
use reqwest::{Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize, Serializer};
use std::{
    any::Any,
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use tracing::debug;

pub(crate) type ObjectMap<T> = LruCache<i32, Arc<T>>;
type CacheMap = HashMap<&'static str, Arc<Mutex<Box<dyn Any + Send + Sync>>>>;
static CACHES: Lazy<Mutex<CacheMap>> = Lazy::new(Mutex::default);

pub(crate) fn obtain_map_cache<T: Object + 'static>() -> Arc<Mutex<Box<dyn Any + Send + Sync>>> {
    let mut caches = CACHES.lock().unwrap();
    Arc::clone(
        caches
            .entry(T::QUERY_PATH)
            .or_insert_with(|| Arc::new(Mutex::new(Box::new(ObjectMap::<T>::new(64.try_into().unwrap()))))),
    )
}

pub trait Object: Clone + DeserializeOwned + Send + Sync {
    const QUERY_PATH: &'static str;

    fn id(&self) -> i32;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "String")]
#[serde(into = "String")]
pub struct MusicPosition {
    pub seconds: u32,
}
impl TryFrom<String> for MusicPosition {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let seconds = || -> Option<u32> {
            let mut it = value.splitn(3, ':');
            let mut res = it.next()?.parse::<u32>().ok()?;
            res = res * 60 + it.next()?.parse::<u32>().ok()?;
            res = res * 60 + it.next()?.parse::<u32>().ok()?;
            Some(res)
        }()
        .ok_or("illegal position")?;
        Ok(MusicPosition { seconds })
    }
}
impl From<MusicPosition> for String {
    fn from(value: MusicPosition) -> Self {
        format!("00:00:{:02}", value.seconds)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "u8")]
#[repr(u8)]
#[allow(dead_code)]
pub enum LevelType {
    EZ = 0,
    HD,
    IN,
    AT,
    SP,
}
impl TryFrom<u8> for LevelType {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use LevelType::*;
        Ok(match value {
            0 => EZ,
            1 => HD,
            2 => IN,
            3 => AT,
            4 => SP,
            x => {
                return Err(format!("illegal level type: {x}"));
            }
        })
    }
}

#[derive(Debug)]
pub struct Ptr<T> {
    pub id: i32,
    _marker: PhantomData<T>,
}
impl<T: Object + 'static> Clone for Ptr<T> {
    fn clone(&self) -> Self {
        Self::new(self.id)
    }
}
impl<T: Object + 'static> From<i32> for Ptr<T> {
    fn from(value: i32) -> Self {
        Self::new(value)
    }
}

impl<T: Object + 'static> Ptr<T> {
    pub fn new(id: i32) -> Self {
        Self { id, _marker: PhantomData }
    }

    #[inline]
    pub async fn fetch(&self) -> Result<Arc<T>> {
        Client::fetch(self.id).await
    }

    #[inline]
    pub async fn fetch_opt(&self) -> Result<Option<Arc<T>>> {
        Client::fetch_opt(self.id).await
    }

    pub async fn load(&self) -> Result<Arc<T>> {
        // sync locks can not be held accross await point
        {
            let map = obtain_map_cache::<T>();
            let mut guard = map.lock().unwrap();
            let Some(actual_map) = guard.downcast_mut::<ObjectMap<T>>() else {
                unreachable!()
            };
            if let Some(value) = actual_map.get(&self.id) {
                return Ok(Arc::clone(value));
            }
            drop(guard);
            drop(map);
        }
        self.fetch().await
    }
}
impl<T: Object + 'static> Serialize for Ptr<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i32(self.id)
    }
}
impl<'de, T: Object + 'static> Deserialize<'de> for Ptr<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        i32::deserialize(deserializer).map(Self::new)
    }
}

pub static CACHE_DIR: Lazy<String> = Lazy::new(|| format!("{}/http-cache", dir::cache().unwrap_or_else(|_| ".".to_owned())));

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct File {
    pub url: String,
}

#[derive(Debug)]
pub(crate) struct MissingFile;

impl std::fmt::Display for MissingFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("remote file not found")
    }
}

impl std::error::Error for MissingFile {}

fn missing_file(status: StatusCode, body: &str) -> bool {
    status == StatusCode::NOT_FOUND
        || serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .is_some_and(|json| json["code"].as_str() == Some("NOT_FOUND"))
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn recognizes_http_and_api_not_found_without_masking_other_failures() {
        assert!(missing_file(StatusCode::NOT_FOUND, "missing"));
        assert!(missing_file(StatusCode::BAD_REQUEST, r#"{"code":"NOT_FOUND"}"#));
        assert!(!missing_file(StatusCode::FORBIDDEN, r#"{"code":"FORBIDDEN"}"#));
        assert!(!missing_file(StatusCode::BAD_GATEWAY, "NOT_FOUND"));
    }
}

impl File {
    pub(crate) fn request_url(url: &str) -> reqwest::RequestBuilder {
        let req = basic_client_builder().build().unwrap().get(url);
        if let Some(token) = CLIENT_TOKEN.load().as_ref() {
            req.header("Authorization", format!("Bearer {token}"))
        } else {
            req
        }
    }

    async fn fetch_remote(url: &str) -> Result<Bytes> {
        let mut response: Response = send_raw(Self::request_url(url)).await?;
        if response.status().is_redirection() {
            let location = response.headers().get(reqwest::header::LOCATION)
                .context("missing file redirect location")?.to_str()?;
            let cid = location.strip_prefix("anys://").context("invalid file redirect")?;
            let gateway_url = format!("{}/{}", get_data().anys_gateway.trim_end_matches('/'), cid);
            debug!("fetching redirected file from ANYS");
            response = send_raw(Self::request_url(&gateway_url)).await?;
        }
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await?;
            let error = anyhow::anyhow!("file request failed (HTTP {status}): {body}");
            return Err(if missing_file(status, &body) { error.context(MissingFile) } else { error });
        }
        Ok(response.bytes().await?)
    }

    pub async fn fetch(&self) -> Result<Bytes> {
        match cacache::read(&*CACHE_DIR, &self.url).await {
            Ok(data) => Ok(data.into()),
            Err(cacache::Error::EntryNotFound(..)) => {
                let base = api_url();
                let alternate = get_data().enable_anys.then(|| self.url.strip_prefix(&format!("{base}/files/")))
                    .flatten().map(|path| format!("{base}/anys/{path}"));
                let bytes = if let Some(alternate) = alternate {
                    match Self::fetch_remote(&alternate).await {
                        Err(error) if error.is::<MissingFile>() => Self::fetch_remote(&self.url).await?,
                        result => result?,
                    }
                } else {
                    Self::fetch_remote(&self.url).await?
                };
                cacache::write(&*CACHE_DIR, &self.url, &bytes).await?;
                Ok(bytes)
            }
            Err(err) => Err(err.into()),
        }
    }

    pub async fn load_image(&self) -> Result<DynamicImage> {
        Ok(image::load_from_memory(&self.fetch().await?)?)
    }

    pub async fn load_thumbnail(&self) -> Result<DynamicImage> {
        if self.url.starts_with("https://phira.mivik.cn/") {
            File {
                url: format!("{}?imageView/0/w/{THUMBNAIL_WIDTH}/h/{THUMBNAIL_HEIGHT}", self.url),
            }
            .load_image()
            .await
        } else if self.url.starts_with("https://files.phira.cn/")
            || self.url.starts_with("https://phira.5wyxi.com/files/")
            || self.url.starts_with(&format!("{}/files/", api_url()))
        {
            File {
                url: format!("{}.thumbnail", self.url),
            }
            .load_image()
            .await
        } else {
            self.load_image().await
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub intro: String,
    pub illust: String,
    pub artist: String,
    pub designer: String,

    #[serde(default)]
    pub name_size: Option<f32>,

    #[serde(default)]
    pub baseline: bool,

    #[serde(default)]
    pub illu_adjust: (f32, f32, f32, f32),

    /// Locally edited translucent English label. Empty means use the default derived from `id`.
    #[serde(default)]
    pub name_en_override: Option<String>,

    #[serde(skip)]
    name_en: Option<String>,
}
impl Default for Character {
    fn default() -> Self {
        Self {
            id: "shee".to_owned(),
            name: ttl!("main-character-name").into_owned(),
            intro: ttl!("main-character-intro").into_owned(),
            illust: "@".to_owned(),
            artist: "清水QR".to_owned(),
            designer: "清水QR".to_owned(),

            name_size: None,

            baseline: false,

            illu_adjust: (0., 0., 0., 0.),

            name_en_override: None,

            name_en: None,
        }
    }
}
impl Character {
    pub fn name_en(&mut self) -> &str {
        if let Some(name) = self.name_en_override.as_deref().filter(|name| !name.is_empty()) {
            return name;
        }
        if self.name_en.is_none() {
            let words = self.id.split('_');
            let mut name_en = String::new();
            for word in words {
                let (first, rest) = word.split_at(1);
                name_en.push_str(&first.to_uppercase());
                name_en.push_str(rest);
                name_en.push(' ');
            }
            if !name_en.is_empty() {
                name_en.pop();
            }
            self.name_en = Some(name_en);
        }
        self.name_en.as_ref().unwrap()
    }
}
