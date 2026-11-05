use crate::app::*;
use crate::browser::{ACTIONS, Layout as BrowserLayout, Mode, Popup, Sort};
use crate::spotify::Track;
use crate::tags::{Field, Tags};
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Position, Rect};
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui_image::{Resize, StatefulImage};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const ACCENT: Color = Color::Rgb(29, 215, 96);
const TEXT: Color = Color::Rgb(232, 234, 240);
const MUTED: Color = Color::Rgb(125, 130, 145);
const DIM: Color = Color::Rgb(66, 70, 84);
const SEL_BG: Color = Color::Rgb(38, 42, 56);
const CHANGED: Color = Color::Rgb(255, 196, 87);
const BAD: Color = Color::Rgb(242, 105, 115);
const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn panel<'a>(title: impl Into<Line<'a>>, active: bool) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if active { ACCENT } else { DIM }))
        .title(title)
        .padding(Padding::horizontal(1))
}

fn title(s: &str, active: bool) -> Line<'static> {
    Line::from(vec![
        Span::styled(" ", Style::new()),
        Span::styled(s.to_string(), Style::new().fg(if active { ACCENT } else { MUTED }).bold()),
        Span::styled(" ", Style::new()),
    ])
}

fn trunc(s: &str, w: usize) -> String {
    if s.chars().count() <= w { return s.to_string(); }
    let mut o: String = s.chars().take(w.saturating_sub(1)).collect();
    o.push('…');
    o
}

fn key_hint(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut v = vec![];
    for (i, (k, d)) in pairs.iter().enumerate() {
        if i > 0 { v.push(Span::styled("  ·  ", Style::new().fg(DIM))); }
        v.push(Span::styled(k.to_string(), Style::new().fg(ACCENT).bold()));
        v.push(Span::styled(format!(" {d}"), Style::new().fg(MUTED)));
    }
    Line::from(v)
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let [head, body, foot] = Layout::vertical([Constraint::Length(1), Constraint::Min(5), Constraint::Length(1)]).areas(f.area());

    // breadcrumb
    let mut crumbs = vec![Span::styled(" ♪ songos", Style::new().fg(ACCENT).bold())];
    let mut push = |s: String| {
        crumbs.push(Span::styled("  ›  ", Style::new().fg(DIM)));
        crumbs.push(Span::styled(s, Style::new().fg(MUTED)));
    };
    if app.screen == Screen::Browser {
        push(app.br.cwd.display().to_string());
        if app.br.layout == BrowserLayout::Clean {
            push(format!("{} items", app.br.visible.len()));
            if !app.br.filter.is_empty() { push(format!("filter “{}”", app.br.filter)); }
        }
    }
    else { push("Tag from Spotify".into()); }
    if matches!(app.screen, Screen::Search | Screen::Compare) {
        if let Some(p) = &app.path { push(p.file_name().unwrap_or_default().to_string_lossy().into_owned()); }
    }
    if app.screen == Screen::Compare { push("Review".into()); }
    f.render_widget(Paragraph::new(Line::from(crumbs)), head);

    match app.screen {
        Screen::Browser => browser(f, body, app),
        Screen::Search => search(f, body, app),
        Screen::Compare => compare(f, body, app),
    }

    // footer: status wins over hints
    let line = if let Some((msg, bad)) = &app.status {
        Line::from(Span::styled(format!(" {msg}"), Style::new().fg(if *bad { BAD } else { ACCENT }).bold()))
    } else {
        match app.screen {
            Screen::Browser => match (&app.br.popup, &app.br.mode) {
                (Some(_), _) => key_hint(&[("↑↓", "move"), ("↵", "choose"), ("esc", "close")]),
                (_, Mode::Goto { .. }) => key_hint(&[("type", "jump to match"), ("↵", "stay here"), ("esc", "cancel")]),
                (_, Mode::Filter(_)) => key_hint(&[("type", "filter"), ("↵", "keep filter"), ("esc", "clear")]),
                _ => key_hint(&[("↑↓", "move"), ("↵/l", "open"), ("h", "up"), ("s", "sort"), ("/", "go to"), ("n/N", "next/prev"), ("f", "filter"), ("P", "layout"), ("u", "refresh"), (".", "hidden"), ("q", "quit")]),
            },
            Screen::Search => match app.focus {
                Focus::Input => key_hint(&[("↵", "search spotify"), ("↓", "results"), ("esc", "back")]),
                Focus::Results => key_hint(&[("↑↓", "browse"), ("↵", "pick track"), ("/", "edit query"), ("esc", "query")]),
            },
            Screen::Compare => {
                let c = app.cmp.as_ref().unwrap();
                if c.confirm { key_hint(&[("y", "save to file"), ("n", "cancel")]) }
                else if c.saving.is_some() { key_hint(&[("…", "writing tags, please wait")]) }
                else if c.editing.is_some() { key_hint(&[("↵", "accept"), ("esc", "cancel edit")]) }
                else if c.sel == FILE_ROW { key_hint(&[("↵", "edit name"), ("→", "standard name"), ("↓", "buttons"), ("esc", "other track")]) }
                else if c.sel == FORMAT_BTN { key_hint(&[("↵", "toggle ARTIST - TITLE"), ("←", "back to name"), ("↓", "buttons"), ("esc", "other track")]) }
                else if c.sel >= SAVE_BTN { key_hint(&[("←→", "choose"), ("↵", "activate"), ("↑", "back to fields"), ("esc", "other track")]) }
                else { key_hint(&[("↑↓←→", "move"), ("↵", "edit"), ("r", "spotify value"), ("s", "save"), ("↓", "filename"), ("esc", "other track")]) }
            }
        }
    };
    f.render_widget(Paragraph::new(line), foot);
}

