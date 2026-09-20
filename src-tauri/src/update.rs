//! Release check and self-update, ported from gitdashy's `src-tauri/src/update.rs`.
//!
//! ponytail: this updates the APP binary only. A meridian-server you host is updated the way you
//! deploy anything else. A desktop updater that reached across and replaced a remote deployment
//! would be a very surprising thing for a "check for updates" button to do.
//!
//! ponytail: gitdashy's file could not be copied whole. Half of it serves a changelog screen and a
//! `seen` field in a settings file, neither of which exists here. What is ported is the half that
//! answers "is there a newer release" and "replace me with it": the version comparison, the
//! asset-ready check, the sha256 gate, the download-over-self and the re-exec, all unchanged.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const REPO: &str = "MartinRovang/meridian";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// (major, minor, patch) from "v1.2.3" or "1.2.3". ponytail: plain numeric tags, no pre-release parsing.
pub fn vkey(v: &str) -> (u64, u64, u64) {
    let mut it = v
        .trim_start_matches('v')
        .split('.')
        .map(|x| x.parse::<u64>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

/// Run `cmd`, output discarded on stderr, stdout captured; None when it failed or hung past `secs`.
/// ponytail: spawn and poll, kill on expiry. The stdlib has no timeout on `wait`.
fn run_capture(cmd: &mut Command, secs: u64) -> Option<String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    // ponytail: read on a thread, so a chatty child cannot fill the pipe and wait on us while we wait on it
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        if let Ok(Some(st)) = child.try_wait() {
            break st;
        }
        if start.elapsed() > Duration::from_secs(secs) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let out = reader.join().ok()?;
    status.success().then_some(out)
}

/// The highest vX.Y.Z tag in `git ls-remote --tags` output, or "".
pub fn newest_tag(ls_remote: &str) -> String {
    let re = regex::Regex::new(r"(?m)refs/tags/v(\d+(?:\.\d+)*)$").expect("static regex");
    re.captures_iter(ls_remote)
        .map(|c| c[1].to_string())
        .max_by_key(|v| vkey(v))
        .unwrap_or_default()
}

/// Highest vX.Y.Z tag on origin, or "". `git ls-remote`, so no gh auth and no API rate limit.
pub fn latest_release() -> String {
    let mut cmd = Command::new("git");
    cmd.args([
        "ls-remote",
        "--tags",
        "--refs",
        &format!("https://github.com/{REPO}.git"),
    ]);
    // not a clone, no origin, offline: all the same answer, which is "no update to offer".
    run_capture(&mut cmd, 10)
        .map(|out| newest_tag(&out))
        .unwrap_or_default()
}

/// `tag` when it is newer than `current`, else "".
pub fn newer(tag: &str, current: &str) -> String {
    if !tag.is_empty() && vkey(tag) > vkey(current) {
        tag.to_string()
    } else {
        String::new()
    }
}

/// The released version newer than ours THAT THIS MACHINE CAN ACTUALLY DOWNLOAD, or "".
///
/// ponytail: a TAG is not a downloadable release. ci.yml creates the release FIRST and uploads the
/// binaries after: for the minutes that takes, and permanently if a build job fails, ls-remote sees
/// a version whose asset for this machine is not there. Offering it would put an update prompt in
/// front of every user and 404 on every press. So the offer waits for the thing it would download.
pub fn update_available() -> String {
    offer(&newer(&latest_release(), VERSION), asset_ready)
}

/// `tag`, once `ready` says its asset is there. "" for no newer tag, so `ready` is never asked then.
fn offer(tag: &str, ready: impl FnOnce(&str) -> bool) -> String {
    if tag.is_empty() || !ready(tag) {
        return String::new();
    }
    tag.to_string()
}

/// A plain agent that gives up after `secs`.
fn agent(secs: u64) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(secs)))
            .build(),
    )
}

/// Whether this machine's asset for `version` can be fetched. One HEAD, and only on a newer tag, so
/// the normal check pays nothing. No token on it, same as the download.
fn asset_ready(version: &str) -> bool {
    let url = format!(
        "https://github.com/{REPO}/releases/download/v{version}/{}",
        asset_name()
    );
    agent(15).head(&url).call().is_ok()
}

/// The release asset for this machine: meridian-{linux-x86_64,macos-arm64,macos-x86_64,windows-x86_64.exe}.
pub fn asset_name() -> String {
    asset_for(std::env::consts::OS, std::env::consts::ARCH)
}

fn asset_for(os: &str, arch: &str) -> String {
    let arch = match arch {
        "aarch64" => "arm64",
        other => other,
    };
    match os {
        "macos" => format!("meridian-macos-{arch}"),
        "windows" => format!("meridian-windows-{arch}.exe"),
        other => format!("meridian-{other}-{arch}"),
    }
}

