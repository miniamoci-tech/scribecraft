//! The tool table. Adding a tool: add a row here, an end-to-end test in
//! `tests/automation.rs`, and cite both in `parity/express-scribe-features.toml`.

use std::sync::Arc;

use scribecraft_engine::media::{Limits, WavFormat, decode_file, synth, write_wav};
use scribecraft_engine::transport::{format_timestamp, parse_time};
use scribecraft_engine::{Dictation, MAX_TEXT_CHARS, Priority, Session, Settings, State, Status, Transport, lock_transport, scan_folder};
use serde_json::{Value, json};

use crate::Runner;
use crate::args;

type Run = fn(&mut Runner, &Value) -> Result<Value, String>;

/// One headless tool.
#[derive(Debug, Clone, Copy)]
pub struct Tool {
    pub name: &'static str,
    /// One line: what it does and its arguments.
    pub summary: &'static str,
    pub(crate) run: Run,
}

/// Every tool, in the order `scribecraft-cli tools` lists them.
pub const TOOLS: &[Tool] = &[
    Tool { name: "jobs_load", summary: "Use the job list in {path} (created on save if missing)", run: jobs_load },
    Tool { name: "jobs_save", summary: "Write the job list to its file atomically", run: jobs_save },
    Tool { name: "dictation_add", summary: "Add the recording at {path} [title, author, priority, due, notes]", run: dictation_add },
    Tool { name: "dictation_list", summary: "List dictations, most urgent then oldest first", run: dictation_list },
    Tool { name: "dictation_update", summary: "Change {id}'s [title, author, priority, status, due, notes]", run: dictation_update },
    Tool { name: "dictation_remove", summary: "Remove {id} from the list (the recording file is kept)", run: dictation_remove },
    Tool { name: "folder_scan", summary: "Find new recordings under {path} [depth=1, add=false]", run: folder_scan },
    Tool { name: "dictation_open", summary: "Open {id} in the transport, resuming at its saved position", run: dictation_open },
    Tool { name: "dictation_close", summary: "Close the open dictation, remembering the position", run: dictation_close },
    Tool { name: "media_info", summary: "Decode {path} and report format, duration and repairs", run: media_info },
    Tool { name: "transport_status", summary: "State, position, duration, speed and preset of the open dictation", run: transport_status },
    Tool {
        name: "transport_control",
        summary: "Press a control {action}: play, stop, toggle, rewind, fast_forward, release, slow, fast, skip_back, skip_forward, start, end",
        run: transport_control,
    },
    Tool { name: "transport_seek", summary: "Move to {ms} or {time} (\"1:20.5\")", run: transport_seek },
    Tool { name: "transport_set_speed", summary: "Set constant-pitch speed {speed} (0.1..=4.0, 1 = normal)", run: transport_set_speed },
    Tool { name: "transport_advance", summary: "Let {ms} of output time pass, as an audio device would", run: transport_advance },
    Tool {
        name: "transport_render",
        summary: "Write what the listener hears to {out} WAV [from, duration_ms, speed, format=pcm16|float32]",
        run: transport_render,
    },
    Tool { name: "settings_get", summary: "Playback settings", run: settings_get },
    Tool { name: "settings_set", summary: "Change any playback settings (same fields as settings_get)", run: settings_set },
    Tool { name: "bookmark_add", summary: "Bookmark the current position of the open dictation [label]", run: bookmark_add },
    Tool { name: "timestamp", summary: "Time stamp text for the current position or {ms} [tenths=true]", run: timestamp },
    Tool { name: "synth_dictation", summary: "Write a synthetic test recording to {out} [seconds=10, rate=16000, channels=2]", run: synth_dictation },
];

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn jobs_load(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let path = r.resolve(args::str(a, "path")?)?;
    let session = Session::with_job_file(&path, r.session.settings().clone()).map_err(err)?;
    r.session = session;
    Ok(json!({ "dictations": r.session.jobs().len() }))
}

