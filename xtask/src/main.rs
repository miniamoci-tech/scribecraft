//! `cargo xtask <command>`: ScribeCraft's quality gates and packaging.
//!
//! - `layers`  — crate dependencies respect `plan/architecture.md` §3
//! - `assets`  — every asset file has an `ATTRIBUTION.toml` entry with a matching hash and an allowed licence
//! - `parity`  — `parity/express-scribe-features.toml` is well-formed and its evidence exists [--partial lists open items]
//! - `wasm`    — the engine crates build for `wasm32-unknown-unknown`
//! - `ci`      — fmt, clippy, tests, then all of the above
//! - `dist`    — release build packaged for this OS (Windows installer/zip, macOS .app + .dmg, Linux tar.gz)

mod dist;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::Value;
use sha2::{Digest, Sha256};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = workspace_root();
    let result = match args.first().map(String::as_str) {
        Some("layers") => layers(&root),
        Some("assets") => assets(&root),
        Some("parity") => parity(&root, args.iter().any(|a| a == "--partial")),
        Some("wasm") => wasm(&root),
        Some("ci") => ci(&root),
        Some("dist") => dist::dist(&root),
        _ => Err("usage: cargo xtask <layers|assets|parity [--partial]|wasm|ci|dist>".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent().map(Path::to_path_buf).unwrap_or(here)
}

pub(crate) fn run(cmd: &mut Command) -> Result<(), String> {
    eprintln!("$ {cmd:?}");
    let status = cmd.status().map_err(|e| format!("cannot run {cmd:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{cmd:?} failed with {status}")) }
}

fn cargo() -> Command {
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}

// ---------------------------------------------------------------------------------------
// layers

/// Layer of each workspace crate. A crate may depend only on crates of a lower layer.
const LAYERS: &[(&str, u8)] = &[
    ("scribecraft-dsp", 0),
    ("scribecraft-media", 1),
    ("scribecraft-transport", 2),
    ("scribecraft-engine", 3),
    ("scribecraft-automation", 4),
    ("scribecraft-platform", 5),
    ("scribecraft-ui-egui", 6),
    ("scribecraft-cli", 7),
    ("scribecraft", 7),
];

/// External crates allowed only from the given layer upwards.
const GATED: &[(&str, u8, &str)] = &[
    ("eframe", 6, "GUI"),
    ("egui", 6, "GUI"),
    ("winit", 6, "GUI"),
    ("rfd", 6, "GUI"),
    ("cpal", 5, "OS audio"),
    ("hidapi", 5, "OS HID"),
    ("global-hotkey", 5, "OS hotkeys"),
    ("windows", 5, "OS API"),
    ("windows-sys", 5, "OS API"),
    ("objc2", 5, "OS API"),
];

fn layers(root: &Path) -> Result<(), String> {
    let out = cargo()
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata output: {e}"))?;
    let layer: BTreeMap<&str, u8> = LAYERS.iter().copied().collect();
    let mut problems = Vec::new();
    for pkg in meta.get("packages").and_then(Value::as_array).into_iter().flatten() {
        let name = pkg.get("name").and_then(Value::as_str).unwrap_or("");
        if name == "xtask" {
            continue;
        }
        let Some(&own) = layer.get(name) else {
            problems.push(format!("{name} has no layer; add it to LAYERS in xtask and to plan/architecture.md §3"));
            continue;
        };
        for dep in pkg.get("dependencies").and_then(Value::as_array).into_iter().flatten() {
            if dep.get("kind").and_then(Value::as_str) == Some("dev") {
                continue;
            }
            let d = dep.get("name").and_then(Value::as_str).unwrap_or("");
            if own == 0 {
                problems.push(format!("{name} (L0) must have no dependencies, but uses {d}"));
            }
            if let Some(&l) = layer.get(d)
                && l >= own
            {
                problems.push(format!("{name} (L{own}) depends on {d} (L{l}); dependencies must point down"));
            }
            if let Some((_, min, what)) = GATED.iter().find(|(g, _, _)| *g == d)
                && own < *min
            {
                problems.push(format!("{name} (L{own}) uses {d} ({what}), allowed only from L{min} up"));
            }
        }
    }
    report("layers", problems)
}

