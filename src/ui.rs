use crate::app::*;
use crate::browser::{ACTIONS, Mode, Popup, Sort};
use crate::spotify::Track;
use crate::tags::{Field, Tags};
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Position, Rect};
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui_image::{Resize, StatefulImage};

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
    if app.screen == Screen::Browser { push(app.br.cwd.display().to_string()); }
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
                _ => key_hint(&[("↑↓", "move"), ("↵/l", "open"), ("h", "up"), ("s", "sort"), ("/", "go to"), ("n/N", "next/prev"), ("f", "filter"), (".", "hidden"), ("q", "quit")]),
            },
            Screen::Search => match app.focus {
                Focus::Input => key_hint(&[("↵", "search spotify"), ("↓", "results"), ("esc", "back")]),
                Focus::Results => key_hint(&[("↑↓", "browse"), ("↵", "pick track"), ("tab", "edit query"), ("esc", "query")]),
            },
            Screen::Compare => {
                let c = app.cmp.as_ref().unwrap();
                if c.confirm { key_hint(&[("y/↵", "write to file"), ("any", "cancel")]) }
                else if c.editing.is_some() { key_hint(&[("↵", "accept"), ("esc", "cancel")]) }
                else { key_hint(&[("↑↓←→", "move"), ("↵", "edit"), ("r", "spotify value"), ("c", "keep current"), ("w", "save"), ("esc", "back")]) }
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
    let empty = items.is_empty();
    let list = List::new(items).block(block).highlight_style(Style::new().bg(SEL_BG).bold()).highlight_symbol("▍");
    f.render_stateful_widget(list, list_area, &mut br.list);
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
        let r = Rect { x: cols[col].x, y: cols[col].y + row as u16 * 1, width: cols[col].width, height: 1 };
        if r.y >= area.bottom() { continue; }
        let selected = sel == Some(i);
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

fn current_cells(t: &Tags, dims: Option<(u32, u32)>, bytes: Option<usize>) -> Vec<GridCell> {
    let mut v: Vec<GridCell> = Field::ALL.iter().map(|f| {
        let s = t.get(*f).to_string();
        GridCell { label: f.label(), kind: if s.is_empty() { Cell::Missing } else { Cell::Plain }, value: s }
    }).collect();
    v.push(GridCell {
        label: "Cover",
        kind: if dims.is_some() { Cell::Plain } else { Cell::Missing },
        value: dims.map(|(w, h)| format!("{w}×{h}  {}", kb(bytes.unwrap_or(0)))).unwrap_or_default(),
    });
    v
}

fn kb(n: usize) -> String { if n > 1 << 20 { format!("{:.1} MB", n as f64 / 1048576.0) } else { format!("{} KB", n / 1024) } }

/// Panel with tags on the right and optional cover on the left.
fn tags_panel(f: &mut Frame, area: Rect, label: &str, active: bool, cells: &[GridCell], sel: Option<usize>, editing: Option<&Input>,
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
    // vertically centre the 5 rows
    let g = center_v(grid_area, LEFT_ROWS as u16);
    grid(f, g, cells, sel, editing);
}

// ---------------------------------------------------------------- search
fn search(f: &mut Frame, area: Rect, app: &mut App) {
    let [cur, inp, body] = Layout::vertical([Constraint::Length(7), Constraint::Length(3), Constraint::Min(4)]).areas(area);
    let cells = current_cells(&app.current, app.current_dims, app.current.cover.as_ref().map(|c| c.len()));
    let label = if app.current.is_empty() { "Current tags · none found" } else { "Current tags" };
    tags_panel(f, cur, label, false, &cells, None, None, None);
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
    let block = panel(title("Track info", false), false);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let meta_h = 10u16.min(inner.height.saturating_sub(4));
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
    let bar = |p: u32| format!("{}{} {p}", "█".repeat((p / 10) as usize), "░".repeat(10 - (p / 10) as usize));
    let lines = vec![
        Line::from(Span::styled(t.name.clone(), Style::new().fg(TEXT).bold())),
        Line::from(Span::styled(t.artists.join(", "), Style::new().fg(ACCENT))),
        Line::raw(""),
        row("Album", t.album.clone()),
        row("Released", t.release_date.clone()),
        row("Track", format!("{} / {}  ·  disc {}", t.track_number, t.total_tracks, t.disc_number)),
        row("Length", fmt_dur(t.duration_ms)),
        row("ISRC", t.isrc.clone().unwrap_or_else(|| "—".into())),
        row("Popularity", bar(t.popularity)),
        row("Cover", if t.cover_px > 0 { format!("{0}×{0} (api) · hi-res on select", t.cover_px) } else { "—".into() }),
    ];
    f.render_widget(Paragraph::new(lines), meta);
}

// ---------------------------------------------------------------- compare
fn compare(f: &mut Frame, area: Rect, app: &mut App) {
    let n = app.change_count();
    let [cur, new, note] = Layout::vertical([Constraint::Length(9), Constraint::Length(9), Constraint::Min(0)]).areas(area);

    let cells = current_cells(&app.current, app.current_dims, app.current.cover.as_ref().map(|c| c.len()));
    tags_panel(f, cur, "Current tags", false, &cells, None, None, Some((&mut app.current_proto, "no cover")));

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

    let label = format!("New tags · {}", if n == 0 { "no changes".to_string() } else { format!("{n} change{}", if n == 1 { "" } else { "s" }) });
    let sel = Some(c.sel);
    let editing = c.editing.as_ref();
    tags_panel(f, new, &label, true, &cells, sel, editing, Some((&mut c.cover_proto, "fetching…")));

    let msg = if c.confirm {
        let file = app.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        Line::from(vec![
            Span::styled(format!(" Write {n} change(s) to "), Style::new().fg(CHANGED).bold()),
            Span::styled(file, Style::new().fg(TEXT)),
            Span::styled("?  y / n", Style::new().fg(CHANGED).bold()),
        ])
    } else {
        Line::from(Span::styled(
            format!(" Matched: {} — {}  ({})", c.track.name, c.track.artists.join(", "), c.track.album),
            Style::new().fg(MUTED),
        ))
    };
    f.render_widget(Paragraph::new(msg), note);
}
