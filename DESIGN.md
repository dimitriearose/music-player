# am — a quiet terminal front end for Apple Music

Status: **approved** · 2026-11-22

## 1. Intent

A distraction-free, keyboard-driven terminal UI for Music.app on macOS.
Music.app does the actual playback in the background; `am` is the only
thing you look at. It should have "every feature, but quiet": no
recommendations, no Home/Radio/Browse noise, nothing on screen you didn't
ask for.

It is also a **learning project**: the owner is new to Rust, the terminal
and git, so the code is written to be read. Comments explain *why*, and
Rust concepts (ownership, `Result`, `match`, traits, threads, channels)
are explained where they first appear.

## 2. Goals, non-goals, anti-goals

**Goals**
- Focus screen showing only what's playing, with a progress bar.
- Full control from the keyboard: play/pause, next/previous, seek, volume,
  shuffle, repeat.
- Browse and search *your library* (songs, albums, artists, playlists)
  from a pop-up that disappears when you're done.
- Queue ("play next" / "add to queue"), AirPlay speaker selection, album
  art.
- Never freezes: slow Music.app calls never block typing or drawing.

**Non-goals (for now)**
- Lyrics: Music.app only exposes lyrics embedded in files, so they'd
  usually be empty.
- A one-shot CLI (`am next`, `am status --json`). The architecture leaves
  room to add it later.
- Apple Music catalog search. AppleScript can't do it.
- MusicKit or the web API, and other OSes.

**Anti-goals (failure even if it "works")**
- Music.app's window popping up in your face.
- A busy, colourful, cluttered screen.
- The UI hanging while waiting on Music.app.
- Any personal data (your library, logs, your email address) ending up in
  the git repository.

## 3. Constraints

- macOS on Apple Silicon. Terminal.app is the primary terminal.
- Terminal.app has no inline-image protocol. Album art is drawn with
  coloured half-block characters (`▀`). Colours are chosen to look right
  in Terminal.app's palette.
- The library is small (hundreds to a few thousand songs). Loading it all
  into memory at startup is fine, and no disk cache is needed.
- Music.app's scripting has **no access to Up Next**. See §6.3.

## 4. Approaches considered

| | How Rust talks to Music.app | Trade-off |
|---|---|---|
| **A (chosen)** | Run `/usr/bin/osascript` with AppleScript text and parse its printed output | Simple and readable; every script can be tested in Script Editor. ~50–100 ms per call, which is fine at 1 call/second and a small library |
| B | Run AppleScript in-process via `NSAppleScript` (`objc2` crates) | ~5 ms per call, but a lot of `unsafe` Objective-C code. Too heavy for a first Rust project |
| C | `osascript -l JavaScript` (JXA) returning JSON | Structured output, but poorly documented for Music.app, and it means learning two languages |

**A** was chosen. All Music.app code lives behind one module
(`src/music/`), so it could be swapped for B later without touching the
UI.

TUI stack: **ratatui** (drawing widgets) + **crossterm** (keyboard input,
raw mode, alternate screen). This is the standard Rust pairing.

## 5. Look and keys

### 5.1 Focus screen (always the base view)

```
                     Bloodbuzz Ohio
                The National — High Violet
                     [ half-block art ]

        1:42 ━━━━━━━━━━━━━━━━──────────────── 4:36

               ▶ playing   vol 60   ⤮ shuffle   ↻ all

                                          (dim status / error line)
```

Everything is centred, and secondary text is dim. Errors and notices
appear on the bottom line and fade after ~4 seconds.

### 5.2 Library pop-up (`/`)

```
 ┌ Search · Playlists · Albums · Artists · Queue · Speakers ┐
 │ > blood_                                                  │
 │   ♪ Bloodbuzz Ohio — The National                         │
 │   ◎ Blood Bank (EP) — Bon Iver                            │
 └───────────────────────────────────────────────────────────┘
```

`Tab`/`Shift-Tab` switches tabs. `Enter` plays, or opens an album or
playlist so you can see its tracks. `Esc` goes back or closes. The
Speakers tab arrives in M5, and Queue in M4.

### 5.3 Keys (vim keys and arrows both work)

| Key | Action |
|---|---|
| `space` | play / pause |
| `n` / `p` | next / previous track |
| `←` `→` or `h` `l` | seek −/+ 5 s |
| `+` / `-` | volume ±5 |
| `s` / `r` | toggle shuffle / cycle repeat (off → all → one) |
| `a` | show / hide album art |
| `/` | open library pop-up |
| `j` `k` / `↓` `↑` | move in lists |
| `.` | (in pop-up) play next · `Q` add to queue |
| `?` | key help overlay |
| `q` | quit (Music keeps playing) |

There is no `:` command mode.

## 6. Architecture

### 6.1 Threads

```
 main thread                               music worker thread
 ───────────                               ───────────────────
 read keys (crossterm) ─┐                   loop {
 update App state       │  Command  ───▶      run queued commands
 draw with ratatui      │  (channel)          every 1 s: fetch status
 every ~100 ms tick ◀───┘  ◀─── Update        send Update back
                                            }
```

- Two threads talk over `std::sync::mpsc` channels. This is plain
  `std::thread`, not async/tokio, which is easier to learn and enough here.
- The main thread **never** calls `osascript` itself, so the UI can't
  freeze.
- One status fetch per second uses **one** `osascript` call that returns
  everything at once: state, title, artist, album, duration, position,
  volume, shuffle, repeat and track ID.