fn report(gate: &str, problems: Vec<String>) -> Result<(), String> {
    if problems.is_empty() {
        println!("{gate}: ok");
        Ok(())
    } else {
        for p in &problems {
            eprintln!("  - {p}");
        }
        Err(format!("{gate}: {} problem(s)", problems.len()))
    }
}

// ---------------------------------------------------------------------------------------
// assets

const ASSET_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "ico", "icns", "bmp", "webp", "ttf", "otf", "woff", "woff2", "wav", "mp3", "flac", "ogg", "m4a", "aac",
    "mp4", "mov", "dss", "ds2", "wma",
];
const LICENCES: &[&str] = &[
    "ISC",
    "MIT",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "Apache-2.0",
    "OFL-1.1",
    "Ubuntu-Font-1.0",
    "Bitstream-Vera",
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "LicenseRef-Public-Domain",
];
const FORBIDDEN_ORIGINS: &[&str] = &["nch software", "nch.com.au", "express scribe", "nchsoftware"];

fn tracked_files(root: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect())
}

fn assets(root: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(root.join("ATTRIBUTION.toml")).map_err(|e| format!("ATTRIBUTION.toml: {e}"))?;
    let doc: toml::Value = toml::from_str(&text).map_err(|e| format!("ATTRIBUTION.toml: {e}"))?;
    let mut problems = Vec::new();
    let mut by_path = BTreeMap::new();
    for table in ["asset", "bundled"] {
        for e in doc.get(table).and_then(toml::Value::as_array).into_iter().flatten() {
            let field = |k: &str| e.get(k).and_then(toml::Value::as_str).unwrap_or("").to_owned();
            let id = if table == "asset" { field("path") } else { field("title") };
            for k in ["title", "author", "source", "license", "license_file", "usage"] {
                if field(k).is_empty() {
                    problems.push(format!("[[{table}]] {id}: missing \"{k}\""));
                }
            }
            let licence = field("license");
            let terms = licence.replace(['(', ')'], " ");
            let terms: Vec<&str> = terms.split_whitespace().filter(|t| *t != "AND" && *t != "OR").collect();
            if terms.is_empty() || !terms.iter().all(|t| LICENCES.contains(t)) {
                problems.push(format!("[[{table}]] {id}: licence {:?} is not on the allowlist (AGENTS.md §1.2)", field("license")));
            }
            let lic = field("license_file");
            if !lic.is_empty() && !lic.starts_with("http") && !root.join(&lic).is_file() {
                problems.push(format!("[[{table}]] {id}: licence file {lic} is missing"));
            }
            let origin = format!("{} {}", field("author"), field("source")).to_lowercase();
            if FORBIDDEN_ORIGINS.iter().any(|f| origin.contains(f)) {
                problems
                    .push(format!("[[{table}]] {id}: names NCH / Express Scribe as author or source; such assets are forbidden (AGENTS.md §1.1)"));
            }
            if table == "asset" {
                by_path.insert(id, field("sha256"));
            }
        }
    }
    for file in tracked_files(root)? {
        let ext = Path::new(&file).extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
        if !ASSET_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        match by_path.remove(&file) {
            None => problems.push(format!("{file} has no [[asset]] entry in ATTRIBUTION.toml")),
            Some(expected) => {
                let bytes = std::fs::read(root.join(&file)).map_err(|e| format!("{file}: {e}"))?;
                let actual = format!("{:x}", Sha256::digest(&bytes));
                if actual != expected {
                    problems.push(format!("{file}: sha256 is {actual}, ATTRIBUTION.toml says {expected}"));
                }
            }
        }
    }
    for stale in by_path.keys() {
        problems.push(format!("ATTRIBUTION.toml lists {stale}, which is not in the repository"));
    }
    report("assets", problems)
}