fn center_v(area: Rect, h: u16) -> Rect {
    let h = h.min(area.height);
    Rect { y: area.y + (area.height - h) / 2, height: h, ..area }
}

// ---------------------------------------------------------------- browser
/// Truncate to a display width (CJK-aware), adding an ellipsis.
fn fit(s: &str, w: usize) -> String {
    if UnicodeWidthStr::width(s) <= w { return s.to_string(); }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + cw + 1 > w { break; }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out
}

fn human(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let (mut v, mut i) = (n as f64, 0);
    while v >= 1024.0 && i < 4 { v /= 1024.0; i += 1; }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

fn browser(f: &mut Frame, area: Rect, app: &mut App) {
    let br = &mut app.br;
    let (list_area, bar) = match &br.mode {
        Mode::Normal => (area, None),
        _ => {
            let [l, b] = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(area);
            (l, Some(b))
        }
    };

    let empty = br.visible.is_empty();
    match br.layout {
        BrowserLayout::Detailed => {
            let inner_w = list_area.width.saturating_sub(5) as usize;
            let (size_w, date_w) = (10usize, 16usize);
            let name_w = inner_w.saturating_sub(size_w + date_w + 6);
            let items: Vec<ListItem> = br.visible.iter().map(|&i| {
                let e = &br.entries[i];
                let (icon, style) = if e.is_dir { ("▸ ", Style::new().fg(ACCENT).bold()) }
                    else if e.audio { ("♪ ", Style::new().fg(TEXT)) }
                    else { ("· ", Style::new().fg(MUTED)) };
                let name = if e.is_dir { format!("{}/", e.name) } else { e.name.clone() };
                let date = e.modified.map(|m| chrono::DateTime::<chrono::Local>::from(m).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(icon, if e.audio { Style::new().fg(ACCENT) } else { style }),
                    Span::styled(format!("{:<w$}", trunc(&name, name_w), w = name_w), style),
                    Span::styled(format!("  {:>w$}", if e.is_dir { "—".to_string() } else { human(e.size) }, w = size_w), Style::new().fg(MUTED)),
                    Span::styled(format!("  {date:>w$}", w = date_w), Style::new().fg(MUTED)),
                ]))
            }).collect();

            let mut t = format!("{}", br.cwd.display());
            t.push_str(&format!("  ·  {} items", br.visible.len()));
            if !br.filter.is_empty() { t.push_str(&format!("  ·  filter “{}”", br.filter)); }
            let block = panel(title(&t, true), true).title_bottom(Line::from(vec![
                Span::styled(" sorted by ", Style::new().fg(DIM)),
                Span::styled(format!("{} ", br.sort.label().split("  ").next().unwrap_or("")), Style::new().fg(MUTED)),
            ]).right_aligned());
            let list = List::new(items).block(block).highlight_style(Style::new().bg(SEL_BG).bold()).highlight_symbol("▍");
            f.render_stateful_widget(list, list_area, &mut br.list);
        }
        BrowserLayout::Clean => {
            // no border, no icons: artist left, title right (file name when untagged)
            let full = Rect { y: list_area.y + 1, height: list_area.height.saturating_sub(1), ..list_area };
            // song info panel on the right, like the Spotify results page (needs some width)
            let (area, side) = if full.width >= 90 {
                let [l, r] = Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).areas(full);
                (l, Some(Rect { width: r.width.saturating_sub(1), ..r }))
            } else { (full, None) };
            let w = area.width.saturating_sub(4) as usize;
            let items: Vec<ListItem> = {
                let meta = br.meta.lock().unwrap();
                br.visible.iter().map(|&i| {
                    let e = &br.entries[i];
                    let (left, right, ls, rs) = if e.is_dir {
                        (format!("{}/", e.name), String::new(), Style::new().fg(ACCENT).bold(), Style::new())
                    } else if !e.audio {
                        (String::new(), e.name.clone(), Style::new(), Style::new().fg(DIM))
                    } else {
                        let (a, t) = meta.get(&e.path).cloned().unwrap_or_default();
                        (a, if t.is_empty() { e.name.clone() } else { t }, Style::new().fg(Color::Rgb(190, 194, 206)), Style::new().fg(TEXT))
                    };
                    // the title gets up to 55% of the row, the artist whatever is left
                    let rw = UnicodeWidthStr::width(right.as_str()).min(w * 55 / 100);
                    let right = fit(&right, rw);
                    let rw = UnicodeWidthStr::width(right.as_str());
                    let left = fit(&left, w.saturating_sub(rw + 2));
                    let gap = w.saturating_sub(UnicodeWidthStr::width(left.as_str()) + rw);
                    ListItem::new(Line::from(vec![
                        Span::raw("  "), Span::styled(left, ls), Span::raw(" ".repeat(gap)), Span::styled(right, rs), Span::raw("  "),
                    ]))
                }).collect()
            };
            let list = List::new(items).highlight_style(Style::new().bg(SEL_BG).bold());
            f.render_stateful_widget(list, area, &mut br.list);
            if let Some(side) = side {
                let sel = br.selected().map(|e| (e.path.clone(), e.name.clone(), e.is_dir, e.audio, e.size));
                peek_panel(f, side, sel, &mut app.peek, app.tick);
            }
        }
    }
    if empty {
        let msg = br.error.clone().unwrap_or_else(|| if br.filter.is_empty() { "Empty folder".into() } else { "Nothing matches the filter".into() });
        f.render_widget(Paragraph::new(msg).style(Style::new().fg(if br.error.is_some() { BAD } else { MUTED })).alignment(Alignment::Center),
            center_v(list_area.inner(Margin::new(1, 1)), 1));
    }

    match (&br.mode, bar) {
        (Mode::Goto { input, .. }, Some(b)) => input_box(f, b, "Go to", input, true, "type part of a name…"),
        (Mode::Filter(input), Some(b)) => input_box(f, b, "Filter", input, true, "type to hide non-matching…"),
        _ => {}
    }

    // popups
    match &mut br.popup {
        Some(Popup::Sort(st)) => {
            let items: Vec<ListItem> = Sort::ALL.iter().map(|s| {
                let on = *s == br.sort;
                ListItem::new(Line::from(vec![
                    Span::styled(if on { "● " } else { "○ " }, Style::new().fg(if on { ACCENT } else { DIM })),
                    Span::styled(s.label(), Style::new().fg(TEXT)),
                ]))
            }).collect();
            popup(f, area, "Sort by", items, st, 40);
        }
        Some(Popup::Action(st, path)) => {
            let items: Vec<ListItem> = ACTIONS.iter().map(|a| ListItem::new(Line::from(vec![
                Span::styled("♪ ", Style::new().fg(ACCENT)), Span::styled(*a, Style::new().fg(TEXT)),
            ]))).collect();
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            popup(f, area, &trunc(&name, 38), items, st, 44);
        }
        None => {}
    }
}

