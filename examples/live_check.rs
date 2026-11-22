//! End-to-end check against the real Music.app: `cargo run --example live_check`
//!
//! Unlike `cargo test` (which never touches Music), this really plays,
//! queues, seeks and skips, so it proves the AppleScript behaves the way the
//! app assumes. To stay quiet it sets Music's volume to 0 first and puts it
//! back at the end. It also stops playback and empties the `am · Queue`
//! playlist when done. It uses the first few songs of your library.
//!
//! Every surprise found while writing it is now a comment in
//! `src/music/scripts.rs` ("Found by testing").
#![allow(dead_code)]
#[path = "../src/art.rs"]
mod art;
#[path = "../src/library.rs"]
mod library;
#[path = "../src/music/mod.rs"]
mod music;
#[path = "../src/shuffle.rs"]
mod shuffle;
use music::queue::Queued;
use music::{Music, PlayState, StatusReply};
use std::{thread::sleep, time::Duration};

fn status(m: &Music) -> music::PlayerStatus {
    match m.status().unwrap() {
        StatusReply::Status(s) => s,
        _ => panic!("not running"),
    }
}
fn now_id(m: &Music) -> Option<String> {
    status(m).track.map(|t| t.id)
}
fn check(label: &str, ok: bool) {
    println!("  {}  {label}", if ok { "ok  " } else { "FAIL" });
}
fn wait() {
    sleep(Duration::from_millis(1500));
}
fn settle() {
    sleep(Duration::from_millis(300));
} // same as the worker's SETTLE

