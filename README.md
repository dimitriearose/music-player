# am — a quiet Apple Music player for the terminal

`am` is a minimal, keyboard-driven screen for Music.app on macOS. Music.app
does the playing in the background; `am` shows you only what's playing and
lets you control it without touching the mouse. No recommendations, no
clutter.

```
                     Test Song
                Some Band — Some Album

        1:42 ━━━━━━━━━━━━━━━━──────────────── 4:36

               ▶ playing   vol 60   ⤮ shuffle
```

**Status:** milestone 2 of 6 (now playing, playback controls, seek, volume,
shuffle, repeat).
See [DESIGN.md](DESIGN.md) for the full plan.

---

## Setup

You only need to do this once.

### 1. Install Rust

Rust is installed with **rustup**, the official tool that installs and
updates the Rust compiler (`rustc`) and its build tool (`cargo`). With
[Homebrew](https://brew.sh):

```bash
brew install rustup
```

```bash
rustup default stable
```

Homebrew's rustup is "keg-only", meaning it doesn't put `cargo` on your
`PATH` (the list of folders your shell searches for commands) automatically.
Add this line to the end of `~/.zshrc`, the settings file zsh reads every
time you open a terminal:

```bash
echo 'export PATH="/opt/homebrew/opt/rustup/bin:$PATH"' >> ~/.zshrc
```

Then open a new Terminal window (or run `source ~/.zshrc`) and check it worked:

```bash
cargo --version
```

### 2. Get the code and run it

```bash
git clone <your-repo-url> music-player
```

```bash
cd music-player
```

```bash
cargo run
```

`cargo run` downloads the libraries listed in `Cargo.toml` (first time only),
compiles everything, and starts the program. The first build takes a minute;
later ones take a few seconds.

To install it as a normal command you can run from anywhere as `am`:

```bash
cargo install --path .
```

(This copies the program into `~/.cargo/bin`. Run that command again after
you pull new changes. If `am` isn't found, add `~/.cargo/bin` to your `PATH`
the same way as above.)

### 3. Allow Terminal to control Music (the Automation permission)

macOS doesn't let one app control another without asking you. The **first
time** you run `am`, a dialog appears:

> "Terminal" wants access to control "Music".

Click **OK**. That's it.

The permission belongs to your *terminal app* (Terminal.app), not to `am`.
If you later run `am` from a different terminal (iTerm2, Ghostty…), you'll be
asked again for that app.

**If you clicked "Don't Allow"**, `am` shows a help screen. To fix it:

1. Open **System Settings → Privacy & Security → Automation**.
2. Find **Terminal** and expand it.
3. Turn on the switch next to **Music**.

`am` notices within a second. If Terminal isn't in that list at all, reset
the decision so macOS asks again next time:

```bash
tccutil reset AppleEvents com.apple.Terminal
```

### If Music.app isn't open

`am` opens it for you in the background, hidden, so its window doesn't get
in your way. If you quit Music while `am` is running, `am` says so and you
can press `o` to reopen it.

---

## Keys

| Key | Action |
|---|---|
| `space` | play / pause |
| `n` / `p` | next / previous track |
| `←` `→` or `h` `l` | seek back / forward 5 seconds |
| `+` / `-` | volume up / down by 5 (`=` works for `+` too, no Shift needed) |
| `s` | shuffle on / off |
| `r` | repeat: off → all → one → off |
| `?` | show / hide this list on screen (`Esc` closes it) |
| `o` | open Music (when it isn't running) |
| `q` or `Ctrl-C` | quit `am` (Music keeps playing) |

More keys arrive with later milestones (library search, queue, AirPlay…).

---

## How it works (short version)

- `am` talks to Music.app with **AppleScript**, by running the built-in
  macOS program `osascript`.
- A background thread asks Music for its status once a second, and the screen
  redraws 10 times a second. The progress bar moves smoothly between checks.
- All AppleScript is in [`src/music/scripts.rs`](src/music/scripts.rs).

You can try the same thing `am` does yourself, right in the terminal:

```bash
osascript -e 'tell application "Music" to name of current track'
```

```bash
osascript -e 'tell application "Music" to playpause'
```

## Code tour (where to start reading)

```
src/
├── main.rs              start here: sets up the terminal, runs the main loop
├── app.rs               the app's state + what each key does (has tests)
├── worker.rs            the background thread that talks to Music
├── music/
│   ├── mod.rs           the types: Track, PlayerStatus, MusicError…
│   ├── scripts.rs       every AppleScript, in one place
│   ├── osascript.rs     runs a script, turns errors into MusicError
│   └── parse.rs         turns osascript's output into Rust structs (has tests)
└── ui/
    ├── mod.rs           draws whichever screen applies
    ├── now_playing.rs   the focus screen
    └── help.rs          the `?` key list
```

Comments marked **"Rust note:"** explain a Rust concept the first time it shows
up.

## Development commands

| Command | What it does |
|---|---|
| `cargo run` | build and start the app |
| `cargo test` | run the unit tests (they use made-up songs, never your library) |
| `cargo clippy` | Rust's linter: points out likely mistakes and unidiomatic code |
| `cargo fmt` | reformat all code to the standard Rust style |
| `cargo build --release` | build an optimised version into `target/release/am` |

---

## Git basics used in this project

| Command | What it does |
|---|---|
| `git status` | what's changed since the last commit, and what's staged |
| `git diff` | the exact lines you changed |
| `git add <file>` | "stage" a file: mark it to go into the next commit |
| `git commit -m "message"` | save the staged changes as a snapshot with a message |
| `git log --oneline` | list commits, newest first |
| `git push` | upload your commits to GitHub |

A good habit: run `git status` and `git diff` *before* every commit, so you
only commit what you meant to.

### Privacy and safety

- **Your email:** every commit records an author email, and it becomes public
  when you push. Use GitHub's private address (Settings → Emails → "Keep my
  email address private" shows it, ending in `@users.noreply.github.com`):

  ```bash
  git config --global user.email "<id>+<username>@users.noreply.github.com"
  ```

  Check what's set with `git config user.email`.
- **No secrets here:** `am` only talks to the Music.app on your Mac, so there
  are no API keys or passwords and no `.env` file is needed. `.gitignore`
  still blocks `.env*`, logs and key files, so they can't be committed by
  accident.
- **On GitHub:** in your repository's **Settings → Code security**, turn on
  **Secret scanning** and **Push protection**. GitHub will then refuse a push
  that contains something that looks like a password or key.

## License

MIT. See [LICENSE](LICENSE).