/// Info about the song under the cursor: cover on top, tags below (mirrors `detail`).
fn peek_panel(f: &mut Frame, area: Rect, sel: Option<(std::path::PathBuf, String, bool, bool, u64)>, peek: &mut Option<Peek>, tick: usize) {
    let block = panel(title("Song info", false), false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let note = |f: &mut Frame, lines: Vec<Line<'static>>| {
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), center_v(inner, 3));
    };
    let Some((path, name, is_dir, audio, size)) = sel else { return };
    if is_dir {
        return note(f, vec![Line::from(Span::styled(name, Style::new().fg(ACCENT).bold())), Line::from(Span::styled("folder", Style::new().fg(MUTED)))]);
    }
    if !audio {
        return note(f, vec![Line::from(Span::styled(name, Style::new().fg(TEXT))), Line::from(Span::styled(format!("not an audio file · {}", human(size)), Style::new().fg(MUTED)))]);
    }
    let Some(p) = peek.as_mut().filter(|p| p.path == path) else {
        return note(f, vec![Line::from(Span::styled(format!("{} reading tags", SPIN[tick / 2 % 10]), Style::new().fg(DIM)))]);
    };

    let meta_h = 10u16.min(inner.height.saturating_sub(4));
    let art_h = inner.height.saturating_sub(meta_h + 1);
    let [art, _, meta] = Layout::vertical([Constraint::Length(art_h), Constraint::Length(1), Constraint::Length(meta_h)]).areas(inner);
    let aw = (art.height * 2).min(art.width);
    let art = Rect { x: art.x + (art.width - aw) / 2, width: aw, ..art };
    thumb(f, art, p.proto.as_mut(), "no cover");

    let t = &p.tags;
    let dash = |s: &str| if s.is_empty() { "—".to_string() } else { s.to_string() };
    let row = |k: &str, v: String| Line::from(vec![
        Span::styled(format!("{k:<11}"), Style::new().fg(MUTED)),
        Span::styled(v, Style::new().fg(TEXT)),
    ]);
    let title_txt = if t.get(Field::Title).is_empty() { name } else { t.get(Field::Title).to_string() };
    let track = match (t.get(Field::Track), t.get(Field::Disc)) {
        ("", _) => "—".to_string(),
        (n, "") => n.to_string(),
        (n, d) => format!("{n}  ·  disc {d}"),
    };
    let lines = vec![
        Line::from(Span::styled(title_txt, Style::new().fg(TEXT).bold())),
        Line::from(Span::styled(t.get(Field::Artist).to_string(), Style::new().fg(ACCENT))),
        Line::raw(""),
        row("Album", dash(t.get(Field::Album))),
        row("Released", dash(t.get(Field::Year))),
        row("Track", track),
        row("Length", if t.duration_ms > 0 { fmt_dur(t.duration_ms) } else { "—".into() }),
        row("Genre", dash(t.get(Field::Genre))),
        row("ISRC", dash(t.get(Field::Isrc))),
        row("Cover", p.dims.map(|(w, h)| format!("{w}×{h}  {}", kb(p.cover_len))).unwrap_or_else(|| "—".into())),
    ];
    f.render_widget(Paragraph::new(lines), meta);
}

