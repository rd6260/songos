use anyhow::{Result, anyhow};
use serde::{Deserialize, Deserializer};
use std::time::Duration;

fn nn<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// One LRCLIB record.
#[derive(Deserialize, Clone, Debug)]
pub struct Rec {
    #[serde(default, rename = "trackName", deserialize_with = "nn")] pub name: String,
    #[serde(default, rename = "artistName", deserialize_with = "nn")] pub artist: String,
    #[serde(default, rename = "albumName", deserialize_with = "nn")] pub album: String,
    #[serde(default)] pub duration: Option<f64>,
    #[serde(default)] pub instrumental: bool,
    #[serde(default, rename = "plainLyrics", deserialize_with = "nn")] pub plain: String,
    #[serde(default, rename = "syncedLyrics", deserialize_with = "nn")] pub synced: String,
}

/// Which lyrics version to show/save.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ver { Synced, Plain, Both }

impl Ver {
    pub fn label(self) -> &'static str {
        match self { Ver::Synced => "synced (LRC)", Ver::Plain => "plain", Ver::Both => "synced + plain" }
    }
}

/// Separator used when synced and plain are shown/compared as one text.
pub const SEP: &str = "\n\n──── plain ────\n\n";

impl Rec {
    /// Versions this record can provide, in cycling order.
    pub fn versions(&self) -> Vec<Ver> {
        let mut v = vec![];
        if self.has_synced() { v.push(Ver::Synced); }
        if self.has_plain() { v.push(Ver::Plain); }
        if self.has_synced() && self.has_plain() { v.push(Ver::Both); }
        v
    }
    /// The preferred version if this record has it, else the best it does have.
    pub fn pick(&self, pref: Ver) -> Ver {
        let v = self.versions();
        if v.contains(&pref) { pref } else { v.first().copied().unwrap_or(Ver::Plain) }
    }
    pub fn next_ver(&self, cur: Ver) -> Ver {
        let v = self.versions();
        let i = v.iter().position(|x| *x == cur).unwrap_or(0);
        v.get((i + 1) % v.len().max(1)).copied().unwrap_or(cur)
    }
    /// Text for a version (Both = synced, separator, plain).
    pub fn text_of(&self, ver: Ver) -> String {
        match ver {
            Ver::Synced => self.synced.clone(),
            Ver::Plain => self.plain.clone(),
            Ver::Both => format!("{}{SEP}{}", self.synced.trim_end(), self.plain.trim()),
        }
    }

    pub fn has_synced(&self) -> bool { !self.synced.trim().is_empty() }
    pub fn has_plain(&self) -> bool { !self.plain.trim().is_empty() }
    pub fn has_any(&self) -> bool { self.has_synced() || self.has_plain() }
    pub fn duration_ms(&self) -> u64 { (self.duration.unwrap_or(0.0) * 1000.0) as u64 }
}

/// Free-text search; results closest in length to the local file come first,
/// records without any lyrics last.
pub fn search(q: &str, local_ms: u64) -> Result<Vec<Rec>> {
    let http = reqwest::blocking::Client::builder()
        .user_agent("songos/0.1")
        .timeout(Duration::from_secs(20))
        .build()?;
    let r = http.get("https://lrclib.net/api/search").query(&[("q", q)]).send()?;
    if !r.status().is_success() { return Err(anyhow!("LRCLIB error {}", r.status())); }
    let mut recs: Vec<Rec> = r.json()?;
    recs.truncate(25);
    recs.sort_by_key(|r| {
        let diff = if local_ms == 0 || r.duration.is_none() { 0 } else { r.duration_ms().abs_diff(local_ms) };
        (!r.has_any(), diff)
    });
    Ok(recs)
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "hits the network"]
    fn live_search() {
        let r = super::search("radiohead creep", 238_000).unwrap();
        assert!(!r.is_empty() && r[0].has_any());
    }
}