fn jobs_save(r: &mut Runner, _: &Value) -> Result<Value, String> {
    if r.session.job_file().is_none() {
        return Err("no job file is in use; call jobs_load with a path first".into());
    }
    r.session.save().map_err(err)?;
    Ok(json!({ "saved": r.session.jobs().len() }))
}

fn dictation_json(r: &Runner, d: &Dictation) -> Value {
    json!({
        "id": d.id,
        "path": r.display(&d.path),
        "title": d.title,
        "author": d.author,
        "received": d.received,
        "duration_ms": d.duration_ms,
        "priority": d.priority,
        "status": d.status,
        "due": d.due,
        "notes": d.notes,
        "position_ms": d.position_ms,
        "bookmarks": d.bookmarks.iter().map(|b| json!({ "ms": b.ms, "time": format_timestamp(b.ms, true), "label": b.label })).collect::<Vec<_>>(),
    })
}

fn parse_priority(s: &str) -> Result<Priority, String> {
    Priority::parse(s).ok_or_else(|| format!("unknown priority {s:?} (allowed: low, normal, high, urgent)"))
}

fn parse_status(s: &str) -> Result<Status, String> {
    Status::parse(s).ok_or_else(|| format!("unknown status {s:?} (allowed: new, in_progress, done, dispatched)"))
}

fn dictation_add(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let path = r.resolve(args::str(a, "path")?)?;
    if !path.is_file() {
        return Err(format!("{} is not a file", r.display(&path)));
    }
    let id = r.session.add_file(&path, args::opt_str(a, "title")?, args::opt_str(a, "author")?.unwrap_or("")).map_err(err)?;
    apply_fields(r, id, a)?;
    Ok(json!({ "id": id }))
}

fn apply_fields(r: &mut Runner, id: u64, a: &Value) -> Result<(), String> {
    let priority = args::opt_str(a, "priority")?.map(parse_priority).transpose()?;
    let status = args::opt_str(a, "status")?.map(parse_status).transpose()?;
    let d = r.session.jobs_mut().get_mut(id).map_err(err)?;
    if let Some(p) = priority {
        d.priority = p;
    }
    if let Some(s) = status {
        d.status = s;
    }
    for (key, field) in [("title", &mut d.title), ("author", &mut d.author), ("due", &mut d.due), ("notes", &mut d.notes)] {
        if let Some(v) = args::opt_str(a, key)? {
            *field = v.chars().take(MAX_TEXT_CHARS).collect();
        }
    }
    Ok(())
}

fn dictation_list(r: &mut Runner, _: &Value) -> Result<Value, String> {
    let list: Vec<Value> = r.session.jobs().queue().into_iter().map(|d| dictation_json(r, d)).collect();
    Ok(Value::Array(list))
}

fn dictation_update(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let id = args::u64(a, "id")?;
    apply_fields(r, id, a)?;
    let d = r.session.jobs().get(id).map_err(err)?;
    Ok(dictation_json(r, d))
}

fn dictation_remove(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let id = args::u64(a, "id")?;
    if r.session.open_id() == Some(id) {
        r.session.close();
    }
    r.session.jobs_mut().remove(id).map_err(err)?;
    Ok(json!({ "removed": id }))
}

fn folder_scan(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let dir = r.resolve(args::str(a, "path")?)?;
    let depth = usize::try_from(args::opt_u64(a, "depth")?.unwrap_or(1)).unwrap_or(usize::MAX);
    let found = scan_folder(&dir, depth, r.session.jobs()).map_err(err)?;
    let mut added = Vec::new();
    if args::opt_bool(a, "add")?.unwrap_or(false) {
        for p in &found {
            added.push(r.session.add_file(p, None, "").map_err(err)?);
        }
    }
    let found: Vec<String> = found.iter().map(|p| r.display(p)).collect();
    Ok(json!({ "found": found, "added": added }))
}

fn status_json(t: &Transport) -> Value {
    json!({
        "state": t.state(),
        "position_ms": t.position_ms(),
        "position": format_timestamp(t.position_ms(), true),
        "duration_ms": t.duration_ms(),
        "speed": t.speed(),
        "preset": t.preset(),
        "sample_rate": t.sample_rate(),
        "channels": t.channels(),
    })
}

