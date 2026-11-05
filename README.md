# songos

A TUI for managing your music library. Right now it does one thing well: **fixing song tags and cover art using Spotify**.

Browse your files like a normal file manager, pick a song, search Spotify for it, compare the current tags against Spotify's side by side, tweak anything you like, and write it back to the file, including the highest-quality album art available.

## Features

- **File-manager home screen** with two layouts you can flip between with `P`:
  - *Detailed*: bordered table with name, size and modified date.
  - *Clean*: borderless, artist on the left and title on the right, plus a side panel with the cover and tags of the song under the cursor.
- Sort popup (name, modified, size, type), `/` to jump to a match, `f` to find (recursive search below the current folder). Your sort order and layout are remembered.
- **Spotify search** prefilled from the file's existing tags, with album art and track info for every result, including runtime next to the local file's runtime so you can tell when you've matched the wrong version.
- **Review screen**: current tags on top, new tags below. Edit any field, choose whether to replace the cover, and rename the file (with a one-key `ARTIST - TITLE` toggle).
- Saves in the background with a progress indicator and a confirmation step first.
- Cover art is fetched at the best resolution available (often 2000×2000 or more, not Spotify's 640px API version).
- Supported files: `mp3`, `flac`, `m4a`, `ogg`, `opus`, `wav`, `aac`.

## Installation

You need a [Rust toolchain](https://rustup.rs) (1.85 or newer; the project uses the 2024 edition).

```sh
git clone https://github.com/rd6260/songos.git
cd songos
cargo install --path .
```

Or straight from GitHub without cloning:

```sh
cargo install --git https://github.com/rd6260/songos
```

Both put a `songos` binary in `~/.cargo/bin`, so make sure that's on your `PATH`.

To try it without installing: `cargo run --release`.

### Terminal

Album art is drawn with whichever image protocol your terminal supports (Kitty, Sixel, iTerm2), and falls back to coloured half-blocks everywhere else. Kitty, WezTerm, Ghostty and foot give the sharpest results. A terminal of at least 25 rows works best for the review screen.

## Configuration

songos needs Spotify API credentials.

1. Create an app at the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard). Any redirect URI will do; it isn't used.
2. Copy its **Client ID** and **Client secret**.
3. Create `~/.config/songos/config.json`:

```json
{
  "spotify_client_id": "your-client-id",
  "spotify_client_secret": "your-client-secret",
  "music_dir": "~/Music"
}
```

`music_dir` is optional and is where the file browser starts (default `~/Music`).

songos also keeps `~/.config/songos/state.json` to remember your sort order and layout.

## Usage

```sh
songos                     # start in your music folder
songos path/to/folder      # start in a folder
songos path/to/song.flac   # start in its folder with the song selected
```

Select a song, press `Enter`, choose **Tag from Spotify**, press `Enter` to search, pick a track, review, save.

## Keys

### File browser

| Key | Action |
| --- | --- |
| `↑` `↓` / `j` `k` | Move |
| `PgUp` `PgDn` / `Ctrl-u` `Ctrl-d` | Move by 10 |
| `g` / `G` | Top / bottom |
| `Enter` / `l` / `→` | Open folder, or open the action menu on a song |
| `h` / `←` / `Backspace` | Parent folder |
| `s` | Sort menu |
| `/` | Jump to the first match (nothing is hidden); `n` / `N` for next / previous |
| `f` | Find: recursive search of this folder and everything below; `Enter` keeps the results, `Esc` clears them |
| `P` | Toggle detailed / clean layout |
| `u` | Refresh / rescan |
| `.` | Show / hide hidden files |
| `q` | Quit |

`/` and `f` match the file name (for `f`, the path relative to the current folder) and, once loaded, the artist and title.

### Spotify search

| Key | Action |
| --- | --- |
| `Enter` (in the query) | Search Spotify |
| `↓` / `Tab` | Move to the results |
| `↑` `↓` / `j` `k` | Browse results |
| `Enter` (on a result) | Review this track |
| `/` | Back to the query box |
| `Esc` | Query box, or back to the browser from the query |

### Review

| Key | Action |
| --- | --- |
| `↑` `↓` `←` `→` | Move between fields (columns are independent; `↓` from the last row goes to the filename, then the buttons) |
| `Enter` / `e` | Edit the selected field or filename |
| `Space` / `Enter` on Cover | Toggle replacing the cover |
| `r` | Reset the selected field to Spotify's value |
| `→` on the filename row, then `Enter` | Toggle the `ARTIST - TITLE` name (uses the album artist) |
| `s` or the **Save** button | Save, then confirm with `y` / `n` |
| **Cancel** button | Discard and return to the browser |
| `Esc` | Back to the search results to pick a different track |

## Notes

- **Files are modified in place** once you confirm. There is no backup, so keep one if the library matters.
- **Genre**: Spotify no longer returns genres, so it can't be filled in automatically. The field is editable and keeps whatever the file already has.
- **Cover quality**: the Spotify API only lists images up to 640×640. songos requests the original upload from Spotify's image server by changing the URL prefix, which Spotify doesn't document, and falls back to the 640px image if that fails.
- **tmux**: the startup image-protocol probe waits 2 seconds if the terminal never answers (tmux without `allow-passthrough`). Set `SONGOS_NO_IMAGE_PROBE=1` to skip it and use the half-block fallback.
- Spotify's client-credentials flow is used, so no login is required, and only public catalogue data is read.
