use crate::browser::{Browser, Ev};
use crate::config::Config;
use crate::spotify::{Spotify, Track};
use crate::tags::{self, Field, Tags};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use image::DynamicImage;
use ratatui::widgets::ListState;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

#[derive(PartialEq, Clone, Copy)]
pub enum Screen { Browser, Search, Compare }
#[derive(PartialEq, Clone, Copy)]
pub enum Focus { Input, Results }

pub enum Msg {
    Search(u64, Result<Vec<Track>, String>),
    Preview(String, Option<DynamicImage>),
    Saved(Result<PathBuf, String>),
    Peek(PathBuf, Option<(Tags, usize, Option<(DynamicImage, (u32, u32))>)>),
    Best(String, Result<(Vec<u8>, DynamicImage), String>),
}

#[derive(Default, Clone)]
pub struct Input { pub text: String, pub cur: usize }

impl Input {
    pub fn new(s: &str) -> Self { Self { text: s.to_string(), cur: s.chars().count() } }
    fn byte(&self, ci: usize) -> usize {
        self.text.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(self.text.len())
    }
    pub fn key(&mut self, k: &KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let len = self.text.chars().count();
        match k.code {
            KeyCode::Char('u') if ctrl => { self.text.drain(..self.byte(self.cur)); self.cur = 0; }
            KeyCode::Char('w') if ctrl => {
                let chars: Vec<char> = self.text.chars().collect();
                let mut i = self.cur;
                while i > 0 && chars[i - 1] == ' ' { i -= 1; }
                while i > 0 && chars[i - 1] != ' ' { i -= 1; }
                let (a, b) = (self.byte(i), self.byte(self.cur));
                self.text.drain(a..b);
                self.cur = i;
            }
            KeyCode::Char('a') if ctrl => self.cur = 0,
            KeyCode::Char('e') if ctrl => self.cur = len,
            KeyCode::Char(c) if !ctrl => { let b = self.byte(self.cur); self.text.insert(b, c); self.cur += 1; }
            KeyCode::Backspace if self.cur > 0 => {
                let (a, b) = (self.byte(self.cur - 1), self.byte(self.cur));
                self.text.drain(a..b);
                self.cur -= 1;
            }
            KeyCode::Delete if self.cur < len => {
                let (a, b) = (self.byte(self.cur), self.byte(self.cur + 1));
                self.text.drain(a..b);
            }
            KeyCode::Left => self.cur = self.cur.saturating_sub(1),
            KeyCode::Right => self.cur = (self.cur + 1).min(len),
            KeyCode::Home => self.cur = 0,
            KeyCode::End => self.cur = len,
            _ => return false,
        }
        true
    }
}

/// Number of selectable rows in the compare view: 9 tag fields + the cover.
pub const COVER_ROW: usize = 9;
/// Rows in the left column of the compare grid; the rest are in the right one.
pub const LEFT_ROWS: usize = 5;
/// Button "rows" below the grid in the review view.
/// Full-width filename row under the grid.
pub const FILE_ROW: usize = 10;
pub const SAVE_BTN: usize = 11;
pub const CANCEL_BTN: usize = 12;
/// "Use ARTIST - TITLE" button at the right end of the filename row.
pub const FORMAT_BTN: usize = 13;

impl Compare {
    /// Filename currently equals the standard "ALBUM ARTIST - TITLE.ext".
    pub fn is_standard(&self) -> bool { self.filename == standard_name(self) }

    /// On: remember the current name and switch to the standard one. Off: restore it.
    pub fn toggle_standard(&mut self) {
        if self.is_standard() {
            self.filename = std::mem::take(&mut self.prev_filename);
        } else {
            self.prev_filename = self.filename.clone();
            self.filename = standard_name(self);
        }
    }
    /// Cover is wanted but hasn't finished downloading yet.
    pub fn cover_pending(&self) -> bool { self.apply_cover && self.cover_bytes.is_none() && self.cover_err.is_none() }
}

pub struct Compare {
    pub track: Track,
    pub spotify: Tags,
    pub new: Tags,
    pub edited: [bool; 9],
    pub sel: usize,
    pub editing: Option<Input>,
    /// Set while the file is being written (drives the progress overlay).
    pub saving: Option<std::time::Instant>,
    /// y/n popup shown before writing.
    pub confirm: bool,
    /// Last grid row selected, so ↑ from the filename row returns to the same column.
    pub last_grid: usize,
    pub apply_cover: bool,
    pub filename: String,
    pub orig_filename: String,
    /// Name to restore when the standard-name toggle is switched off.
    pub prev_filename: String,
    pub cover_bytes: Option<Vec<u8>>,
    pub cover_dims: Option<(u32, u32)>,
    pub cover_proto: Option<StatefulProtocol>,
    pub cover_err: Option<String>,
}

