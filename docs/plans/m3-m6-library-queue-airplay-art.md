# M3–M6 — Library, queue, AirPlay, album art

**Spec:** [DESIGN.md](../../DESIGN.md) §5.2, §5.3, §6.3, §6.4, §8, §12

Built in one pass at the owner's request ("do everything you planned").

## Decisions made while building

- **Pop-up tabs:** Search · Playlists · Albums · Artists · Queue · Speakers.
- **Keys in the search box:** letters type there, so queue shortcuts there
  are `Ctrl-N` (play next) and `Ctrl-A` (add to queue). `.` and `Q` work
  everywhere else.
- **Search:** every word must match. A word scores 3 at the start of the
  text, 2 at the start of a word, 1 anywhere else. Ties go artists → albums
  → playlists → songs. At most 100 results.
- **Library load:** read once, automatically, on first contact. `Ctrl-R`
  reloads it.
- **Album art:** 20×20 pixels drawn as 20×10 half-block cells. It uses
  truecolor if `COLORTERM` says so, otherwise the nearest of the 256 xterm
  colours. It's skipped when the window is too short for art plus text.
- **`am --check`:** a read-only self-test with timings, for troubleshooting.

## Verification

- **Unit tests (`cargo test`):** 69, all made-up data.
- **`examples/live_check.rs` against the real Music.app:** 22 checks, all
  passing. It led to the changes listed in DESIGN.md §12.