// ---------------------------------------------------------------------------------------
// parity

fn parity(root: &Path, partial: bool) -> Result<(), String> {
    let path = root.join("parity").join("express-scribe-features.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc: toml::Value = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let tools_src = std::fs::read_to_string(root.join("crates/automation/src/tools.rs")).map_err(|e| format!("tools.rs: {e}"))?;
    let statuses = ["planned", "partial", "shipped", "na"];
    let tiers = ["P0", "P1", "P2", "P3"];
    let mut problems = Vec::new();
    let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    let features = doc.get("feature").and_then(toml::Value::as_array).cloned().unwrap_or_default();
    for f in &features {
        let s = |k: &str| f.get(k).and_then(toml::Value::as_str).unwrap_or("").to_owned();
        let list = |k: &str| -> Vec<String> {
            f.get(k).and_then(toml::Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect()
        };
        let id = s("id");
        if !seen.insert(id.clone()) {
            problems.push(format!("{id}: duplicate id"));
        }
        let status = s("status");
        if !statuses.contains(&status.as_str()) {
            problems.push(format!("{id}: status {status:?} (allowed: {statuses:?})"));
        }
        if !tiers.contains(&s("tier").as_str()) {
            problems.push(format!("{id}: tier {:?} (allowed: {tiers:?})", s("tier")));
        }
        if s("notes").is_empty() {
            problems.push(format!("{id}: notes must say what is still missing"));
        }
        let evidence = list("evidence");
        if status == "shipped" && evidence.is_empty() {
            problems.push(format!("{id}: shipped needs at least one passing test in evidence"));
        }
        for e in &evidence {
            let Some((file, test)) = e.split_once("::") else {
                problems.push(format!("{id}: evidence {e:?} must be path::test_name"));
                continue;
            };
            match std::fs::read_to_string(root.join(file)) {
                Ok(src) if src.contains(&format!("fn {test}(")) => {}
                Ok(_) => problems.push(format!("{id}: {file} has no test fn {test}")),
                Err(_) => problems.push(format!("{id}: evidence file {file} does not exist")),
            }
        }
        for t in list("tools") {
            if !tools_src.contains(&format!("name: \"{t}\"")) {
                problems.push(format!("{id}: automation tool {t} does not exist"));
            }
        }
        let tier = counts.entry(s("tier")).or_default();
        tier.1 += 1;
        if status == "shipped" {
            tier.0 += 1;
        }
        if partial && status != "shipped" && status != "na" {
            println!("  [{status:>7}] {:<3} {id:<32} {}", s("tier"), s("notes"));
        }
    }
    let shipped: usize = counts.values().map(|c| c.0).sum();
    println!("parity vs Express Scribe Pro: {shipped}/{} features shipped", features.len());
    for (tier, (done, all)) in &counts {
        println!("  {tier}: {done}/{all}");
    }
    report("parity", problems)
}

// ---------------------------------------------------------------------------------------
// wasm and ci

const WASM_CRATES: &[&str] = &["scribecraft-dsp", "scribecraft-media", "scribecraft-transport", "scribecraft-engine", "scribecraft-automation"];

fn wasm(root: &Path) -> Result<(), String> {
    let mut cmd = cargo();
    cmd.current_dir(root).args(["check", "--target", "wasm32-unknown-unknown"]);
    for c in WASM_CRATES {
        cmd.args(["-p", c]);
    }
    run(&mut cmd)?;
    println!("wasm: ok");
    Ok(())
}

fn ci(root: &Path) -> Result<(), String> {
    run(cargo().current_dir(root).args(["fmt", "--all", "--check"]))?;
    run(cargo().current_dir(root).args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]))?;
    run(cargo().current_dir(root).args(["test", "--workspace"]))?;
    layers(root)?;
    assets(root)?;
    parity(root, false)?;
    wasm(root)
}