fn popup(f: &mut Frame, area: Rect, label: &str, items: Vec<ListItem>, st: &mut ListState, w: u16) {
    let h = items.len() as u16 + 2;
    let w = w.min(area.width);
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + area.height.saturating_sub(h) / 2, width: w, height: h.min(area.height) };
    f.render_widget(Clear, r);
    let l = List::new(items)
        .block(panel(title(label, true), true))
        .highlight_style(Style::new().bg(SEL_BG).bold())
        .highlight_symbol("▍");
    f.render_stateful_widget(l, r, st);
}

// ---------------------------------------------------------------- shared bits
fn input_box(f: &mut Frame, area: Rect, label: &str, inp: &Input, active: bool, placeholder: &str) {
    let block = panel(title(label, active), active);
    let inner = block.inner(area);
    let line = if inp.text.is_empty() {
        Line::from(Span::styled(placeholder.to_string(), Style::new().fg(DIM).italic()))
    } else {
        Line::from(vec![Span::styled("❯ ", Style::new().fg(ACCENT)), Span::styled(inp.text.clone(), Style::new().fg(TEXT))])
    };
    f.render_widget(Paragraph::new(line).block(block), area);
    if active {
        let off = if inp.text.is_empty() { 0 } else { 2 + inp.cur as u16 };
        f.set_cursor_position(Position::new((inner.x + off).min(inner.right().saturating_sub(1)), inner.y));
    }
}

fn fmt_dur(ms: u64) -> String { format!("{}:{:02}", ms / 60000, ms / 1000 % 60) }

enum Cell { Plain, Changed, Missing }

struct GridCell {
    label: &'static str,
    value: String,
    kind: Cell,
}

/// Two-column grid of `label  value` rows; selection highlights a row.
fn grid(f: &mut Frame, area: Rect, cells: &[GridCell], sel: Option<usize>, editing: Option<&Input>) {
    let cols = Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)]).spacing(2).split(area);
    for (i, c) in cells.iter().enumerate() {
        let (col, row) = if i < LEFT_ROWS { (0, i) } else { (1, i - LEFT_ROWS) };
        let r = Rect { x: cols[col].x, y: cols[col].y + row as u16, width: cols[col].width, height: 1 };
        if r.y >= area.bottom() { continue; }
        cell_row(f, r, c, sel == Some(i), editing);
    }
}

