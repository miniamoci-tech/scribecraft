//! `cargo xtask dist`: release build packaged for the current operating system, in `dist/`.
//!
//! - Windows: `ScribeCraft-<v>-windows-x64.zip` (portable) and, when Inno Setup's `iscc` is
//!   installed, `ScribeCraft-<v>-windows-x64-setup.exe` from `packaging/windows/scribecraft.iss`.
//! - macOS: `ScribeCraft.app` (universal when both Apple targets are installed) inside
//!   `ScribeCraft-<v>-macos.dmg`, made with `hdiutil`. Unsigned until a signing identity exists.
//! - Linux/FreeBSD: `ScribeCraft-<v>-<os>-<arch>.tar.gz`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{cargo, run};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BINARIES: &[&str] = &["scribecraft", "scribecraft-cli"];

fn target_dir(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("cannot copy {} to {}: {e}", from.display(), to.display()))
}

fn fresh_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|e| format!("cannot clear {}: {e}", path.display()))?;
    }
    std::fs::create_dir_all(path).map_err(|e| format!("cannot create {}: {e}", path.display()))
}

fn common_files(root: &Path, into: &Path) -> Result<(), String> {
    for f in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE"] {
        copy(&root.join(f), &into.join(f))?;
    }
    copy(&root.join("docs").join("install.md"), &into.join("README-install.md"))
}

pub fn dist(root: &Path) -> Result<(), String> {
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| format!("cannot create dist/: {e}"))?;
    if cfg!(windows) {
        windows(root, &dist)
    } else if cfg!(target_os = "macos") {
        macos(root, &dist)
    } else {
        unix(root, &dist)
    }
}

fn build(root: &Path, target: Option<&str>) -> Result<PathBuf, String> {
    let mut cmd = cargo();
    cmd.current_dir(root).args(["build", "--release", "--locked"]);
    for b in BINARIES {
        cmd.args(["-p", b]);
    }
    if let Some(t) = target {
        cmd.args(["--target", t]);
    }
    run(&mut cmd)?;
    let mut dir = target_dir(root);
    if let Some(t) = target {
        dir.push(t);
    }
    dir.push("release");
    Ok(dir)
}

fn windows(root: &Path, dist: &Path) -> Result<(), String> {
    let bin = build(root, None)?;
    let name = format!("ScribeCraft-{VERSION}-windows-x64");
    let stage = dist.join(&name);
    fresh_dir(&stage)?;
    for b in BINARIES {
        copy(&bin.join(format!("{b}.exe")), &stage.join(format!("{b}.exe")))?;
    }
    common_files(root, &stage)?;
    let zip = dist.join(format!("{name}.zip"));
    let _ = std::fs::remove_file(&zip);
    // bsdtar ships with Windows 10 1803 and later.
    run(Command::new("tar").current_dir(dist).args(["-a", "-c", "-f"]).arg(&zip).arg(&name))?;
    println!("dist: {}", zip.display());

    let iscc = find_iscc();
    match iscc {
        Some(iscc) => {
            run(Command::new(iscc)
                .current_dir(root)
                .arg(format!("/DAppVersion={VERSION}"))
                .arg(format!("/DStageDir={}", stage.display()))
                .arg(format!("/DOutputDir={}", dist.display()))
                .arg(root.join("packaging").join("windows").join("scribecraft.iss")))?;
            println!("dist: {}", dist.join(format!("{name}-setup.exe")).display());
        }
        None => println!("dist: Inno Setup (iscc) not found, so no installer was made; the zip runs without installing"),
    }
    Ok(())
}

fn find_iscc() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ISCC") {
        return Some(PathBuf::from(p));
    }
    let candidates = [
        std::env::var_os("ProgramFiles(x86)").map(|p| PathBuf::from(p).join("Inno Setup 6").join("ISCC.exe")),
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("Inno Setup 6").join("ISCC.exe")),
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Programs").join("Inno Setup 6").join("ISCC.exe")),
    ];
    candidates.into_iter().flatten().find(|p| p.is_file())
}

#[cfg(not(target_os = "macos"))]
fn macos(_: &Path, _: &Path) -> Result<(), String> {
    Err("a .dmg can only be built on macOS (hdiutil, lipo and codesign are macOS tools); use the release workflow's macOS runner".into())
}

#[cfg(target_os = "macos")]
fn macos(root: &Path, dist: &Path) -> Result<(), String> {
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let targets: Vec<&str> = ["aarch64-apple-darwin", "x86_64-apple-darwin"].into_iter().filter(|t| installed.contains(t)).collect();
    let app = dist.join("ScribeCraft.app");
    fresh_dir(&app)?;
    let macos_dir = app.join("Contents").join("MacOS");
    std::fs::create_dir_all(&macos_dir).map_err(|e| e.to_string())?;
    let dirs: Vec<PathBuf> =
        if targets.len() == 2 { targets.iter().map(|t| build(root, Some(t))).collect::<Result<_, _>>()? } else { vec![build(root, None)?] };
    for b in BINARIES {
        let out = macos_dir.join(b);
        if dirs.len() == 2 {
            let mut lipo = Command::new("lipo");
            lipo.arg("-create").arg("-output").arg(&out);
            for d in &dirs {
                lipo.arg(d.join(b));
            }
            run(&mut lipo)?;
        } else if let Some(d) = dirs.first() {
            copy(&d.join(b), &out)?;
        }
    }
    let plist = std::fs::read_to_string(root.join("packaging").join("macos").join("Info.plist")).map_err(|e| format!("Info.plist: {e}"))?;
    std::fs::write(app.join("Contents").join("Info.plist"), plist.replace("@VERSION@", VERSION)).map_err(|e| e.to_string())?;
    // Ad-hoc signature: required to run on Apple silicon; not a Developer ID signature.
    run(Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(&app))?;

    let stage = dist.join("dmg-stage");
    fresh_dir(&stage)?;
    run(Command::new("cp").arg("-R").arg(&app).arg(&stage))?;
    common_files(root, &stage)?;
    std::os::unix::fs::symlink("/Applications", stage.join("Applications")).map_err(|e| e.to_string())?;
    let dmg = dist.join(format!("ScribeCraft-{VERSION}-macos.dmg"));
    let _ = std::fs::remove_file(&dmg);
    run(Command::new("hdiutil")
        .args(["create", "-volname", "ScribeCraft", "-fs", "HFS+", "-format", "UDZO", "-ov", "-srcfolder"])
        .arg(&stage)
        .arg(&dmg))?;
    println!("dist: {}", dmg.display());
    Ok(())
}

fn unix(root: &Path, dist: &Path) -> Result<(), String> {
    let bin = build(root, None)?;
    let name = format!("ScribeCraft-{VERSION}-{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let stage = dist.join(&name);
    fresh_dir(&stage)?;
    for b in BINARIES {
        copy(&bin.join(b), &stage.join(b))?;
    }
    common_files(root, &stage)?;
    run(Command::new("tar").current_dir(dist).arg("-czf").arg(format!("{name}.tar.gz")).arg(&name))?;
    println!("dist: {}", dist.join(format!("{name}.tar.gz")).display());
    Ok(())
}
