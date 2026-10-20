//! `scribecraft-cli`: inspect, render and play recordings; run automation scripts.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use scribecraft_automation::{Runner, TOOLS};
use scribecraft_engine::media::{Limits, WavFormat, decode_file, synth, write_wav};
use scribecraft_engine::transport::{format_timestamp, parse_time};
use scribecraft_engine::{Settings, State, Transport, lock_transport};

const USAGE: &str = "\
scribecraft-cli — the ScribeCraft transcription player, headless

USAGE:
  scribecraft-cli info <file>
  scribecraft-cli play <file> [--speed S] [--from TIME] [--seconds N]
  scribecraft-cli render <file> --out <out.wav> [--speed S] [--from TIME] [--seconds N]
  scribecraft-cli tone --out <out.wav> [--seconds N]
  scribecraft-cli run --script <steps.json> --root <dir>
  scribecraft-cli tools
  scribecraft-cli pedal list
  scribecraft-cli pedal monitor <number from list | COM3 | /dev/ttyUSB0> [--seconds N]

TIME is ss, mm:ss or hh:mm:ss (optionally .fraction). S is 0.1..=4.0 (1 = normal speed).
While playing, type a key and press Enter:
  p = play/stop   b = back 5 s   n = forward 5 s   s = slow   f = fast   q = quit";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Parsed `--flag value` options after the positional arguments.
struct Opts<'a> {
    positional: Vec<&'a str>,
    flags: Vec<(&'a str, &'a str)>,
}

impl<'a> Opts<'a> {
    fn parse(args: &'a [String]) -> Result<Self, String> {
        let mut positional = Vec::new();
        let mut flags = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                let value = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
                flags.push((name, value.as_str()));
            } else {
                positional.push(a.as_str());
            }
        }
        Ok(Self { positional, flags })
    }

    fn get(&self, name: &str) -> Option<&'a str> {
        self.flags.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }

    fn file(&self, i: usize, what: &str) -> Result<PathBuf, String> {
        self.positional.get(i).map(PathBuf::from).ok_or_else(|| format!("missing {what}\n\n{USAGE}"))
    }

    fn speed(&self) -> Result<f64, String> {
        self.get("speed").map(|s| s.parse::<f64>().map_err(|_| format!("--speed {s:?} is not a number"))).transpose().map(|s| s.unwrap_or(1.0))
    }

    fn time(&self, name: &str) -> Result<Option<u64>, String> {
        self.get(name).map(|t| parse_time(t).ok_or_else(|| format!("--{name} {t:?} is not a time (use ss, mm:ss or hh:mm:ss)"))).transpose()
    }

    fn seconds(&self) -> Result<Option<f64>, String> {
        self.get("seconds")
            .map(|s| {
                s.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| format!("--seconds {s:?} is not a non-negative number"))
            })
            .transpose()
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let Some((command, rest)) = args.split_first() else {
        println!("{USAGE}");
        return Ok(());
    };
    let opts = Opts::parse(rest)?;
    match command.as_str() {
        "info" => info(&opts.file(0, "the recording to inspect")?),
        "play" => play(&opts),
        "render" => render(&opts),
        "tone" => tone(&opts),
        "run" => script(&opts),
        "pedal" => pedal(&opts),
        "tools" => {
            for t in TOOLS {
                println!("{:<22} {}", t.name, t.summary);
            }
            Ok(())
        }
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

fn transport_for(path: &Path, opts: &Opts) -> Result<Transport, String> {
    let audio = decode_file(path, &Limits::default()).map_err(|e| e.to_string())?;
    for r in &audio.repairs {
        eprintln!("warning: {r}");
    }
    let mut t = Transport::new(Arc::new(audio), Settings::default()).map_err(|e| e.to_string())?;
    t.set_speed(opts.speed()?).map_err(|e| e.to_string())?;
    if let Some(ms) = opts.time("from")? {
        t.seek_ms(ms);
    }
    Ok(t)
}

fn info(path: &Path) -> Result<(), String> {
    let audio = decode_file(path, &Limits::default()).map_err(|e| e.to_string())?;
    println!("codec:       {}", audio.codec);
    println!("sample rate: {} Hz", audio.sample_rate);
    println!("channels:    {}", audio.channels);
    println!("duration:    {}", format_timestamp(audio.duration_ms(), true));
    for r in &audio.repairs {
        println!("repaired:    {r}");
    }
    Ok(())
}

fn render(opts: &Opts) -> Result<(), String> {
    let path = opts.file(0, "the recording to render")?;
    let out = opts.get("out").ok_or("render needs --out <file.wav>")?;
    let mut t = transport_for(&path, opts)?;
    let from = t.position_ms();
    let ms = opts.seconds()?.map(|s| (s * 1000.0) as u64).unwrap_or(u64::MAX);
    let rendered = t.render_range(from, ms);
    write_wav(Path::new(out), &rendered, WavFormat::Pcm16).map_err(|e| e.to_string())?;
    println!("wrote {} ({} at {}x)", out, format_timestamp(rendered.duration_ms(), true), t.speed());
    Ok(())
}

fn tone(opts: &Opts) -> Result<(), String> {
    let out = opts.get("out").ok_or("tone needs --out <file.wav>")?;
    let audio = synth::dictation(16_000, 2, opts.seconds()?.unwrap_or(30.0).min(3600.0));
    write_wav(Path::new(out), &audio, WavFormat::Pcm16).map_err(|e| e.to_string())?;
    println!("wrote synthetic dictation {out} ({})", format_timestamp(audio.duration_ms(), true));
    Ok(())
}

fn script(opts: &Opts) -> Result<(), String> {
    let script = opts.get("script").ok_or("run needs --script <steps.json>")?;
    let root = opts.get("root").ok_or("run needs --root <dir>; every path in the script stays inside it")?;
    let text = std::fs::read_to_string(script).map_err(|e| format!("cannot read {script}: {e}"))?;
    let steps: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{script} is not JSON: {e}"))?;
    let mut runner = Runner::new(Path::new(root)).map_err(|e| e.to_string())?;
    let results = runner.run_script(&steps).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?);
    Ok(())
}