fn dictation_open(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let id = args::u64(a, "id")?;
    r.session.open(id).map_err(err)?;
    r.session.with_transport(|t| status_json(t)).map_err(err)
}

fn dictation_close(r: &mut Runner, _: &Value) -> Result<Value, String> {
    let id = r.session.open_id();
    r.session.close();
    Ok(json!({ "closed": id }))
}

fn media_info(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let path = r.resolve(args::str(a, "path")?)?;
    let audio = decode_file(&path, &Limits::default()).map_err(err)?;
    Ok(json!({
        "path": r.display(&path),
        "codec": audio.codec,
        "sample_rate": audio.sample_rate,
        "channels": audio.channels,
        "frames": audio.frames(),
        "duration_ms": audio.duration_ms(),
        "duration": format_timestamp(audio.duration_ms(), true),
        "repairs": audio.repairs,
    }))
}

fn transport_status(r: &mut Runner, _: &Value) -> Result<Value, String> {
    r.session.with_transport(|t| status_json(t)).map_err(err)
}

fn transport_control(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let action = args::str(a, "action")?.to_owned();
    let skip = r.session.settings().skip_ms;
    let skip = i64::try_from(skip).unwrap_or(i64::MAX);
    r.session
        .with_transport(|t| {
            match action.as_str() {
                "play" => t.play(),
                "stop" => t.stop(),
                "toggle" => t.toggle_play(),
                "rewind" => t.rewind(),
                "fast_forward" => t.fast_forward(),
                "release" => t.release(),
                "slow" => t.toggle_slow(),
                "fast" => t.toggle_fast(),
                "skip_back" => t.skip_ms(-skip),
                "skip_forward" => t.skip_ms(skip),
                "start" => t.seek_ms(0),
                "end" => t.seek_ms(u64::MAX),
                other => {
                    return Err(format!(
                        "unknown action {other:?} (allowed: play, stop, toggle, rewind, fast_forward, release, slow, fast, skip_back, skip_forward, start, end)"
                    ));
                }
            }
            Ok(status_json(t))
        })
        .map_err(err)?
}

fn transport_seek(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let ms = match (args::opt_u64(a, "ms")?, args::opt_str(a, "time")?) {
        (Some(ms), None) => ms,
        (None, Some(text)) => {
            parse_time(text).ok_or_else(|| format!("cannot read {text:?} as a time (use ss, mm:ss or hh:mm:ss, optionally .fraction)"))?
        }
        _ => return Err("give exactly one of \"ms\" or \"time\"".into()),
    };
    r.session
        .with_transport(|t| {
            t.seek_ms(ms);
            status_json(t)
        })
        .map_err(err)
}

fn transport_set_speed(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let speed = args::opt_f64(a, "speed")?.ok_or("missing required number argument \"speed\"")?;
    r.session.with_transport(|t| t.set_speed(speed).map(|()| status_json(t)).map_err(err)).map_err(err)?
}

/// Longest stretch of simulated time per call (one hour).
const MAX_ADVANCE_MS: u64 = 3_600_000;

fn transport_advance(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let ms = args::u64(a, "ms")?;
    if ms > MAX_ADVANCE_MS {
        return Err(format!("ms must be at most {MAX_ADVANCE_MS}"));
    }
    r.session
        .with_transport(|t| {
            let ch = t.channels();
            let mut left = usize::try_from(scribecraft_engine::media::ms_to_frames(ms, t.sample_rate())).unwrap_or(0);
            let mut block = vec![0.0f32; 512 * ch];
            let mut audible = 0usize;
            while left > 0 {
                let n = left.min(512);
                if let Some(b) = block.get_mut(..n * ch) {
                    audible += t.render(b);
                }
                left -= n;
                if t.state() == State::Stopped {
                    break;
                }
            }
            let mut s = status_json(t);
            if let Some(o) = s.as_object_mut() {
                o.insert("audible_ms".into(), json!(scribecraft_engine::media::frames_to_ms(audible as u64, t.sample_rate())));
            }
            s
        })
        .map_err(err)
}

