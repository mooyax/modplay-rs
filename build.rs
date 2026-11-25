//! With the `openmpt` feature, looks for libopenmpt and enables the bindings
//! (cfg `has_openmpt`) used by the comparison tools in examples/. Without the
//! feature nothing is linked: the player and the engines are pure Rust.
//!
//! Windows: put the official "libopenmpt dev" package (from https://lib.openmpt.org)
//! into `<project>/libopenmpt/` (or set LIBOPENMPT_DIR). The DLLs are copied next
//! to the built .exe files automatically.
//! Linux/macOS: install libopenmpt via your package manager (pkg-config is used).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn find_file(dir: &Path, name: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            find_file(&p, name, out);
        } else if p.file_name().map(|f| f.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false) {
            out.push(p);
        }
    }
}

/// Pick the build for our architecture from a multi-arch package
/// (lib/amd64, lib/amd64-legacy, lib/arm64, lib/x86 ...): prefer a folder named
/// exactly "amd64" (etc.), then one that merely contains it.
fn pick(mut v: Vec<PathBuf>, arch: &str) -> Option<PathBuf> {
    let key: &[&str] = match arch {
        "x86_64" => &["amd64", "x86_64", "x64"],
        "aarch64" => &["arm64", "aarch64"],
        _ => &["x86"],
    };
    v.sort();
    let dir_name = |p: &PathBuf| p.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    v.iter()
        .find(|p| key.iter().any(|k| dir_name(p) == *k))
        .or_else(|| v.iter().find(|p| key.iter().any(|k| dir_name(p).starts_with(k) && !dir_name(p).contains("arm64ec"))))
        .cloned()
        .or_else(|| v.first().cloned())
}

/// target/<profile> directory, derived from OUT_DIR. Cargo's layout changed over
/// time (target/release/build/<pkg>-<hash>/out vs. deeper nesting), so look for
/// the "build" folder instead of counting levels.
fn profile_dir() -> Option<PathBuf> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").ok()?);
    out_dir
        .ancestors()
        .find(|p| p.file_name().map(|n| n == "build").unwrap_or(false))
        .and_then(|b| b.parent())
        .map(|p| p.to_path_buf())
}

fn main() {
    println!("cargo::rustc-check-cfg=cfg(has_openmpt)");
    println!("cargo:rerun-if-env-changed=LIBOPENMPT_DIR");
    println!("cargo:rerun-if-env-changed=PT2_NO_OPENMPT");

    if env::var_os("CARGO_FEATURE_OPENMPT").is_none() || env::var_os("PT2_NO_OPENMPT").is_some() {
        return;
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    // mobile builds (mobile/ahmod) use only the built-in Rust engines
    if target_os == "ios" || target_os == "android" {
        return;
    }
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    if target_os == "windows" {
        let dir = env::var_os("LIBOPENMPT_DIR").map(PathBuf::from).unwrap_or_else(|| manifest.join("libopenmpt"));
        println!("cargo:rerun-if-changed={}", dir.display());

        let mut libs = Vec::new();
        find_file(&dir, "libopenmpt.lib", &mut libs);
        let Some(lib) = pick(libs, &arch) else {
            println!("cargo:warning=libopenmpt not found in {} - building without the libopenmpt comparison tools (see README)", dir.display());
            return;
        };
        println!("cargo:rustc-link-search=native={}", lib.parent().unwrap().display());

        // copy the DLLs (libopenmpt.dll + its helper DLLs) next to the executables
        let mut dlls = Vec::new();
        find_file(&dir, "libopenmpt.dll", &mut dlls);
        if let Some(dll) = pick(dlls, &arch) {
            if let Some(profile_dir) = profile_dir() {
                if let Ok(rd) = fs::read_dir(dll.parent().unwrap()) {
                    for e in rd.flatten() {
                        let p = e.path();
                        if p.extension().map(|x| x.eq_ignore_ascii_case("dll")).unwrap_or(false) {
                            let _ = fs::copy(&p, profile_dir.join(p.file_name().unwrap()));
                        }
                    }
                }
            }
        } else {
            println!("cargo:warning=libopenmpt.dll not found in {} - copy it next to the .exe manually", dir.display());
        }
        println!("cargo:rustc-cfg=has_openmpt");
    } else {
        // pkg-config
        let ok = std::process::Command::new("pkg-config")
            .args(["--libs", "libopenmpt"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            let out = std::process::Command::new("pkg-config").args(["--libs-only-L", "libopenmpt"]).output().unwrap();
            for tok in String::from_utf8_lossy(&out.stdout).split_whitespace() {
                if let Some(p) = tok.strip_prefix("-L") {
                    println!("cargo:rustc-link-search=native={p}");
                }
            }
            println!("cargo:rustc-cfg=has_openmpt");
        } else {
            println!("cargo:warning=libopenmpt not found via pkg-config - building without the libopenmpt comparison tools");
        }
    }
}