fn play(opts: &Opts) -> Result<(), String> {
    let path = opts.file(0, "the recording to play")?;
    let t = transport_for(&path, opts)?;
    let limit = opts.seconds()?.map(Duration::from_secs_f64);
    let shared = Arc::new(std::sync::Mutex::new(t));
    lock_transport(&shared).play();
    let output = scribecraft_platform::AudioOutput::open(shared.clone()).map_err(|e| e.to_string())?;
    println!("playing on {:?}; p/b/n/s/f/q + Enter to control", output.device_name());

    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let started = std::time::Instant::now();
    let mut input_open = true;
    loop {
        let line = if input_open {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(line) => Some(line),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // No console (or it closed): keep playing, just without key controls.
                    input_open = false;
                    None
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(500));
            None
        };
        if let Some(line) = line {
            let mut t = lock_transport(&shared);
            match line.trim() {
                "p" => t.toggle_play(),
                "b" => t.skip_ms(-5_000),
                "n" => t.skip_ms(5_000),
                "s" => t.toggle_slow(),
                "f" => t.toggle_fast(),
                "q" => break,
                "" => {}
                other => eprintln!("unknown key {other:?} (p, b, n, s, f, q)"),
            }
        }
        let (state, pos, dur, speed) = {
            let t = lock_transport(&shared);
            (t.state(), t.position_ms(), t.duration_ms(), t.speed())
        };
        println!("{:?} {} / {} at {:.0}%", state, format_timestamp(pos, true), format_timestamp(dur, false), speed * 100.0);
        if limit.is_some_and(|l| started.elapsed() >= l) || (state == State::Stopped && pos >= dur) {
            break;
        }
    }
    drop(output);
    Ok(())
}

#[cfg(not(feature = "pedal"))]
fn pedal(_: &Opts) -> Result<(), String> {
    Err("this build has no foot pedal support (built without the \"pedal\" feature)".into())
}

/// `pedal list` shows the devices a pedal may be; `pedal monitor` prints what one sends,
/// so a user can see which device is the pedal and which bit each pedal sets.
#[cfg(feature = "pedal")]
fn pedal(opts: &Opts) -> Result<(), String> {
    use scribecraft_engine::pedal::{PedalSource, SERIAL_CTS, SERIAL_DCD, SERIAL_DSR, SERIAL_RI};
    use scribecraft_platform::pedal::{PedalReader, ReaderStatus, list_devices};
    let devices = list_devices();
    match opts.positional.first().copied() {
        Some("list") => {
            if devices.is_empty() {
                println!("no USB HID devices (other than keyboards and mice) or serial ports found");
            }
            for (i, d) in devices.iter().enumerate() {
                println!("{:>3}  {}{}", i + 1, d.label, if d.likely_pedal { "   <- known pedal maker" } else { "" });
            }
            Ok(())
        }
        Some("monitor") => {
            let which = opts.positional.get(1).copied().ok_or("pedal monitor needs a device: a number from `pedal list`, or a serial port name")?;
            let source = match which.parse::<usize>() {
                Ok(n) => {
                    devices.get(n.wrapping_sub(1)).map(|d| d.source.clone()).ok_or_else(|| format!("there is no device {n}; run `pedal list`"))?
                }
                Err(_) => PedalSource::Serial { port: which.to_owned() },
            };
            let serial = matches!(source, PedalSource::Serial { .. });
            let reader = PedalReader::start(&source, move |report| {
                if serial {
                    let b = report.first().copied().unwrap_or(0);
                    let on = |m: u8, name: &str| if b & m != 0 { format!(" {name}") } else { String::new() };
                    println!(
                        "lines:{}{}{}{}",
                        on(SERIAL_DCD, "DCD(pin1)"),
                        on(SERIAL_DSR, "DSR(pin6)"),
                        on(SERIAL_CTS, "CTS(pin8)"),
                        on(SERIAL_RI, "RI(pin9)")
                    );
                } else {
                    println!("report: {report:02x?}");
                }
            })
            .map_err(|e| e.to_string())?;
            println!("press the pedals (Ctrl+C to stop)");
            let limit = opts.seconds()?.map(Duration::from_secs_f64);
            let started = std::time::Instant::now();
            let mut last = None;
            while limit.is_none_or(|l| started.elapsed() < l) {
                let status = reader.status();
                if last.as_ref() != Some(&status) {
                    match &status {
                        ReaderStatus::Waiting(why) => eprintln!("{why}"),
                        s => eprintln!("{s:?}"),
                    }
                    last = Some(status);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Ok(())
        }
        _ => Err(format!(
            "usage: scribecraft-cli pedal list | pedal monitor <device>

{USAGE}"
        )),
    }
}
