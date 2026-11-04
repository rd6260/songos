use crate::app::Input;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub audio: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Sort { NameAsc, NameDesc, ModifiedNew, ModifiedOld, SizeBig, SizeSmall, Type }

impl Sort {
    pub const ALL: [Sort; 7] = [Sort::NameAsc, Sort::NameDesc, Sort::ModifiedNew, Sort::ModifiedOld, Sort::SizeBig, Sort::SizeSmall, Sort::Type];
    pub fn label(self) -> &'static str {
        match self {
            Sort::NameAsc => "Name  (A → Z)", Sort::NameDesc => "Name  (Z → A)",
            Sort::ModifiedNew => "Modified  (newest first)", Sort::ModifiedOld => "Modified  (oldest first)",
            Sort::SizeBig => "Size  (largest first)", Sort::SizeSmall => "Size  (smallest first)",
            Sort::Type => "Type  (extension)",
        }
    }
}

pub enum Mode {
    Normal,
    /// `/` — jump to the first entry matching, without hiding anything.
    Goto { input: Input, origin: usize },
    /// `f` — hide everything that doesn't match.
    Filter(Input),
}

pub enum Popup {
    Sort(ListState),
    Action(ListState, PathBuf),
}

pub const ACTIONS: [&str; 1] = ["Tag from Spotify"];

pub enum Ev { None, Quit, Tag(PathBuf), Status(String) }

pub struct Browser {
    pub cwd: PathBuf,
    pub entries: Vec<Entry>,
    /// Indices into `entries` that pass the filter.
    pub visible: Vec<usize>,
    pub list: ListState,
    pub sort: Sort,
    pub filter: String,
    pub show_hidden: bool,
    pub mode: Mode,
    pub popup: Option<Popup>,
    pub last_goto: String,
    pub error: Option<String>,
}

fn is_audio(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e|
        matches!(e.to_lowercase().as_str(), "mp3" | "flac" | "m4a" | "ogg" | "opus" | "wav" | "aac"))
}

impl Browser {
    pub fn new(start: PathBuf) -> Self {
        let (cwd, select) = if start.is_file() {
            (start.parent().map(Path::to_path_buf).unwrap_or_default(), start.file_name().map(|n| n.to_string_lossy().into_owned()))
        } else { (start, None) };
        let mut b = Self {
            cwd, entries: vec![], visible: vec![], list: ListState::default(), sort: Sort::NameAsc,
            filter: String::new(), show_hidden: false, mode: Mode::Normal, popup: None,
            last_goto: String::new(), error: None,
        };
        b.reload(select);
        b
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.list.selected().and_then(|i| self.visible.get(i)).map(|&i| &self.entries[i])
    }