fn cell_row(f: &mut Frame, r: Rect, c: &GridCell, selected: bool, editing: Option<&Input>) {
    let label_w = 13usize;
    let vw = (r.width as usize).saturating_sub(label_w + 2);
    let mut spans = vec![
        Span::styled(if selected { "▍" } else { " " }, Style::new().fg(ACCENT)),
        Span::styled(format!("{:<w$}", c.label, w = label_w), Style::new().fg(if selected { TEXT } else { MUTED })),
    ];
    match (selected, editing) {
        (true, Some(inp)) => {
            spans.push(Span::styled(trunc(&inp.text, vw), Style::new().fg(TEXT).underlined()));
            f.set_cursor_position(Position::new(r.x + 1 + label_w as u16 + (inp.cur as u16).min(vw as u16), r.y));
        }
        _ => {
            let (txt, st) = match c.kind {
                Cell::Missing => ("—".to_string(), Style::new().fg(DIM)),
                Cell::Changed => (format!("{} ✎", trunc(&c.value, vw.saturating_sub(2))), Style::new().fg(CHANGED).bold()),
                Cell::Plain => (trunc(&c.value, vw), Style::new().fg(TEXT)),
            };
            spans.push(Span::styled(txt, st));
        }
    }
    let mut line = Line::from(spans);
    if selected { line = line.style(Style::new().bg(SEL_BG)); }
    f.render_widget(Paragraph::new(line), r);
}

fn thumb(f: &mut Frame, area: Rect, proto: Option<&mut ratatui_image::protocol::StatefulProtocol>, placeholder: &str) {
    match proto {
        Some(p) => f.render_stateful_widget(StatefulImage::default().resize(Resize::Fit(None)), area, p),
        None => {
            f.render_widget(Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(DIM)), area);
            f.render_widget(Paragraph::new(placeholder.to_string()).style(Style::new().fg(DIM)).alignment(Alignment::Center),
                center_v(area.inner(Margin::new(1, 1)), 1));
        }
    }
}

fn current_cells(t: &Tags, dims: Option<(u32, u32)>, bytes: Option<usize>, with_len: bool) -> Vec<GridCell> {
    let mut v: Vec<GridCell> = Field::ALL.iter().map(|f| {
        let s = t.get(*f).to_string();
        GridCell { label: f.label(), kind: if s.is_empty() { Cell::Missing } else { Cell::Plain }, value: s }
    }).collect();
    v.push(GridCell {
        label: "Cover",
        kind: if dims.is_some() { Cell::Plain } else { Cell::Missing },
        value: dims.map(|(w, h)| format!("{w}×{h}  {}", kb(bytes.unwrap_or(0)))).unwrap_or_default(),
    });
    if with_len {
        v.push(GridCell {
            label: "Length",
            kind: if t.duration_ms > 0 { Cell::Plain } else { Cell::Missing },
            value: if t.duration_ms > 0 { fmt_dur(t.duration_ms) } else { String::new() },
        });
    }
    v
}

fn kb(n: usize) -> String { if n > 1 << 20 { format!("{:.1} MB", n as f64 / 1048576.0) } else { format!("{} KB", n / 1024) } }

/// Panel with tags on the right and optional cover on the left.
fn tags_panel(f: &mut Frame, area: Rect, label: &str, active: bool, cells: &[GridCell], wide: Option<GridCell>, btn: Option<(&str, bool)>, sel: Option<usize>, editing: Option<&Input>,
              art: Option<(&mut Option<ratatui_image::protocol::StatefulProtocol>, &str)>) {
    let block = panel(title(label, active), active);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let grid_area = if let Some((proto, ph)) = art {
        let aw = (inner.height * 2).min(inner.width / 3);
        let [a, g] = Layout::horizontal([Constraint::Length(aw), Constraint::Min(10)]).spacing(2).areas(inner);
        thumb(f, a, proto.as_mut(), ph);
        g
    } else { inner };
    // vertically centre the rows
    let rows = cells.len().saturating_sub(LEFT_ROWS).max(LEFT_ROWS) + wide.is_some() as usize;
    let g = center_v(grid_area, rows as u16);
    let wide_sel = sel == Some(FILE_ROW);
    grid(f, g, cells, sel.filter(|s| *s < FILE_ROW), if wide_sel { None } else { editing });
    if let Some(w) = wide {
        let r = Rect { y: g.y + rows as u16 - 1, height: 1, ..g };
        if r.y < grid_area.bottom() {
            match btn {
                Some((text, on)) => {
                    let bw = text.chars().count() as u16 + 4;
                    let (left, right) = (Rect { width: r.width.saturating_sub(bw + 1), ..r }, Rect { x: r.right().saturating_sub(bw), width: bw.min(r.width), ..r });
                    cell_row(f, left, &w, wide_sel, editing);
                    let st = if on { Style::new().fg(Color::Black).bg(ACCENT).bold() } else { Style::new().fg(ACCENT).bg(SEL_BG) };
                    f.render_widget(Paragraph::new(Span::styled(format!("  {text}  "), st)), right);
                }
                None => cell_row(f, r, &w, wide_sel, editing),
            }
        }
    }
}