/// What the browser's side panel shows for the song under the cursor.
pub struct Peek {
    pub path: PathBuf,
    pub tags: Tags,
    pub cover_len: usize,
    pub dims: Option<(u32, u32)>,
    pub proto: Option<StatefulProtocol>,
}

pub struct App {
    pub spotify: Arc<Spotify>,
    pub picker: Picker,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub screen: Screen,
    pub br: Browser,

    pub path: Option<PathBuf>,
    pub current: Tags,
    pub current_proto: Option<StatefulProtocol>,
    pub current_dims: Option<(u32, u32)>,

    pub query: Input,
    pub focus: Focus,
    pub results: Vec<Track>,
    pub list: ListState,
    pub searching: bool,
    search_seq: u64,
    pub searched: bool,
    pub previews: HashMap<String, StatefulProtocol>,
    pending: HashSet<String>,

    pub peek: Option<Peek>,
    peek_pending: Option<PathBuf>,
    pub cmp: Option<Compare>,
    pub status: Option<(String, bool)>,
    pub tick: usize,
    pub quit: bool,
}

impl App {
    pub fn new(cfg: Config, picker: Picker, start: Option<PathBuf>) -> Self {
        let (tx, rx) = channel();
        Self {
            spotify: Arc::new(Spotify::new(cfg.spotify_client_id.clone(), cfg.spotify_client_secret.clone())),
            picker, tx, rx,
            screen: Screen::Browser, br: Browser::new(start.unwrap_or_else(|| cfg.music_dir())),
            path: None, current: Tags::default(), current_proto: None, current_dims: None,
            query: Input::default(), focus: Focus::Input, results: vec![], list: ListState::default(),
            searching: false, search_seq: 0, searched: false,
            previews: HashMap::new(), pending: HashSet::new(),
            peek: None, peek_pending: None,
            cmp: None, status: None, tick: 0, quit: false,
        }
    }

    pub fn ok(&mut self, s: impl Into<String>) { self.status = Some((s.into(), false)); }
    pub fn err(&mut self, s: impl Into<String>) { self.status = Some((s.into(), true)); }