- The progress bar is interpolated locally between fetches: last known
  position + time elapsed while playing.
- The library is loaded once, in the background, after startup. Search
  runs in Rust over that in-memory copy, so it's instant.

### 6.2 Talking to Music.app

- Output format: fields are separated by ASCII 31 (unit separator) and
  records by ASCII 30. In AppleScript that's `character id 31`. Song titles
  never contain these characters, so splitting is safe.
- Errors are mapped to a Rust `enum MusicError`:
  - `NotRunning`
  - `PermissionDenied` (osascript error **-1743**)
  - `Script(message)`
- **Music not running:** check `application "Music" is running`. This
  check doesn't launch Music. If it isn't running, launch it with
  `open -g -j -a Music` (background, hidden). If the window still appears,
  hide it via System Events. (That costs a second, one-time permission
  prompt, so it's only a fallback.)
- **Permission denied:** show a full-screen help panel explaining System
  Settings → Privacy & Security → Automation → Terminal → Music.

### 6.3 Queue, albums, playlists

Music.app can't read or change Up Next, so `am` keeps its own playlist
named **`am · Queue`**:

- **Play an album or song list:** clear `am · Queue`, add the tracks in
  order, play the playlist.
- **Play next / add to queue:** insert or append tracks in `am · Queue`.
- **Queue tab:** lists `am · Queue`, starting at the current track.
- **Library views:** hide the playlist, because it's an implementation
  detail.
- **Trade-off:** while you use `am`, Music's own Up Next isn't used.
- **Playing a saved playlist:** plays that playlist directly.

### 6.4 Album art (M6)

When the track ID changes, AppleScript writes `raw data of artwork 1` to
a temp file in `$TMPDIR`. Rust decodes it (`image` crate), scales it, and
draws it with half-blocks. If there's no artwork (common for some streamed
songs), nothing is drawn and the layout doesn't jump.

## 7. Module layout

```
music-player/
├── Cargo.toml            # crate "am", binary "am"
├── Cargo.lock            # committed (this is an app, not a library)
├── README.md             # setup, permissions, keys, git basics
├── DESIGN.md             # this file
├── .gitignore
└── src/
    ├── main.rs           # terminal setup/restore (also on panic), event loop
    ├── app.rs            # App state + key handling (pure logic, unit-tested)
    ├── worker.rs         # background thread, Command/Update channels
    ├── library.rs        # in-memory library + search
    ├── music/
    │   ├── mod.rs        # public API: Music, Track, PlayerStatus, MusicError
    │   ├── osascript.rs  # run a script, map exit codes to MusicError
    │   ├── scripts.rs    # all AppleScript text in one place
    │   └── parse.rs      # split delimited output into structs (unit-tested)
    └── ui/
        ├── mod.rs        # top-level draw()
        ├── now_playing.rs
        ├── popup.rs      # library / queue / speakers tabs
        ├── help.rs
        └── art.rs
```

Expected crates:
- `ratatui` and `crossterm` for the terminal UI.
- `anyhow` for errors in `main`, and `thiserror` for `MusicError`.
- `nucleo-matcher` (or `fuzzy-matcher`) for search, from M3.
- `image` for album art, from M6.

## 8. Milestones

Each milestone ends in something you can run. Work stops after each one
so you can try it.

| # | Delivers |
|---|---|
| **M1** | Project + git + README. Focus screen: title/artist/album, progress bar, play state. Keys `space` `n` `p` `q`. Worker thread with 1 s polling. Music-not-running (launch hidden) and permission-denied screens |
| **M2** | Seek, volume, shuffle, repeat, `?` help overlay, fading status line |
| **M3** | Library pop-up: Search / Playlists / Albums / Artists, background library load, fuzzy search, play song / album / playlist (via `am · Queue`) |
| **M4** | Queue: play next, add to queue, Queue tab |
| **M5** | AirPlay: Speakers tab, toggle output devices |
| **M6** | Album art (half-blocks, `a` toggle) |

## 9. Privacy and repository hygiene

This project has **no secrets**: no API keys, no tokens and no accounts,
because AppleScript talks to the local Music.app. A `.env` file would
have nothing to hold, so there isn't one. The things that *could* leak
are personal data, and these are the guards against that:

- **`.gitignore`** excludes:
  - build output (`/target`) and macOS clutter (`.DS_Store`);
  - editor folders;
  - logs (`*.log`), temp art files and any `.env*` file, in case one is
    ever added.
- **Commit email:** git stamps every commit with an email address, and
  that address becomes public when you push. The README shows how to use
  GitHub's private `…@users.noreply.github.com` address for this repo.
- **Tests use made-up data**, never your real library.
- **Debug logging** (if enabled with `AM_DEBUG=1`) writes to
  `~/Library/Logs/am/`, outside the repository, so library contents can't
  be committed by accident.
- **GitHub settings:** the README suggests turning on secret scanning and
  push protection.

## 10. Testing

- Unit tests (`cargo test`) cover:
  - output parsing;
  - key handling and state changes in `App`;
  - search ranking;
  - progress interpolation.
- Music.app interaction is checked by hand at each milestone. The README
  includes `osascript` one-liners you can run yourself to see what the app
  sees.

## 11. Left to the builder (calls already made)

- License: MIT, so others may use the code once the repo is public.
- Seek step 5 s, volume step 5, status poll 1 s, UI tick 100 ms.
- Colours: default terminal foreground plus one dim grey and one accent.
  No truecolor assumed.
- Exact fuzzy-search crate and album-art scaling method.