// ---------------------------------------------------------------- search
fn search(f: &mut Frame, area: Rect, app: &mut App) {
    let [cur, inp, body] = Layout::vertical([Constraint::Length(8), Constraint::Length(3), Constraint::Min(4)]).areas(area);
    let cells = current_cells(&app.current, app.current_dims, app.current.cover.as_ref().map(|c| c.len()), true);
    let label = if app.current.is_empty() { "Current tags · none found" } else { "Current tags" };
    tags_panel(f, cur, label, false, &cells, None, None, None, None, Some((&mut app.current_proto, "no cover")));
    input_box(f, inp, "Search Spotify", &app.query, app.focus == Focus::Input, "song title artist…");

    let [left, right] = Layout::horizontal([Constraint::Percentage(44), Constraint::Percentage(56)]).areas(body);
    let active = app.focus == Focus::Results;

    if app.results.is_empty() {
        let msg = if app.searching { format!("{} Searching Spotify…", SPIN[app.tick / 2 % 10]) }
            else if app.searched { "Nothing found".to_string() }
            else { "Press ↵ to search Spotify with the query above".to_string() };
        f.render_widget(panel(title("Results", false), false), body);
        f.render_widget(Paragraph::new(msg).style(Style::new().fg(MUTED)).alignment(Alignment::Center), center_v(body.inner(Margin::new(1, 1)), 1));
        return;
    }

    let w = left.width.saturating_sub(6) as usize;
    let items: Vec<ListItem> = app.results.iter().map(|t| {
        ListItem::new(vec![
            Line::from(vec![
                Span::styled(trunc(&t.name, w.saturating_sub(2)), Style::new().fg(TEXT).bold()),
                Span::styled(if t.explicit { " E" } else { "" }, Style::new().fg(MUTED)),
            ]),
            Line::from(Span::styled(trunc(&format!("{} · {} · {}", t.artists.join(", "), t.album, t.year()), w), Style::new().fg(MUTED))),
        ])
    }).collect();
    let l = List::new(items)
        .block(panel(title(&format!("Results ({})", app.results.len()), active), active))
        .highlight_style(Style::new().bg(SEL_BG))
        .highlight_symbol("▍ ")
        .highlight_spacing(HighlightSpacing::Always);
    f.render_stateful_widget(l, left, &mut app.list);

    if let Some(t) = app.selected_track().cloned() {
        detail(f, right, &t, app);
    }
}

fn detail(f: &mut Frame, area: Rect, t: &Track, app: &mut App) {
    let local_ms = app.current.duration_ms;
    let block = panel(title("Track info", false), false);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let meta_h = 9u16.min(inner.height.saturating_sub(4));
    let art_h = inner.height.saturating_sub(meta_h + 1);
    let [art, _, meta] = Layout::vertical([Constraint::Length(art_h), Constraint::Length(1), Constraint::Length(meta_h)]).areas(inner);
    // square-ish art, centred: cells are about twice as tall as wide
    let aw = (art.height * 2).min(art.width);
    let art = Rect { x: art.x + (art.width - aw) / 2, width: aw, ..art };
    let proto = t.cover_url.as_ref().and_then(|u| app.previews.get_mut(u));
    let loading = format!("{} loading art", SPIN[app.tick / 2 % 10]);
    thumb(f, art, proto, if t.cover_url.is_some() { &loading } else { "no art" });

    let row = |k: &str, v: String| Line::from(vec![
        Span::styled(format!("{k:<11}"), Style::new().fg(MUTED)),
        Span::styled(v, Style::new().fg(TEXT)),
    ]);
    let lines = vec![
        Line::from(Span::styled(t.name.clone(), Style::new().fg(TEXT).bold())),
        Line::from(Span::styled(t.artists.join(", "), Style::new().fg(ACCENT))),
        Line::raw(""),
        row("Album", t.album.clone()),
        row("Released", t.release_date.clone()),
        row("Track", format!("{} / {}  ·  disc {}", t.track_number, t.total_tracks, t.disc_number)),
        length_line(t.duration_ms, local_ms),
        row("ISRC", t.isrc.clone().unwrap_or_else(|| "—".into())),
        row("Cover", if t.cover_px > 0 { format!("{0}×{0} (api) · hi-res on select", t.cover_px) } else { "—".into() }),
    ];
    f.render_widget(Paragraph::new(lines), meta);
}