fn transport_render(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let out = r.resolve_output(args::str(a, "out")?)?;
    let format = match args::opt_str(a, "format")?.unwrap_or("pcm16") {
        "pcm16" => WavFormat::Pcm16,
        "float32" => WavFormat::Float32,
        other => return Err(format!("unknown format {other:?} (allowed: pcm16, float32)")),
    };
    let from = match (args::opt_u64(a, "from_ms")?, args::opt_str(a, "from")?) {
        (Some(ms), _) => Some(ms),
        (None, Some(text)) => Some(parse_time(text).ok_or_else(|| format!("cannot read {text:?} as a time"))?),
        (None, None) => None,
    };
    let duration = args::opt_u64(a, "duration_ms")?;
    let speed = args::opt_f64(a, "speed")?;
    let transport = r.session.transport().map_err(err)?;
    // Render on a copy so the user's open transport keeps its position, speed and state.
    let (audio, settings, position, current_speed) = {
        let t = lock_transport(&transport);
        (Arc::clone(t.audio()), t.settings().clone(), t.position_ms(), t.speed())
    };
    let mut copy = Transport::new(audio, settings).map_err(err)?;
    copy.set_speed(speed.unwrap_or(current_speed)).map_err(err)?;
    let from = from.unwrap_or(position);
    let duration = duration.unwrap_or(u64::MAX).min(MAX_ADVANCE_MS);
    let rendered = copy.render_range(from, duration);
    write_wav(&out, &rendered, format).map_err(err)?;
    Ok(json!({ "out": r.display(&out), "duration_ms": rendered.duration_ms(), "speed": copy.speed(), "from_ms": from }))
}

fn settings_get(r: &mut Runner, _: &Value) -> Result<Value, String> {
    serde_json::to_value(r.session.settings()).map_err(err)
}

fn settings_set(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let mut current = serde_json::to_value(r.session.settings()).map_err(err)?;
    let (Some(cur), Some(patch)) = (current.as_object_mut(), a.as_object()) else {
        return Err("arguments must be a JSON object".into());
    };
    for (k, v) in patch {
        if !cur.contains_key(k) {
            let known: Vec<&String> = cur.keys().collect();
            return Err(format!("unknown setting {k:?} (known: {known:?})"));
        }
        cur.insert(k.clone(), v.clone());
    }
    let settings: Settings = serde_json::from_value(current).map_err(|e| format!("invalid settings: {e}"))?;
    r.session.set_settings(settings).map_err(err)?;
    settings_get(r, &Value::Null)
}

fn bookmark_add(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let label = args::opt_str(a, "label")?.unwrap_or("");
    let ms = r.session.bookmark_here(label).map_err(err)?;
    Ok(json!({ "ms": ms, "time": format_timestamp(ms, true) }))
}

fn timestamp(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let ms = match args::opt_u64(a, "ms")? {
        Some(ms) => ms,
        None => r.session.with_transport(|t| t.position_ms()).map_err(err)?,
    };
    let tenths = args::opt_bool(a, "tenths")?.unwrap_or(true);
    Ok(json!({ "ms": ms, "text": format!("[{}]", format_timestamp(ms, tenths)) }))
}

fn synth_dictation(r: &mut Runner, a: &Value) -> Result<Value, String> {
    let out = r.resolve_output(args::str(a, "out")?)?;
    let seconds = args::opt_f64(a, "seconds")?.unwrap_or(10.0);
    if !(0.0..=3600.0).contains(&seconds) {
        return Err("seconds must be between 0 and 3600".into());
    }
    let rate = u32::try_from(args::opt_u64(a, "rate")?.unwrap_or(16_000)).map_err(err)?;
    let channels = u16::try_from(args::opt_u64(a, "channels")?.unwrap_or(2)).map_err(err)?;
    let audio = synth::dictation(rate, channels, seconds);
    write_wav(&out, &audio, WavFormat::Pcm16).map_err(err)?;
    Ok(json!({ "out": r.display(&out), "duration_ms": audio.duration_ms(), "sample_rate": audio.sample_rate, "channels": audio.channels }))
}
