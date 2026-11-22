# M1 — Now playing + play/pause/next

**Spec:** [DESIGN.md](../../DESIGN.md) §5.1, §6.1, §6.2, §8 (M1 row)

**Goal:** `cargo run` opens a focus screen showing the current track, a
live progress bar and play state. `space` / `n` / `p` / `q` work.
Music.app is launched hidden if it isn't running, and a missing
Automation permission gets a clear help screen.

## Interfaces

- `music::Music::status() -> Result<StatusReply, MusicError>`. This is one
  `osascript` call. It returns `StatusReply::NotRunning` without launching
  Music, or `StatusReply::Status(PlayerStatus)`.
- `music::Music::{play_pause, next, previous, launch_hidden}() -> Result<(), MusicError>`
- `music::parse::parse_status(&str) -> Result<StatusReply, MusicError>`:
  - fields are separated by ASCII 31;
  - `NOT_RUNNING` is a sentinel meaning Music isn't open;
  - `,` and `.` are both accepted as the decimal separator.
- `music::osascript::classify_error(&str) -> MusicError`:
  - `(-1743)` maps to `PermissionDenied`;
  - `(-600)` and `(-609)` map to `NotRunning`;
  - anything else maps to `Script`.
- `worker::spawn(Music) -> (Sender<Command>, Receiver<Update>)`. The
  worker polls every 1 s and also right after each command. If Music
  isn't running on the *first* poll, it launches it hidden. After that it
  only reports `NotRunning`, and `o` opens Music.
- `app::App`:
  - `handle_key(KeyEvent) -> Option<Command>`;
  - `apply(Update, Instant)`;
  - `position(Instant) -> f64` (interpolated, clamped to duration).

## Tasks

1. **Parsing and error classification.** Tests first:
   - output when playing, paused and stopped;
   - comma decimals;
   - the `NOT_RUNNING` sentinel;
   - malformed output, which becomes `BadOutput`;
   - each error code mapping.
2. **App state.** Tests for:
   - the key-to-command mapping (`space`, `n`, `p`, `o`, `q`; other keys
     do nothing);
   - interpolation: advances while playing, frozen while paused, never
     past the duration;
   - `format_time` (e.g. `276.4` gives `4:36`).
3. **Worker thread, focus screen UI, `main` loop.** The terminal must be
   restored on quit and on panic, using `ratatui::init` and `restore`.
4. **Repo hygiene:**
   - `.gitignore`;
   - README with Rust setup, the PATH line and the Automation permission;
   - MIT LICENSE;
   - commits.

## Review focus

- **The first run blocks on the macOS permission prompt.** The UI must
  stay responsive and say "Waiting for Music.app… (look for a permission
  prompt)" after 2 s with no reply.
- **Music quits while `am` is open.** Show "Music isn't running, press `o`
  to open it". Do not relaunch it silently.
- **A streamed or radio track with missing fields.** Don't crash; show
  whatever is known.
- **A tiny terminal window.** Don't panic; draw what fits.
- **Song titles with unusual characters** (quotes, emoji, non-Latin
  scripts). They're displayed as-is.