    /// Re-read the directory, then try to keep the cursor on `select`.
    pub fn reload(&mut self, select: Option<String>) {
        let select = select.or_else(|| self.selected().map(|e| e.name.clone()));
        self.error = None;
        self.entries.clear();
        match std::fs::read_dir(&self.cwd) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if !self.show_hidden && name.starts_with('.') { continue; }
                    let path = e.path();
                    let md = std::fs::metadata(&path).ok(); // follows symlinks
                    let is_dir = md.as_ref().is_some_and(|m| m.is_dir());
                    self.entries.push(Entry {
                        audio: !is_dir && is_audio(&path), is_dir, name, size: md.as_ref().map_or(0, |m| m.len()),
                        modified: md.and_then(|m| m.modified().ok()), path,
                    });
                }
            }
            Err(e) => self.error = Some(format!("{}: {e}", self.cwd.display())),
        }
        self.sort_entries();
        self.refilter(select);
    }

    fn sort_entries(&mut self) {
        let sort = self.sort;
        self.entries.sort_by(|a, b| {
            use std::cmp::Ordering::*;
            // directories always come first
            b.is_dir.cmp(&a.is_dir).then_with(|| {
                let by_name = a.name.to_lowercase().cmp(&b.name.to_lowercase());
                match sort {
                    Sort::NameAsc => by_name,
                    Sort::NameDesc => by_name.reverse(),
                    Sort::ModifiedNew => b.modified.cmp(&a.modified).then(by_name),
                    Sort::ModifiedOld => a.modified.cmp(&b.modified).then(by_name),
                    Sort::SizeBig => if a.is_dir { by_name } else { b.size.cmp(&a.size).then(by_name) },
                    Sort::SizeSmall => if a.is_dir { by_name } else { a.size.cmp(&b.size).then(by_name) },
                    Sort::Type => {
                        let ext = |e: &Entry| e.path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
                        match ext(a).cmp(&ext(b)) { Equal => by_name, o => o }
                    }
                }
            })
        });
    }

    fn refilter(&mut self, select: Option<String>) {
        let words: Vec<String> = self.filter.to_lowercase().split_whitespace().map(String::from).collect();
        self.visible = (0..self.entries.len()).filter(|&i| {
            let n = self.entries[i].name.to_lowercase();
            words.iter().all(|w| n.contains(w))
        }).collect();
        let pos = select.and_then(|s| self.visible.iter().position(|&i| self.entries[i].name == s));
        self.list.select(if self.visible.is_empty() { None } else { Some(pos.unwrap_or(0)) });
    }

    fn enter_dir(&mut self, dir: PathBuf, select: Option<String>) {
        self.cwd = dir;
        self.filter.clear();
        self.list.select(None);
        self.reload(select);
        if let Some(e) = &self.error { eprintln!("{e}"); }
    }

    fn go_up(&mut self) {
        if let Some(parent) = self.cwd.parent().map(Path::to_path_buf) {
            let from = self.cwd.file_name().map(|n| n.to_string_lossy().into_owned());
            self.enter_dir(parent, from);
        }
    }

    fn mv(&mut self, delta: isize) {
        if self.visible.is_empty() { return; }
        let cur = self.list.selected().unwrap_or(0) as isize;
        self.list.select(Some((cur + delta).clamp(0, self.visible.len() as isize - 1) as usize));
    }

    /// First entry whose name contains `q`, scanning from `from` and wrapping.
    fn find(&self, q: &str, from: usize, forward: bool) -> Option<usize> {
        let n = self.visible.len();
        if n == 0 || q.is_empty() { return None; }
        let q = q.to_lowercase();
        (0..n).map(|k| if forward { (from + k) % n } else { (from + n - k % n) % n })
            .find(|&i| self.entries[self.visible[i]].name.to_lowercase().contains(&q))
    }

    pub fn key(&mut self, k: KeyEvent) -> Ev {
        if let Some(kind) = self.popup.as_ref().map(p_kind) { return self.key_popup(k, kind); }
        match &mut self.mode {
            Mode::Goto { input, origin } => {
                match k.code {
                    KeyCode::Esc => { let o = *origin; self.list.select(Some(o)); self.mode = Mode::Normal; }
                    KeyCode::Enter => { self.last_goto = input.text.clone(); self.mode = Mode::Normal; }
                    _ => if input.key(&k) {
                        let (q, o) = (input.text.clone(), *origin);
                        // empty query goes back to where we started
                        let hit = self.find(&q, o, true).unwrap_or(o);
                        self.list.select(Some(hit));
                    },
                }
                Ev::None
            }
            Mode::Filter(input) => {
                match k.code {
                    KeyCode::Esc => { self.filter.clear(); self.mode = Mode::Normal; self.refilter(None); }
                    KeyCode::Enter => self.mode = Mode::Normal,
                    _ => if input.key(&k) { self.filter = input.text.clone(); self.refilter(None); },
                }
                Ev::None
            }
            Mode::Normal => self.key_normal(k),
        }
    }

    fn key_normal(&mut self, k: KeyEvent) -> Ev {
        let page = 10;
        match k.code {
            KeyCode::Char('q') => return Ev::Quit,
            KeyCode::Esc if !self.filter.is_empty() => { self.filter.clear(); self.refilter(None); }
            KeyCode::Down | KeyCode::Char('j') => self.mv(1),
            KeyCode::Up | KeyCode::Char('k') => self.mv(-1),
            KeyCode::PageDown => self.mv(page),
            KeyCode::PageUp => self.mv(-page),
            KeyCode::Char('d') if k.modifiers.contains(KeyModifiers::CONTROL) => self.mv(page),
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => self.mv(-page),
            KeyCode::Home | KeyCode::Char('g') => self.list.select(if self.visible.is_empty() { None } else { Some(0) }),
            KeyCode::End | KeyCode::Char('G') => self.list.select(self.visible.len().checked_sub(1)),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => self.go_up(),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                let Some(e) = self.selected() else { return Ev::None };
                if e.is_dir {
                    let p = e.path.clone();
                    self.enter_dir(p, None);
                } else if e.audio {
                    let mut st = ListState::default();
                    st.select(Some(0));
                    self.popup = Some(Popup::Action(st, e.path.clone()));
                } else if k.code == KeyCode::Enter {
                    return Ev::Status(format!("{} isn't a supported audio file", e.name));
                }
            }
            KeyCode::Char('s') => {
                let mut st = ListState::default();
                st.select(Sort::ALL.iter().position(|s| *s == self.sort));
                self.popup = Some(Popup::Sort(st));
            }
            KeyCode::Char('/') => {
                self.mode = Mode::Goto { input: Input::default(), origin: self.list.selected().unwrap_or(0) };
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                let fwd = k.code == KeyCode::Char('n');
                let cur = self.list.selected().unwrap_or(0);
                let n = self.visible.len().max(1);
                let from = if fwd { (cur + 1) % n } else { (cur + n - 1) % n };
                let q = self.last_goto.clone();
                match self.find(&q, from, fwd) {
                    Some(i) => self.list.select(Some(i)),
                    None if q.is_empty() => return Ev::Status("Nothing to repeat — press / to search first".into()),
                    None => return Ev::Status(format!("No match for “{q}”")),
                }
            }
            KeyCode::Char('f') => self.mode = Mode::Filter(Input::new(&self.filter)),
            KeyCode::Char('.') => { self.show_hidden = !self.show_hidden; self.reload(None); }
            KeyCode::Char('r') => self.reload(None),
            _ => {}
        }
        Ev::None
    }

    fn key_popup(&mut self, k: KeyEvent, kind: PopupKind) -> Ev {
        let len = match kind { PopupKind::Sort => Sort::ALL.len(), PopupKind::Action => ACTIONS.len() };
        let st = match self.popup.as_mut().unwrap() { Popup::Sort(s) | Popup::Action(s, _) => s };
        let cur = st.selected().unwrap_or(0);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => self.popup = None,
            KeyCode::Down | KeyCode::Char('j') => st.select(Some((cur + 1).min(len - 1))),
            KeyCode::Up | KeyCode::Char('k') => st.select(Some(cur.saturating_sub(1))),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => match self.popup.take().unwrap() {
                Popup::Sort(_) => {
                    self.sort = Sort::ALL[cur];
                    self.reload(None);
                }
                Popup::Action(_, path) => return Ev::Tag(path),
            },
            _ => {}
        }
        Ev::None
    }
}

#[derive(Clone, Copy)]
enum PopupKind { Sort, Action }
fn p_kind(p: &Popup) -> PopupKind { match p { Popup::Sort(_) => PopupKind::Sort, Popup::Action(..) => PopupKind::Action } }
