//! End-to-end tests: the same scripts `scribecraft-cli run` executes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)] // test-only helpers

use std::path::PathBuf;

use scribecraft_automation::{Runner, TOOLS};
use serde_json::{Value, json};

fn root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scribecraft-auto-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn call(r: &mut Runner, tool: &str, args: Value) -> Value {
    r.call(tool, &args).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn transcription_workflow_end_to_end() {
    let dir = root("workflow");
    let mut r = Runner::new(&dir).unwrap();
    call(&mut r, "synth_dictation", json!({ "out": "inbox/interview.wav", "seconds": 6, "rate": 8000 }));
    call(&mut r, "jobs_load", json!({ "path": "jobs.json" }));
    let scan = call(&mut r, "folder_scan", json!({ "path": "inbox", "add": true }));
    assert_eq!(scan["found"], json!(["inbox/interview.wav"]));
    let id = scan["added"][0].as_u64().unwrap();
    call(&mut r, "dictation_update", json!({ "id": id, "priority": "urgent", "author": "Dr. Example", "due": "2026-10-21" }));

    let opened = call(&mut r, "dictation_open", json!({ "id": id }));
    assert_eq!(opened["duration_ms"], 6000);
    call(&mut r, "transport_control", json!({ "action": "play" }));
    call(&mut r, "transport_advance", json!({ "ms": 3000 }));
    let stopped = call(&mut r, "transport_control", json!({ "action": "stop" }));
    assert_eq!(stopped["position_ms"], 2000, "3 s played, 1 s auto-backstep");
    assert_eq!(call(&mut r, "timestamp", json!({}))["text"], "[00:00:02.0]");
    call(&mut r, "bookmark_add", json!({ "label": "unclear word" }));

    call(&mut r, "transport_control", json!({ "action": "slow" }));
    let status = call(&mut r, "transport_status", json!({}));
    assert_eq!((status["preset"].as_str(), status["speed"].as_f64()), (Some("slow"), Some(0.6)));
    let rendered = call(&mut r, "transport_render", json!({ "out": "check.wav", "from": "0:01", "duration_ms": 2000 }));
    assert_eq!(rendered["duration_ms"], 2000);
    let info = call(&mut r, "media_info", json!({ "path": "check.wav" }));
    assert_eq!((info["sample_rate"].as_u64(), info["duration_ms"].as_u64()), (Some(8000), Some(2000)));
    // Rendering did not move the user's playback position.
    assert_eq!(call(&mut r, "transport_status", json!({}))["position_ms"], 2000);

    call(&mut r, "jobs_save", json!({}));
    let mut again = Runner::new(&dir).unwrap();
    call(&mut again, "jobs_load", json!({ "path": "jobs.json" }));
    let list = call(&mut again, "dictation_list", json!({}));
    let job = &list[0];
    assert_eq!((job["status"].as_str(), job["priority"].as_str(), job["position_ms"].as_u64()), (Some("in_progress"), Some("urgent"), Some(2000)));
    assert_eq!(job["bookmarks"][0]["label"], "unclear word");
    assert!(!list.to_string().contains(&*dir.to_string_lossy()), "results never show absolute paths");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn removing_the_open_dictation_stops_it_and_keeps_the_recording() {
    let dir = root("remove");
    let mut r = Runner::new(&dir).unwrap();
    call(&mut r, "synth_dictation", json!({ "out": "note.wav", "seconds": 2 }));
    let id = call(&mut r, "dictation_add", json!({ "path": "note.wav", "author": "Dr. Example" }))["id"].as_u64().unwrap();
    call(&mut r, "dictation_open", json!({ "id": id }));
    assert!(dir.join("note.wav").exists());

    let removed = call(&mut r, "dictation_remove", json!({ "id": id }));
    assert_eq!(removed["removed"], id);
    assert_eq!(call(&mut r, "dictation_list", json!({})), json!([]), "the row is gone");
    assert!(dir.join("note.wav").exists(), "the recording file is kept");
    let err = r.call("transport_status", &json!({})).unwrap_err();
    assert!(err.message.contains("open"), "the transport must not outlive the job: {err}");
    let err = r.call("dictation_remove", &json!({ "id": id })).unwrap_err();
    assert!(err.message.contains(&id.to_string()), "removing it twice must say which id is unknown: {err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn paths_outside_the_root_are_refused() {
    let dir = root("confine");
    let outside = dir.with_file_name(format!("scribecraft-outside-{}.wav", std::process::id()));
    let mut r = Runner::new(&dir).unwrap();
    for bad in ["../x.wav", "a/../../x.wav", &outside.to_string_lossy()] {
        let e = r.call("synth_dictation", &json!({ "out": bad, "seconds": 0.1 })).unwrap_err();
        assert!(e.message.contains("root") || e.message.contains(".."), "{bad}: {e}");
    }
    assert!(!outside.exists());
    // Nested dispatch through a script is confined too.
    let e = r.run_script(&json!({ "steps": [{ "tool": "jobs_load", "args": { "path": "../jobs.json" } }] })).unwrap_err();
    assert!(e.message.starts_with("step 1:"), "{e}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_the_root() {
    let dir = root("symlink");
    let outside = root("symlink-target");
    std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();
    let mut r = Runner::new(&dir).unwrap();
    assert!(r.call("synth_dictation", &json!({ "out": "link/x.wav", "seconds": 0.1 })).is_err());
    assert!(!outside.join("x.wav").exists());
}

#[test]
fn errors_say_what_is_allowed() {
    let dir = root("errors");
    let mut r = Runner::new(&dir).unwrap();
    let cases = [
        ("no_such_tool", json!({}), "available tools"),
        ("transport_status", json!({}), "no dictation is open"),
        ("transport_control", json!({ "action": "jump" }), "no dictation is open"),
        ("dictation_open", json!({ "id": "one" }), "non-negative integer"),
        ("settings_set", json!({ "speed_of_light": 1 }), "unknown setting"),
        ("settings_set", json!({ "auto_backstep_ms": 999999 }), "0..=60000"),
        ("settings_set", json!({ "channel_mode": "surround" }), "stereo, left, right, mix"),
        ("jobs_save", json!({}), "jobs_load"),
        ("media_info", json!({ "path": "missing.wav" }), "cannot read"),
    ];
    let canonical = std::fs::canonicalize(&dir).unwrap();
    for (tool, args, needle) in cases {
        let e = r.call(tool, &args).unwrap_err();
        assert!(e.to_string().contains(needle), "{tool}: {e}");
        assert!(!e.to_string().contains(&*canonical.to_string_lossy()), "{tool} leaks the root: {e}");
    }
    call(&mut r, "synth_dictation", json!({ "out": "a.wav", "seconds": 1 }));
    call(&mut r, "jobs_load", json!({ "path": "jobs.json" }));
    let id = call(&mut r, "dictation_add", json!({ "path": "a.wav" }))["id"].as_u64().unwrap();
    call(&mut r, "dictation_open", json!({ "id": id }));
    let e = r.call("transport_control", &json!({ "action": "jump" })).unwrap_err();
    assert!(e.message.contains("allowed: play"), "{e}");
    let e = r.call("transport_set_speed", &json!({ "speed": 9 })).unwrap_err();
    assert!(e.message.contains("0.1..=4"), "{e}");
    assert!(r.call("dictation_add", &json!({ "path": "a.wav" })).unwrap_err().message.contains("already"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn serial_foot_pedal_drives_playback_end_to_end() {
    let dir = root("pedal");
    let mut r = Runner::new(&dir).unwrap();
    call(&mut r, "synth_dictation", json!({ "out": "d.wav", "seconds": 10, "rate": 8000 }));
    let id = call(&mut r, "dictation_add", json!({ "path": "d.wav" }))["id"].as_u64().unwrap();
    call(&mut r, "dictation_open", json!({ "id": id }));
    call(&mut r, "pedal_config_set", json!({ "serial_port": "COM3" }));
    // Bits: DCD 1 = play, DSR 2 = rewind, CTS 4 = fast-forward.
    let down = call(&mut r, "pedal_report", json!({ "bytes": [1] }));
    assert_eq!(down["events"][0]["action"], "play");
    assert_eq!(down["transport"]["state"], "playing");
    call(&mut r, "transport_advance", json!({ "ms": 4000 }));
    let up = call(&mut r, "pedal_report", json!({ "bytes": [0] }));
    assert_eq!((up["transport"]["state"].as_str(), up["transport"]["position_ms"].as_u64()), (Some("stopped"), Some(3000)));
    let rew = call(&mut r, "pedal_report", json!({ "bytes": [2] }));
    assert_eq!(rew["transport"]["state"], "rewinding");
    call(&mut r, "pedal_report", json!({ "bytes": [0] }));

    // A USB pedal: learn its middle pedal from two reports, then use it as play.
    let mut cfg = call(&mut r, "pedal_config_get", json!({}));
    cfg["buttons"] = json!([]);
    call(&mut r, "pedal_config_set", json!({ "config": cfg }));
    call(&mut r, "pedal_report", json!({ "bytes": [0, 0] }));
    call(&mut r, "pedal_learn", json!({ "button": 0 }));
    let learned = call(&mut r, "pedal_report", json!({ "bytes": [0, 2] }));
    assert_eq!(learned["learned"]["bit"], json!({ "byte": 1, "mask": 2, "active_low": false }));
    let mut cfg = call(&mut r, "pedal_config_get", json!({}));
    cfg["buttons"][0]["action"] = json!("play_toggle");
    call(&mut r, "pedal_config_set", json!({ "config": cfg }));
    call(&mut r, "pedal_report", json!({ "bytes": [0, 0] }));
    let play = call(&mut r, "pedal_report", json!({ "bytes": [0, 2] }));
    assert_eq!(play["transport"]["state"], "playing");

    for (args, needle) in [(json!({ "bytes": [256] }), "not a byte"), (json!({}), "bytes"), (json!({ "bytes": vec![0; 65] }), "at most")] {
        let e = r.call("pedal_report", &args).unwrap_err();
        assert!(e.message.contains(needle), "{e}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn every_tool_has_a_summary_and_a_unique_name() {
    let mut names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
    assert!(TOOLS.iter().all(|t| !t.summary.is_empty()));
    names.sort();
    names.dedup();
    assert_eq!(names.len(), TOOLS.len());
}
