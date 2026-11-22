# am — a quiet terminal front end for Apple Music

Status: **approved and implemented (M1–M6)** · 2026-11-22. Section 12 records what building it changed.

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
| `.` / `Ctrl-N` | (in pop-up) play next |
| `Q` / `Ctrl-A` | (in pop-up) add to queue |
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

Music.app's scripting can't see or change Up Next, so `am` keeps its own
playlist named **`am · Queue`**. It's hidden from `am`'s Playlists tab.

- **Playing an album, an artist or search results:** fill the queue with
  the chosen song and everything after it, then `play` the queue
  playlist.
- **Play next / add to queue:** rebuild the queue as
  [current song, new order…], `play` it, jump back to the same position,
  and pause again if it was paused.
- **Playing a saved playlist from song *n*:** `play` the playlist, wait
  0.3 s, then skip forward *n − 1* times.
- **Queue tab:** lists `am · Queue` from the current song onward. Enter
  jumps to a song by rebuilding the queue from that song.
- **Size limit:** the queue holds at most 200 songs. Filling it costs
  ~10 ms per song.

Section 12 explains why it works this way instead of the simpler
"insert into the playlist" first planned.

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
├── docs/plans/           # per-milestone plans
├── examples/live_check.rs  # end-to-end test against the real Music.app
└── src/
    ├── main.rs           # terminal setup/restore (also on panic), event loop
    ├── app.rs            # App state + key handling (pure logic, unit-tested)
    ├── popup.rs          # library pop-up: tabs, rows, Enter actions (tested)
    ├── library.rs        # in-memory library + search (tested)
    ├── art.rs            # artwork decoding, half-block colours (tested)
    ├── worker.rs         # background thread, Command/Update channels
    ├── check.rs          # `am --check` read-only self-test
    ├── music/
    │   ├── mod.rs        # types: Track, Song, PlayerStatus, MusicError…
    │   ├── osascript.rs  # run a script, map error codes to MusicError
    │   ├── scripts.rs    # all AppleScript text in one place
    │   ├── parse.rs      # split delimited output into structs (tested)
    │   └── queue.rs      # library/queue/AirPlay/artwork commands
    └── ui/
        ├── mod.rs        # top-level draw()
        ├── now_playing.rs
        ├── popup.rs
        └── help.rs
```

Crates used:
- `ratatui` for the terminal UI. Its bundled `crossterm` handles input.
- `anyhow` and `thiserror` for errors.
- `image` (JPEG and PNG only) for album art.

Search is about 30 lines in `library.rs` rather than a fuzzy-search
crate. For a few thousand songs it's instant, and it's easier to read.

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

## 12. What building it changed (found by testing against Music.app)

`examples/live_check.rs` plays, queues and skips at volume 0 and checks
Music's real state after each step. It turned up these behaviours. Each is
also a "Found by testing" comment in `src/music/scripts.rs`.

1. **`play track N of playlist P` plays that one song outside the
   playlist.** `next` then goes somewhere unrelated. Playing the playlist
   itself keeps `next` inside it.
2. **`next track` sent right after `play` is ignored.** A 0.3 s `delay`
   first fixes it.
3. **Music fixes the play order when a playlist starts.** Songs added to a
   playing playlist are ignored, so queue edits rebuild the queue and
   restart it at the same position.
4. **Shuffle and repeat read back their old value for a moment** after
   being changed. The worker waits 300 ms after running commands before
   polling.
5. **Reserved words.**
   - `st` is reserved by AppleScript (ordinals like `1st`).
   - `names` and `active` are reserved by Music's dictionary.

   Variables avoid all three.

Measured on a 1,583-song library:
- loading the library: ~0.5 s;
- status poll: ~0.15 s;
- AirPlay list: ~1 s;
- filling the queue: ~10 ms per song.
