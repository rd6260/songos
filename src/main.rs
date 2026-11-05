mod app;
mod browser;
mod config;
mod lrclib;
mod spotify;
mod tags;
mod ui;

use anyhow::Result;
use app::App;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui_image::picker::Picker;
use std::time::Duration;

fn main() -> Result<()> {
    let cfg = match config::Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("songos: {e:#}\nexpected {{\"spotify_client_id\": …, \"spotify_client_secret\": …}} in {}", config::Config::path().display());
            std::process::exit(1);
        }
    };
    let direct = std::env::args().nth(1).map(std::path::PathBuf::from);

    let mut terminal = ratatui::init();
    // Must happen after entering the alternate screen, before reading events.
    let picker = detect_picker();
    let mut app = App::new(cfg, picker, direct);

    let res = (|| -> Result<()> {
        while !app.quit {
            app.pump();
            app.update_peek();
            terminal.draw(|f| ui::draw(f, &mut app))?;
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind == KeyEventKind::Press { app.key(k); }
                }
            }
            app.tick = app.tick.wrapping_add(1);
        }
        Ok(())
    })();
    ratatui::restore();
    res
}

/// Probe the terminal for its image protocol and font size.
///
/// ratatui-image's probe gives up after 2s when the terminal never answers
/// (e.g. inside tmux without passthrough) but leaves a thread blocked on stdin,
/// which would swallow our first keypress. In that case we poke the terminal
/// with a status request, whose reply is what that thread is waiting for.
fn detect_picker() -> Picker {
    use std::io::Write;
    if std::env::var_os("SONGOS_NO_IMAGE_PROBE").is_some() {
        return Picker::halfblocks();
    }
    let start = std::time::Instant::now();
    let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    if start.elapsed() >= Duration::from_millis(1900) {
        let mut out = std::io::stdout();
        let _ = out.write_all(b"\x1b[5n");
        let _ = out.flush();
        // discard anything that still leaks through as key events
        while event::poll(Duration::from_millis(250)).unwrap_or(false) {
            let _ = event::read();
        }
    }
    picker
}