    // ---------- background messages ----------
    pub fn pump(&mut self) {
        self.br.poll_meta();
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Search(seq, r) if seq == self.search_seq => {
                    self.searching = false;
                    self.searched = true;
                    match r {
                        Ok(t) => {
                            self.list.select(if t.is_empty() { None } else { Some(0) });
                            if t.is_empty() { self.err("No results — try a simpler query"); } else { self.status = None; }
                            self.results = t;
                            if !self.results.is_empty() { self.focus = Focus::Results; }
                            self.ensure_preview();
                        }
                        Err(e) => self.err(e),
                    }
                }
                Msg::Saved(r) => match r {
                    Ok(new_path) => {
                        let name = file_name(Some(&new_path));
                        self.path = Some(new_path);
                        self.cmp = None;
                        self.peek = None;
                        self.screen = Screen::Browser;
                        self.br.reload(Some(name.clone()));
                        self.ok(format!("Saved ✓  {name}"));
                    }
                    Err(e) => {
                        if let Some(c) = self.cmp.as_mut() { c.saving = None; }
                        self.err(format!("Write failed: {e}"));
                    }
                },
                Msg::Peek(path, r) => {
                    self.peek_pending = None;
                    let (tags, cover_len, img) = r.unwrap_or_default();
                    let dims = img.as_ref().map(|(_, d)| *d);
                    let proto = img.map(|(i, _)| self.picker.new_resize_protocol(i));
                    self.peek = Some(Peek { path, tags, cover_len, dims, proto });
                }
                Msg::Search(..) => {}
                Msg::Preview(url, img) => {
                    self.pending.remove(&url);
                    if let Some(img) = img { self.previews.insert(url, self.picker.new_resize_protocol(img)); }
                }
                Msg::Best(url, r) => {
                    let picker = &self.picker;
                    if let Some(c) = self.cmp.as_mut().filter(|c| c.track.cover_url.as_deref() == Some(&url)) {
                        match r {
                            Ok((bytes, img)) => {
                                c.cover_dims = Some((img.width(), img.height()));
                                c.cover_proto = Some(picker.new_resize_protocol(img));
                                c.cover_bytes = Some(bytes);
                            }
                            Err(e) => { c.cover_err = Some(e); c.apply_cover = false; }
                        }
                    }
                }
            }
        }
    }

    /// Keep the clean layout's side panel on the song under the cursor.
    /// One read in flight at a time, always for the latest selection.
    pub fn update_peek(&mut self) {
        if self.screen != Screen::Browser || self.br.layout != crate::browser::Layout::Clean || self.peek_pending.is_some() { return; }
        let Some(e) = self.br.selected().filter(|e| e.audio) else { return };
        if self.peek.as_ref().is_some_and(|p| p.path == e.path) { return; }
        let path = e.path.clone();
        self.peek_pending = Some(path.clone());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = tags::read(&path).ok().map(|mut t| {
                let len = t.cover.as_ref().map_or(0, |c| c.len());
                let img = t.cover.take().and_then(|b| image::load_from_memory(&b).ok())
                    .map(|i| { let d = (i.width(), i.height()); (i.thumbnail(800, 800), d) });
                (t, len, img)
            });
            let _ = tx.send(Msg::Peek(path, r));
        });
    }

    pub fn selected_track(&self) -> Option<&Track> {
        self.list.selected().and_then(|i| self.results.get(i))
    }

    fn ensure_preview(&mut self) {
        let Some(url) = self.selected_track().and_then(|t| t.cover_url.clone()) else { return };
        if self.previews.contains_key(&url) || !self.pending.insert(url.clone()) { return; }
        let (sp, tx) = (self.spotify.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let img = sp.download(&url).ok().and_then(|b| image::load_from_memory(&b).ok());
            let _ = tx.send(Msg::Preview(url, img));
        });
    }

    // ---------- keys ----------
    pub fn key(&mut self, k: KeyEvent) {
        // Don't let anything interrupt a write in progress.
        if self.cmp.as_ref().is_some_and(|c| c.saving.is_some()) { return; }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match self.screen {
            Screen::Browser => self.key_browser(k),
            Screen::Search => self.key_search(k),
            Screen::Compare => self.key_compare(k),
        }
    }

    fn key_browser(&mut self, k: KeyEvent) {
        self.status = None;
        // a refresh means tags may have changed on disk
        if matches!(k.code, KeyCode::Char('u') | KeyCode::Char('r')) { self.peek = None; }
        match self.br.key(k) {
            Ev::None => {}
            Ev::Quit => self.quit = true,
            Ev::Tag(p) => self.load_song(p),
            Ev::Status(m) => self.err(m),
            Ev::Info(m) => self.ok(m),
        }
    }

    fn back_to_browser(&mut self) {
        self.peek = None;
        self.status = None;
        self.screen = Screen::Browser;
        self.br.reload(None);
    }

    pub fn load_song(&mut self, path: PathBuf) {
        match tags::read(&path) {
            Ok(t) => {
                self.current_dims = None;
                self.current_proto = t.cover.as_ref().and_then(|b| image::load_from_memory(b).ok()).map(|img| {
                    self.current_dims = Some((img.width(), img.height()));
                    self.picker.new_resize_protocol(img)
                });
                let q = if t.get(Field::Title).is_empty() {
                    path.file_stem().map(|s| s.to_string_lossy().replace(" - ", " ").replace('_', " ")).unwrap_or_default()
                } else {
                    format!("{} {}", t.get(Field::Title), t.get(Field::Artist)).trim().to_string()
                };
                self.query = Input::new(&q);
                self.current = t;
                self.path = Some(path);
                self.results.clear();
                self.list.select(None);
                self.searched = false;
                self.focus = Focus::Input;
                self.status = None;
                self.screen = Screen::Search;
            }
            Err(e) => {
                self.screen = Screen::Browser;
                self.err(format!("Couldn't read tags: {e}"));
            }
        }
    }

    fn key_search(&mut self, k: KeyEvent) {
        match self.focus {
            Focus::Input => match k.code {
                KeyCode::Esc => self.back_to_browser(),
                KeyCode::Enter => self.search(),
                KeyCode::Down | KeyCode::Tab if !self.results.is_empty() => { self.focus = Focus::Results; }
                _ => { self.query.key(&k); }
            },
            Focus::Results => {
                let cur = self.list.selected().unwrap_or(0);
                match k.code {
                    KeyCode::Esc | KeyCode::Tab | KeyCode::Char('/') => self.focus = Focus::Input,
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.list.select(Some(cur.saturating_sub(1)));
                    }
                    KeyCode::Down | KeyCode::Char('j') => self.list.select(Some((cur + 1).min(self.results.len() - 1))),
                    KeyCode::Enter => self.open_compare(),
                    _ => {}
                }
                self.ensure_preview();
            }
        }
    }

    fn search(&mut self) {
        let q = self.query.text.trim().to_string();
        if q.is_empty() { return; }
        self.search_seq += 1;
        let seq = self.search_seq;
        self.searching = true;
        self.status = None;
        let (sp, tx) = (self.spotify.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let r = sp.search(&q).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Search(seq, r));
        });
    }

    fn open_compare(&mut self) {
        let Some(track) = self.selected_track().cloned() else { return };
        let cur = &self.current;
        let mut sp = Tags::default();
        sp.set(Field::Title, track.name.clone());
        sp.set(Field::Artist, track.artists.join(", "));
        sp.set(Field::Album, track.album.clone());
        let aa = if track.album_artists.is_empty() { &track.artists } else { &track.album_artists };
        sp.set(Field::AlbumArtist, aa.join(", "));
        sp.set(Field::Year, track.year().to_string());
        sp.set(Field::Track, track.track_number.to_string());
        sp.set(Field::Disc, track.disc_number.to_string());
        sp.set(Field::Isrc, track.isrc.clone().unwrap_or_default());
        // Spotify no longer exposes genres; keep whatever the file has.
        sp.set(Field::Genre, cur.get(Field::Genre).to_string());
        let mut new = sp.clone();
        for f in Field::ALL {
            if new.get(f).is_empty() || new.get(f) == "0" { new.set(f, cur.get(f).to_string()); }
        }

        if let Some(url) = track.cover_url.clone() {
            let (s, tx) = (self.spotify.clone(), self.tx.clone());
            std::thread::spawn(move || {
                let r = s.download_best_cover(&url).map_err(|e| format!("{e:#}")).and_then(|b| {
                    image::load_from_memory(&b).map(|i| (b, i)).map_err(|e| e.to_string())
                });
                let _ = tx.send(Msg::Best(url, r));
            });
        }
        self.cmp = Some(Compare {
            apply_cover: track.cover_url.is_some(),
            filename: file_name(self.path.as_deref()), orig_filename: file_name(self.path.as_deref()), prev_filename: file_name(self.path.as_deref()),
            track, spotify: sp, new, edited: [false; 9], sel: 0, editing: None, saving: None, confirm: false, last_grid: 0,
            cover_bytes: None, cover_dims: None, cover_proto: None, cover_err: None,
        });
        self.status = None;
        self.screen = Screen::Compare;
    }

    fn key_compare(&mut self, k: KeyEvent) {
        enum Act { None, Save, Cancel }
        let mut act = Act::None;
        let changes = self.change_count();
        let mut warn = None;
        {
            let Some(c) = self.cmp.as_mut() else { return };
            if c.confirm {
                match k.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => { c.confirm = false; act = Act::Save; }
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => c.confirm = false,
                    _ => {}
                }
            } else if let Some(inp) = c.editing.as_mut() {
                match k.code {
                    KeyCode::Enter if c.sel == FILE_ROW => {
                        let stem = inp.text.trim();
                        if stem.is_empty() || stem == "." || stem == ".." || stem.contains(['/', '\0']) {
                            self.status = Some(("Filename can't be empty or contain “/”".into(), true));
                        } else {
                            c.filename = format!("{stem}{}", ext_of(&c.orig_filename));
                            c.editing = None;
                            self.status = None;
                        }
                    }
                    KeyCode::Enter => {
                        let f = Field::ALL[c.sel];
                        c.new.set(f, inp.text.trim().to_string());
                        c.edited[c.sel] = true;
                        c.editing = None;
                    }
                    KeyCode::Esc => { c.editing = None; self.status = None; }
                    _ => { inp.key(&k); }
                }
            } else {
                match k.code {
                    // back to the search results to pick a different track
                    KeyCode::Esc => { self.screen = Screen::Search; self.cmp = None; return; }
                    // Columns are independent: ↓ past the last row of either one goes to the filename row.
                    KeyCode::Down | KeyCode::Char('j') => c.sel = match c.sel {
                        SAVE_BTN | CANCEL_BTN => c.sel,
                        FILE_ROW | FORMAT_BTN => SAVE_BTN,
                        s if s == LEFT_ROWS - 1 || s == COVER_ROW => { c.last_grid = s; FILE_ROW }
                        s => s + 1,
                    },
                    KeyCode::Up | KeyCode::Char('k') => c.sel = match c.sel {
                        SAVE_BTN | CANCEL_BTN => FILE_ROW,
                        FILE_ROW | FORMAT_BTN => c.last_grid,
                        s if s == 0 || s == LEFT_ROWS => s,
                        s => s - 1,
                    },
                    KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') | KeyCode::Tab => {
                        let right = matches!(k.code, KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab);
                        c.sel = match c.sel {
                            FILE_ROW => if right { FORMAT_BTN } else { FILE_ROW },
                            FORMAT_BTN => if right { FORMAT_BTN } else { FILE_ROW },
                            SAVE_BTN => CANCEL_BTN,
                            CANCEL_BTN => SAVE_BTN,
                            s if s < LEFT_ROWS => s + LEFT_ROWS,
                            s => s - LEFT_ROWS,
                        };
                    }
                    KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char(' ') => match c.sel {
                        SAVE_BTN => if changes == 0 { warn = Some("Nothing to save — no changes") } else if c.cover_pending() { warn = Some("Cover art is still downloading — try again in a moment") } else { c.confirm = true },
                        CANCEL_BTN => act = Act::Cancel,
                        COVER_ROW => c.apply_cover = !c.apply_cover && c.cover_bytes.is_some(),
                        FILE_ROW => c.editing = Some(Input::new(stem_of(&c.filename))),
                        FORMAT_BTN => c.toggle_standard(),
                        _ => c.editing = Some(Input::new(c.new.get(Field::ALL[c.sel]))),
                    },
                    KeyCode::Char('r') if c.sel == FILE_ROW => c.toggle_standard(),
                    // reset to the Spotify value
                    KeyCode::Char('r') if c.sel < COVER_ROW => {
                        let f = Field::ALL[c.sel];
                        c.new.set(f, c.spotify.get(f).to_string());
                        c.edited[c.sel] = false;
                    }
                    KeyCode::Char('s') => if changes == 0 { warn = Some("Nothing to save — no changes") } else if c.cover_pending() { warn = Some("Cover art is still downloading — try again in a moment") } else { c.confirm = true },
                    _ => {}
                }
            }
        }
        if let Some(w) = warn { self.err(w); }
        match act {
            Act::Save => self.save(),
            Act::Cancel => {
                self.cmp = None;
                self.back_to_browser();
            }
            Act::None => {}
        }
    }

    pub fn change_count(&self) -> usize {
        let Some(c) = &self.cmp else { return 0 };
        Field::ALL.iter().filter(|f| c.new.get(**f) != self.current.get(**f)).count() + (c.apply_cover && c.cover_bytes.is_some()) as usize + (c.filename != c.orig_filename) as usize
    }

    /// Write on a worker thread so the UI can show progress (big FLACs are rewritten in full).
    fn save(&mut self) {
        if self.change_count() == 0 {
            self.err("Nothing to save — no changes");
            return;
        }
        let (Some(c), Some(path)) = (self.cmp.as_mut(), self.path.clone()) else { return };
        let cover = if c.apply_cover { c.cover_bytes.clone() } else { None };
        let new = c.new.clone();
        let new_path = path.with_file_name(&c.filename);
        if c.filename != c.orig_filename && new_path.exists() {
            let msg = format!("“{}” already exists in this folder", c.filename);
            self.status = Some((msg, true));
            return;
        }
        c.saving = Some(std::time::Instant::now());
        self.status = None;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = tags::write(&path, &new, cover.as_deref()).map_err(|e| format!("{e:#}")).and_then(|()| {
                if new_path != path {
                    std::fs::rename(&path, &new_path).map_err(|e| format!("tags saved, but renaming failed: {e}"))?;
                }
                Ok(new_path)
            });
            let _ = tx.send(Msg::Saved(r));
        });
    }
}

fn file_name(p: Option<&std::path::Path>) -> String {
    p.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// ".flac" (with the dot), or "" when there is none.
fn ext_of(name: &str) -> String {
    std::path::Path::new(name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()
}

fn stem_of(name: &str) -> &str {
    std::path::Path::new(name).file_stem().and_then(|s| s.to_str()).unwrap_or(name)
}

/// "ALBUM ARTIST - TITLE.ext" from the new tags (falls back to the track artist),
/// with characters filesystems dislike replaced.
pub fn standard_name(c: &Compare) -> String {
    let who = match c.new.get(Field::AlbumArtist) { "" => c.new.get(Field::Artist), a => a };
    let base = format!("{who} - {}", c.new.get(Field::Title));
    let base: String = base.chars()
        .map(|ch| if matches!(ch, '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { ch })
        .collect();
    format!("{}{}", base.trim(), ext_of(&c.orig_filename))
}