/// Spotify runtime, plus how far it is from the local file (a quick sanity check
/// that the match is the same recording).
fn length_line(spotify_ms: u64, local_ms: u64) -> Line<'static> {
    let mut v = vec![
        Span::styled(format!("{:<11}", "Length"), Style::new().fg(MUTED)),
        Span::styled(fmt_dur(spotify_ms), Style::new().fg(TEXT)),
    ];
    if local_ms > 0 {
        let d = (spotify_ms as i64 - local_ms as i64) / 1000;
        let (txt, col) = if d.abs() <= 2 { ("≈ same".to_string(), ACCENT) } else { (format!("{}{}s", if d > 0 { "+" } else { "−" }, d.abs()), CHANGED) };
        v.push(Span::styled(format!("   file {}  ", fmt_dur(local_ms)), Style::new().fg(MUTED)));
        v.push(Span::styled(txt, Style::new().fg(col).bold()));
    }
    Line::from(v)
}

// ---------------------------------------------------------------- compare
fn orig_name(app: &App) -> String {
    app.cmp.as_ref().map(|c| c.orig_filename.clone()).unwrap_or_default()
}

fn compare(f: &mut Frame, area: Rect, app: &mut App) {
    let n = app.change_count();
    let [cur, new, note] = Layout::vertical([Constraint::Length(10), Constraint::Length(10), Constraint::Min(0)]).areas(area);

    let cells = current_cells(&app.current, app.current_dims, app.current.cover.as_ref().map(|c| c.len()), true);
    let file = GridCell { label: "Filename", value: orig_name(app), kind: Cell::Plain };
    tags_panel(f, cur, "Current tags", false, &cells, Some(file), None, None, None, Some((&mut app.current_proto, "no cover")));

    let c = app.cmp.as_mut().unwrap();
    let mut cells: Vec<GridCell> = Field::ALL.iter().map(|fl| {
        let v = c.new.get(*fl).to_string();
        let kind = if v.is_empty() && app.current.get(*fl).is_empty() { Cell::Missing }
            else if v != app.current.get(*fl) { Cell::Changed } else { Cell::Plain };
        GridCell { label: fl.label(), kind, value: v }
    }).collect();
    let (cv, ck) = match (&c.cover_dims, &c.cover_err) {
        (Some((w, h)), _) => (format!("{} {w}×{h}  {}", if c.apply_cover { "☑" } else { "☐" }, kb(c.cover_bytes.as_ref().map_or(0, |b| b.len()))),
            if c.apply_cover { Cell::Changed } else { Cell::Plain }),
        (None, Some(e)) => (trunc(e, 30), Cell::Missing),
        (None, None) if c.track.cover_url.is_some() => (format!("{} fetching best quality", SPIN[app.tick / 2 % 10]), Cell::Plain),
        _ => (String::new(), Cell::Missing),
    };
    cells.push(GridCell { label: "Cover", value: cv, kind: ck });
    // read-only: Spotify's runtime next to the file's
    let local = app.current.duration_ms;
    let sp_len = fmt_dur(c.track.duration_ms);
    let d = (c.track.duration_ms as i64 - local as i64) / 1000;
    cells.push(GridCell {
        label: "Length",
        kind: Cell::Plain,
        value: if local == 0 { sp_len } else if d.abs() <= 2 { format!("{sp_len}  ≈ same as file") } else { format!("{sp_len}  ({d:+}s vs file)") },
    });

    let label = format!("New tags · {}", if n == 0 { "no changes".to_string() } else { format!("{n} change{}", if n == 1 { "" } else { "s" }) });
    let sel = Some(c.sel);
    let editing = c.editing.as_ref();
    let orig = c.orig_filename.clone();
    let file = GridCell { label: "Filename", kind: if c.filename != orig { Cell::Changed } else { Cell::Plain }, value: c.filename.clone() };
    tags_panel(f, new, &label, true, &cells, Some(file), Some((if c.is_standard() { "☑ ARTIST - TITLE" } else { "☐ ARTIST - TITLE" }, c.sel == FORMAT_BTN)), sel, editing, Some((&mut c.cover_proto, "fetching…")));

    let [matched, _, buttons] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)]).areas(note);
    f.render_widget(Paragraph::new(Line::from(Span::styled(
        format!(" Matched: {} — {}  ({})", c.track.name, c.track.artists.join(", "), c.track.album),
        Style::new().fg(MUTED),
    ))), matched);

    let btn = |label: &str, idx: usize, color: Color| {
        let on = c.sel == idx;
        Span::styled(format!("  {label}  "), if on { Style::new().fg(Color::Black).bg(color).bold() } else { Style::new().fg(color).bg(SEL_BG) })
    };
    f.render_widget(Paragraph::new(Line::from(vec![
        Span::raw(" "), btn("✔ Save", SAVE_BTN, ACCENT), Span::raw("   "), btn("✕ Cancel", CANCEL_BTN, BAD),
    ])), buttons);

    if c.confirm {
        let tag_changes = Field::ALL.iter().filter(|f| c.new.get(**f) != app.current.get(**f)).count();
        let mut lines = vec![
            Line::from(Span::styled(format!("Write changes to {}?", trunc(&c.orig_filename, 40)), Style::new().fg(TEXT).bold())),
            Line::raw(""),
        ];
        if tag_changes > 0 { lines.push(Line::from(Span::styled(format!("  • {tag_changes} tag field{}", if tag_changes == 1 { "" } else { "s" }), Style::new().fg(CHANGED)))); }
        if c.apply_cover && c.cover_bytes.is_some() { lines.push(Line::from(Span::styled("  • cover art (hi-res)", Style::new().fg(CHANGED)))); }
        if c.filename != c.orig_filename { lines.push(Line::from(Span::styled(format!("  • rename → {}", trunc(&c.filename, 40)), Style::new().fg(CHANGED)))); }
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(" y ", Style::new().fg(Color::Black).bg(ACCENT).bold()), Span::styled(" Save   ", Style::new().fg(TEXT)),
            Span::styled(" n ", Style::new().fg(Color::Black).bg(BAD).bold()), Span::styled(" Cancel", Style::new().fg(TEXT)),
        ]));
        let h = lines.len() as u16 + 2;
        let w = 60.min(area.width);
        let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + area.height.saturating_sub(h) / 2, width: w, height: h.min(area.height) };
        f.render_widget(Clear, r);
        let block = panel(title("Confirm", true), true);
        let inner = block.inner(r);
        f.render_widget(block, r);
        f.render_widget(Paragraph::new(lines), inner);
    }

    if let Some(start) = c.saving {
        saving_overlay(f, area, app.tick, start.elapsed(), app.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    }
}

