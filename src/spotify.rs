use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Track {
    pub name: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artists: Vec<String>,
    pub release_date: String,
    pub track_number: u32,
    pub disc_number: u32,
    pub total_tracks: u32,
    pub duration_ms: u64,
    pub isrc: Option<String>,
    pub popularity: u32,
    pub explicit: bool,
    /// Largest cover advertised by the API (usually 640x640).
    pub cover_url: Option<String>,
    pub cover_px: u32,
}

impl Track {
    pub fn year(&self) -> &str {
        self.release_date.get(..4).unwrap_or("")
    }
}

#[derive(Deserialize)]
struct SearchResp { tracks: Tracks }
#[derive(Deserialize)]
struct Tracks { items: Vec<Item> }
#[derive(Deserialize)]
struct Item {
    name: String,
    artists: Vec<Artist>,
    album: Album,
    #[serde(default)] track_number: u32,
    #[serde(default)] disc_number: u32,
    #[serde(default)] duration_ms: u64,
    #[serde(default)] popularity: u32,
    #[serde(default)] explicit: bool,
    external_ids: Option<Ext>,
}
#[derive(Deserialize)]
struct Ext { isrc: Option<String> }
#[derive(Deserialize)]
struct Artist { name: String }
#[derive(Deserialize)]
struct Album {
    name: String,
    #[serde(default)] artists: Vec<Artist>,
    #[serde(default)] release_date: String,
    #[serde(default)] total_tracks: u32,
    #[serde(default)] images: Vec<Image>,
}
#[derive(Deserialize)]
struct Image { url: String, width: Option<u32> }

pub struct Spotify {
    http: reqwest::blocking::Client,
    id: String,
    secret: String,
    token: Mutex<Option<(String, Instant)>>,
}

impl Spotify {
    pub fn new(id: String, secret: String) -> Self {
        let http = reqwest::blocking::Client::builder()
            .user_agent("songos/0.1")
            .timeout(Duration::from_secs(20))
            .build()
            .expect("http client");
        Self { http, id, secret, token: Mutex::new(None) }
    }

    fn token(&self) -> Result<String> {
        let mut guard = self.token.lock().unwrap();
        if let Some((t, exp)) = guard.as_ref() {
            if Instant::now() < *exp { return Ok(t.clone()); }
        }
        #[derive(Deserialize)]
        struct Tok { access_token: String, expires_in: u64 }
        let r = self.http.post("https://accounts.spotify.com/api/token")
            .basic_auth(&self.id, Some(&self.secret))
            .form(&[("grant_type", "client_credentials")])
            .send()?;
        if !r.status().is_success() {
            return Err(anyhow!("Spotify auth failed ({}): check your client id/secret", r.status()));
        }
        let t: Tok = r.json()?;
        *guard = Some((t.access_token.clone(), Instant::now() + Duration::from_secs(t.expires_in.saturating_sub(30))));
        Ok(t.access_token)
    }

    fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<reqwest::blocking::Response> {
        let r = self.http.get(url).bearer_auth(self.token()?).query(query).send()?;
        if !r.status().is_success() {
            let s = r.status();
            let body = r.text().unwrap_or_default();
            return Err(anyhow!("Spotify API {s}: {}", body.chars().take(160).collect::<String>()));
        }
        Ok(r)
    }

    pub fn search(&self, q: &str) -> Result<Vec<Track>> {
        let resp: SearchResp = self.get(
            "https://api.spotify.com/v1/search",
            &[("q", q), ("type", "track"), ("limit", "10")],
        )?.json().context("decoding search response")?;
        Ok(resp.tracks.items.into_iter().map(|i| {
            let best = i.album.images.iter().max_by_key(|im| im.width.unwrap_or(0));
            Track {
                artists: i.artists.into_iter().map(|a| a.name).collect(),
                name: i.name,
                album: i.album.name,
                album_artists: i.album.artists.into_iter().map(|a| a.name).collect(),
                release_date: i.album.release_date,
                track_number: i.track_number,
                disc_number: i.disc_number,
                total_tracks: i.album.total_tracks,
                duration_ms: i.duration_ms,
                isrc: i.external_ids.and_then(|e| e.isrc),
                popularity: i.popularity,
                explicit: i.explicit,
                cover_px: best.and_then(|b| b.width).unwrap_or(0),
                cover_url: best.map(|b| b.url.clone()),
            }
        }).collect())
    }

    pub fn download(&self, url: &str) -> Result<Vec<u8>> {
        let r = self.http.get(url).send()?.error_for_status()?;
        Ok(r.bytes()?.to_vec())
    }

    /// Spotify's CDN serves the original upload (often 2000-3000px) under the
    /// `ab67616d000082c1` prefix, while the API only lists up to 640px. Try
    /// the original first and fall back to the API's largest image.
    pub fn download_best_cover(&self, url: &str) -> Result<Vec<u8>> {
        if let Some(rest) = url.strip_prefix("https://i.scdn.co/image/ab67616d0000b273") {
            let hi = format!("https://i.scdn.co/image/ab67616d000082c1{rest}");
            if let Ok(b) = self.download(&hi) {
                if image::guess_format(&b).is_ok() { return Ok(b); }
            }
        }
        self.download(url)
    }
}
