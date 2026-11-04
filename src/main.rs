mod app;
mod config;
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
    let picker = if std::env::var("SONGOS_NOQUERY").is_ok() {
        Picker::halfblocks()
    } else {
        Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
    };
    let mut app = App::new(cfg, picker);
    if let Some(p) = direct {
        app.direct = true;
        app.load_song(p);
    }

    let res = (|| -> Result<()> {
        while !app.quit {
            app.pump();
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