/// Indeterminate progress: lofty gives no byte progress, so show activity + elapsed time.
fn saving_overlay(f: &mut Frame, area: Rect, tick: usize, elapsed: std::time::Duration, file: String) {
    let w = 54.min(area.width);
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + area.height.saturating_sub(7) / 2, width: w, height: 7.min(area.height) };
    f.render_widget(Clear, r);
    let block = panel(title("Saving", true), true);
    let inner = block.inner(r);
    f.render_widget(block, r);
    let bar_w = inner.width.saturating_sub(2) as usize;
    let seg = (bar_w / 4).max(3);
    let span = bar_w.saturating_sub(seg).max(1);
    let t = tick % (span * 2);
    let pos = if t < span { t } else { span * 2 - t };
    let bar: String = (0..bar_w).map(|i| if i >= pos && i < pos + seg { '█' } else { '░' }).collect();
    let lines = vec![
        Line::from(vec![Span::styled(format!("{} ", SPIN[tick / 2 % 10]), Style::new().fg(ACCENT).bold()), Span::styled("Writing tags & cover art…", Style::new().fg(TEXT).bold())]),
        Line::from(Span::styled(trunc(&file, inner.width as usize), Style::new().fg(MUTED))),
        Line::raw(""),
        Line::from(Span::styled(bar, Style::new().fg(ACCENT))),
        Line::from(Span::styled(format!("{:.1}s — don't close the terminal", elapsed.as_secs_f32()), Style::new().fg(DIM))),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}
