# M2 — Seek, volume, shuffle, repeat, help

**Spec:** [DESIGN.md](../../DESIGN.md) §5.3, §8 (M2 row)

## Decisions

- **Keys:**
  - `←`/`h` and `→`/`l` seek ±5 s;
  - `+`/`=` and `-`/`_` change volume ±5, clamped to 0–100;
  - `s` toggles shuffle;
  - `r` cycles repeat off → all → one → off;
  - `?` toggles the key list, and `Esc` closes it.
- **The screen updates instantly.** The app predicts each change right
  away (new volume, shuffle flag, seek position, play/pause), and the next
  poll confirms it.
- **No flicker back to old values.** The worker counts commands it has
  finished and puts that count in every status update. The app ignores any
  status update older than its latest command, so a poll that was already
  in flight can't undo the prediction.
- **Batched key presses.** After running a command, the worker runs any
  other commands already waiting before it polls again, so holding `+`
  doesn't queue up slow polls.
- **Volume is sent as an absolute value** (`SetVolume(65)`), computed from
  the app's own idea of the volume, so what you see is what gets set.
- **The bottom line shows only `? keys`.** The full key list moves into the
  `?` overlay, which is quieter.

## Interfaces

- `Command`: adds
  - `Seek(i32)`, a relative number of seconds;
  - `SetVolume(u8)`;
  - `ToggleShuffle`;
  - `CycleRepeat`.
- `Update::Status { status: PlayerStatus, acked: u64 }`. `acked` is the
  number of commands the worker has finished.
- `App::handle_key(KeyEvent, Instant) -> Option<Command>`. This also
  applies the prediction and counts the command as sent.
- `App::show_help: bool`
- `scripts::seek_by(i32)`, `scripts::set_volume(u8)`,
  `scripts::TOGGLE_SHUFFLE`, `scripts::CYCLE_REPEAT`

## Tests

- **Key mapping:** each new key produces the right command.
- **Volume:** clamps at 0 and 100.
- **Seek:** clamps between 0 and the track's duration.
- **Repeat:** cycles in the order off → all → one.
- **Play/pause:** the prediction freezes and resumes the progress position.
- **Stale updates:** a status with `acked < sent` is ignored, and one with
  `acked == sent` is applied.
- **Help:** `?` toggles it, and `Esc` closes it.