/// Install `version` over this executable and re-exec. Returns the error, or never returns.
pub fn apply_update(version: &str) -> String {
    match install(version).and_then(|exe| reexec(&exe)) {
        Ok(never) => never,
        Err(e) => e.chars().take(60).collect(),
    }
}

/// Download the release asset beside the running executable and rename it over it. The new path.
fn install(version: &str) -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let url = format!(
        "https://github.com/{REPO}/releases/download/v{version}/{}",
        asset_name()
    );
    // ponytail: a plain agent here, redirects and all: releases bounce to objects.githubusercontent.com
    // and no token is on this request, so there is nothing to leak.
    let agent = agent(300);
    let mut resp = agent
        .get(&url)
        .call()
        .map_err(|e| format!("{}: {e}", asset_name()))?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(512 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("empty download".into());
    }
    // ponytail: the release publishes <asset>.sha256 beside the binary; refuse anything that does not
    // match it. This is a tamper check on the download, not a signature: whoever can write the release
    // writes both files. Real provenance needs a key the release job signs with.
    let want = agent
        .get(format!("{url}.sha256"))
        .call()
        .map_err(|e| format!("{}.sha256: {e}", asset_name()))?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    verify(&bytes, &want)?;
    // ponytail: written beside the executable, so the rename is on one filesystem and atomic.
    let tmp = exe.with_file_name(format!(".{}.{}.new", asset_name(), std::process::id()));
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        // ponytail: windows will not overwrite a running exe, but it will let it be renamed away.
        let _ = std::fs::rename(&exe, exe.with_extension("old"));
    }
    if let Err(e) = std::fs::rename(&tmp, &exe) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(exe)
}

/// Ok when `bytes` hash to the digest `sums` names. `sums` is one `sha256sum` line: "<hex>  <name>".
fn verify(bytes: &[u8], sums: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let want = sums
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if want.len() != 64 || !want.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("no checksum published".into());
    }
    let got = format!("{:x}", Sha256::digest(bytes));
    if got != want {
        return Err(format!("checksum mismatch: {}", &got[..12]));
    }
    Ok(())
}

/// Replace this process with `exe` and the same arguments. Only returns on failure.
fn reexec(exe: &std::path::Path) -> Result<String, String> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(Command::new(exe).args(&args).exec().to_string())
    }
    #[cfg(not(unix))]
    {
        let status = Command::new(exe)
            .args(&args)
            .status()
            .map_err(|e| e.to_string())?;
        std::process::exit(status.code().unwrap_or(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LS_REMOTE: &str =
        "abc\trefs/tags/v0.9.0\ndef\trefs/tags/v1.10.0\nfed\trefs/tags/v1.2.0\nx\trefs/tags/v2.0.0-rc1\n";

    #[test]
    fn vkey_is_numeric() {
        assert_eq!(vkey("v1.2.3"), (1, 2, 3));
        assert_eq!(vkey("1.10"), (1, 10, 0));
        assert!(vkey("1.10.0") > vkey("1.2.0"));
    }

    #[test]
    fn update_available_offers_newer_release() {
        // numeric compare, not lexical; and the -rc1 tag is not offered to anyone
        assert_eq!(newest_tag(LS_REMOTE), "1.10.0");
        assert_eq!(newer(&newest_tag(LS_REMOTE), "1.2.0"), "1.10.0");
    }

    #[test]
    fn update_available_silent_when_current() {
        assert_eq!(newer(&newest_tag(LS_REMOTE), "1.10.0"), "");
        assert_eq!(newer(&newest_tag(LS_REMOTE), "2.0.0"), "");
    }

    #[test]
    fn a_tagged_release_with_no_asset_for_this_machine_is_not_offered() {
        assert_eq!(offer("1.10.0", |_| false), "");
        assert_eq!(offer("1.10.0", |_| true), "1.10.0");
        // "" short-circuits: ready is never consulted, so a normal check makes no request
        assert_eq!(offer("", |_| panic!("must not be asked")), "");
    }

    #[test]
    fn asset_names() {
        assert_eq!(asset_for("linux", "x86_64"), "meridian-linux-x86_64");
        assert_eq!(asset_for("macos", "aarch64"), "meridian-macos-arm64");
        assert_eq!(asset_for("macos", "x86_64"), "meridian-macos-x86_64");
        assert_eq!(
            asset_for("windows", "x86_64"),
            "meridian-windows-x86_64.exe"
        );
    }

    #[test]
    fn a_download_that_does_not_match_its_checksum_is_refused() {
        let bytes = b"hello";
        use sha2::{Digest, Sha256};
        let good = format!("{:x}  meridian", Sha256::digest(bytes));
        assert!(verify(bytes, &good).is_ok());
        assert!(verify(b"tampered", &good).is_err());
        assert!(verify(bytes, "not-a-checksum").is_err());
        assert!(verify(bytes, "").is_err());
    }
}
