use anyhow::{Result, anyhow};
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::{ItemKey, Tag};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field { Title, Artist, Album, AlbumArtist, Year, Track, Disc, Genre, Isrc }

impl Field {
    pub const ALL: [Field; 9] = [
        Field::Title, Field::Artist, Field::Album, Field::AlbumArtist, Field::Genre,
        Field::Year, Field::Track, Field::Disc, Field::Isrc,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Field::Title => "Title", Field::Artist => "Artist", Field::Album => "Album",
            Field::AlbumArtist => "Album Artist", Field::Year => "Year", Field::Track => "Track",
            Field::Disc => "Disc", Field::Genre => "Genre", Field::Isrc => "ISRC",
        }
    }
    fn key(self) -> ItemKey {
        match self {
            Field::Title => ItemKey::TrackTitle, Field::Artist => ItemKey::TrackArtist,
            Field::Album => ItemKey::AlbumTitle, Field::AlbumArtist => ItemKey::AlbumArtist,
            Field::Year => ItemKey::RecordingDate, Field::Track => ItemKey::TrackNumber,
            Field::Disc => ItemKey::DiscNumber, Field::Genre => ItemKey::Genre, Field::Isrc => ItemKey::Isrc,
        }
    }
}

#[derive(Clone, Default)]
pub struct Tags {
    pub values: [String; 9],
    pub cover: Option<Vec<u8>>,
    pub duration_ms: u64,
}

impl Tags {
    pub fn get(&self, f: Field) -> &str { &self.values[idx(f)] }
    pub fn set(&mut self, f: Field, v: String) { self.values[idx(f)] = v; }
    pub fn is_empty(&self) -> bool { self.values.iter().all(|v| v.is_empty()) }
}

pub fn idx(f: Field) -> usize { Field::ALL.iter().position(|x| *x == f).unwrap() }

pub fn read(path: &Path) -> Result<Tags> {
    let tf = lofty::read_from_path(path)?;
    let mut t = Tags::default();
    t.duration_ms = tf.properties().duration().as_millis() as u64;
    let Some(tag) = tf.primary_tag().or_else(|| tf.first_tag()) else { return Ok(t) };
    for f in Field::ALL {
        if let Some(v) = tag.get_string(f.key()) { t.set(f, v.to_string()); }
    }
    // Year is stored as a recording date (TDRC / DATE / ©day); show just the year.
    let y: String = t.get(Field::Year).chars().take(4).collect();
    t.set(Field::Year, if y.is_empty() { tag.get_string(ItemKey::Year).unwrap_or_default().chars().take(4).collect() } else { y });
    t.cover = tag.pictures().iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| tag.pictures().first())
        .map(|p| p.data().to_vec());
    Ok(t)
}

pub fn write(path: &Path, new: &Tags, cover: Option<&[u8]>) -> Result<()> {
    let mut tf = lofty::read_from_path(path)?;
    if tf.primary_tag().is_none() {
        let tt = tf.primary_tag_type();
        tf.insert_tag(Tag::new(tt));
    }
    let tag = tf.primary_tag_mut().ok_or_else(|| anyhow!("no writable tag"))?;
    for f in Field::ALL {
        let v = new.get(f).trim();
        if v.is_empty() { tag.remove_key(f.key()); } else { tag.insert_text(f.key(), v.to_string()); }
    }
    if let Some(bytes) = cover {
        let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { MimeType::Png } else { MimeType::Jpeg };
        tag.remove_picture_type(PictureType::CoverFront);
        tag.push_picture(Picture::unchecked(bytes.to_vec())
            .pic_type(PictureType::CoverFront)
            .mime_type(mime)
            .build());
    }
    tf.save_to_path(path, WriteOptions::default())?;
    Ok(())
}