fn main() {
    let m = Music::new();
    let original_volume = status(&m).volume;
    m.set_volume(0).unwrap();
    let (songs, playlists) = m.library().unwrap();
    let lib = library::Library::new(songs, playlists);
    let ids: Vec<String> = lib.songs.iter().take(5).map(|s| s.id.clone()).collect();
    let _ = music::osascript::run("tell application \"Music\" to stop");
    music::osascript::run(&music::scripts::replace_queue(
        &[],
        None,
        false,
        None,
        Some(false),
    ))
    .unwrap();

    // Queue while stopped, then start it from the Queue tab (play_queue_at).
    check(
        "enqueue while stopped → Ready",
        m.enqueue(&ids[..2], false, None).unwrap() == Queued::Ready,
    );
    check(
        "…then add 1 more at the end",
        m.enqueue(&ids[2..3], false, None).unwrap() == Queued::Ready,
    );
    let order: Vec<String> = m.queue().unwrap().songs.into_iter().map(|s| s.id).collect();
    check("queue is 1, 2, 3", order == ids[..3]);
    m.play_queue_at(2).unwrap();
    wait();
    check("play queue at song 2", now_id(&m).as_ref() == Some(&ids[1]));
    m.next().unwrap();
    wait();
    check(
        "next → song 3 (stays in the queue)",
        now_id(&m).as_ref() == Some(&ids[2]),
    );

    // Play next while playing from the queue.
    m.seek_by(20).unwrap();
    settle();
    let before = status(&m).position;
    check(
        "play next while in queue → Added",
        m.enqueue(&ids[3..4], true, None).unwrap() == Queued::Added,
    );
    wait();
    let s = status(&m);
    check(
        "…same song still playing",
        s.track.as_ref().map(|t| &t.id) == Some(&ids[2]),
    );
    check(
        &format!(
            "…at about the same spot ({before:.0} s → {:.0} s)",
            s.position
        ),
        (s.position - before).abs() < 4.0,
    );
    m.enqueue(&ids[4..5], false, None).unwrap();
    wait();
    m.next().unwrap();
    wait();
    check(
        "next → the 'play next' song (4)",
        now_id(&m).as_ref() == Some(&ids[3]),
    );
    m.next().unwrap();
    wait();
    check(
        "next → the 'add to queue' song (5)",
        now_id(&m).as_ref() == Some(&ids[4]),
    );

    // Shuffle / repeat, read back after the worker's settle pause.
    let sh = status(&m).shuffle;
    m.toggle_shuffle().unwrap();
    settle();
    check("shuffle toggles", status(&m).shuffle != sh);
    m.toggle_shuffle().unwrap();
    settle();
    let rep = status(&m).repeat;
    m.cycle_repeat().unwrap();
    settle();
    let rep2 = status(&m).repeat;
    m.cycle_repeat().unwrap();
    m.cycle_repeat().unwrap();
    settle();
    check(
        &format!("repeat cycles {rep:?} → {rep2:?} → back"),
        rep != rep2 && status(&m).repeat == rep,
    );

    // An album from track 2, then next.
    if let Some(album) = lib.albums.iter().find(|a| lib.album_songs(a).len() >= 3) {
        let a: Vec<String> = lib
            .album_songs(album)
            .iter()
            .map(|s| s.id.clone())
            .collect();
        m.play_songs(&a, 2, None, Some(false)).unwrap();
        wait();
        check(
            "play album from track 2",
            now_id(&m).as_ref() == Some(&a[1]),
        );
        check("…not marked as shuffled", !status(&m).queue_shuffled);
        m.next().unwrap();
        wait();
        check("next → album track 3", now_id(&m).as_ref() == Some(&a[2]));
    }

    // Shuffle an Apple Music playlist (songs mostly not in the library), the
    // way Shift-S does: our order must be the order Music plays.
    if let Some(p) = lib.playlists.iter().find(|p| p.apple_music) {
        let mut ps: Vec<String> = m
            .playlist_songs(&p.id)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        if ps.len() >= 3 {
            shuffle::shuffle(&mut ps, shuffle::seed());
            m.play_songs(&ps, 1, Some(&p.id), Some(true)).unwrap();
            wait();
            check(
                "shuffled Apple Music playlist starts at our first pick",
                now_id(&m).as_ref() == Some(&ps[0]),
            );
            let q: Vec<String> = m.queue().unwrap().songs.into_iter().map(|s| s.id).collect();
            let want = ps.len().min(music::queue::MAX_QUEUE);
            check(
                &format!("shuffled queue holds {want} songs in our order"),
                q.len() == want && q[..] == ps[..want],
            );
            check(
                "status says the queue is shuffled",
                status(&m).queue_shuffled,
            );
            // Adding a song keeps it marked as shuffled (the queue is rebuilt).
            m.enqueue(&ids[..1], true, None).unwrap();
            wait();
            check("…and still after 'play next'", status(&m).queue_shuffled);
        }
    }

    // A saved playlist from song 2, next, then "play next" taking it over.
    if let Some(p) = lib.playlists.iter().find(|p| {
        m.playlist_songs(&p.id)
            .map(|s| s.len() >= 3)
            .unwrap_or(false)
    }) {
        let ps: Vec<String> = m
            .playlist_songs(&p.id)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        m.play_playlist(&p.id, 2).unwrap();
        wait();
        check(
            "play playlist from song 2",
            now_id(&m).as_ref() == Some(&ps[1]),
        );
        m.next().unwrap();
        wait();
        check(
            "next → playlist song 3",
            now_id(&m).as_ref() == Some(&ps[2]),
        );
        let before = status(&m);
        check(
            "play next from a playlist → Started",
            m.enqueue(&ids[..1], true, None).unwrap() == Queued::Started,
        );
        wait();
        let after = status(&m);
        check(
            "…same song keeps playing",
            after.track.map(|t| t.id) == before.track.map(|t| t.id),
        );
        m.next().unwrap();
        wait();
        check(
            "…next → the queued song",
            now_id(&m).as_ref() == Some(&ids[0]),
        );
    }

    // An Apple Music playlist (streamed, slower to start) from song 3.
    if let Some(p) = lib.playlists.iter().find(|p| p.apple_music) {
        let ps: Vec<String> = m
            .playlist_songs(&p.id)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        if ps.len() >= 3 {
            m.play_playlist(&p.id, 3).unwrap();
            wait();
            check(
                "play an Apple Music playlist from song 3",
                now_id(&m).as_ref() == Some(&ps[2]),
            );
        }
    }

    // Liked songs that aren't in the library: copied from their playlist,
    // and still there after the queue is rebuilt for another song.
    if let Some(p) = lib
        .playlists
        .iter()
        .find(|p| p.apple_music && p.name == "Favorite Songs")
    {
        let in_library: std::collections::HashSet<&String> =
            lib.songs.iter().map(|s| &s.id).collect();
        let outside: Vec<String> = m
            .playlist_songs(&p.id)
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .filter(|id| !in_library.contains(id))
            .take(2)
            .collect();
        if outside.len() == 2 {
            let _ = music::osascript::run("tell application \"Music\" to stop");
            music::osascript::run(&music::scripts::replace_queue(
                &[],
                None,
                false,
                None,
                Some(false),
            ))
            .unwrap();
            m.enqueue(&outside, false, Some(&p.id)).unwrap();
            m.enqueue(&ids[..1], false, None).unwrap(); // rebuild without the source
            let q: Vec<String> = m.queue().unwrap().songs.into_iter().map(|s| s.id).collect();
            check(
                "liked songs outside the library get queued, and survive a rebuild",
                q == vec![outside[0].clone(), outside[1].clone(), ids[0].clone()],
            );
            m.play_queue_at(1).unwrap();
            wait();
            check("…and play", now_id(&m).as_ref() == Some(&outside[0]));
            let size = m.library().unwrap().0.len();
            check(
                &format!("…without being added to the library ({size} songs)"),
                size == lib.songs.len(),
            );
        }
    }

    // Paused "play next" should stay paused.
    m.play_pause().unwrap();
    settle();
    m.enqueue(&ids[1..2], true, None).unwrap();
    wait();
    check(
        "play next while paused stays paused",
        status(&m).state == PlayState::Paused,
    );

    let path = std::env::temp_dir().join("am-live-art");
    let saved = m.save_artwork(&path.to_string_lossy()).unwrap();
    let decoded = saved && art::decode(&std::fs::read(&path).unwrap(), 20).is_some();
    let _ = std::fs::remove_file(&path);
    check(
        &format!("artwork (has art: {saved}, decoded: {decoded})"),
        !saved || decoded,
    );

    // Leave things as we found them.
    let _ = music::osascript::run("tell application \"Music\" to stop");
    music::osascript::run(&music::scripts::replace_queue(
        &[],
        None,
        false,
        None,
        Some(false),
    ))
    .unwrap();
    m.set_volume(original_volume).unwrap();
    check(
        &format!("stopped, queue emptied, volume back to {original_volume}"),
        status(&m).volume.abs_diff(original_volume) <= 1,
    );
}
