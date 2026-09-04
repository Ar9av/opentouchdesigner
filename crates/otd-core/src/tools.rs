//! Finding the command-line tools the editor uses but does not ship.
//!
//! `Command::new("ffmpeg")` searches `PATH`, which is the right answer only
//! when the app was started from a shell. Launched from Finder or the Dock, a
//! bundle inherits `/usr/bin:/bin:/usr/sbin:/sbin` and nothing else — so a
//! Homebrew ffmpeg is invisible, and a feature that plainly works from a
//! terminal reports the tool as missing to everybody who double-clicks. For
//! most users the fallback list *is* the lookup.
//!
//! The search directories are a parameter rather than a constant because the
//! two kinds of tool disagree about them on purpose. The assistant's CLIs are
//! kept current by their own installers in `~/.local/bin`, so a copy in
//! `/usr/local/bin` is usually an old one somebody forgot about and the
//! per-user path has to win. ffmpeg is the other way round: there is no
//! per-user install to prefer. One list would have to be wrong for one of
//! them.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Where a media tool is installed when it is not on `PATH`.
pub const MEDIA_DIRECTORIES: &[&str] = &[
    "/opt/homebrew/bin", // Homebrew on Apple silicon
    "/usr/local/bin",    // Homebrew on Intel, and most manual installs
    "/opt/local/bin",    // MacPorts
    "/usr/bin",
    "/snap/bin",
];

/// A file that exists and can be run.
pub fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Look for `name` on `PATH`, then in `also`.
///
/// `PATH` first, so a deliberately chosen build still beats whatever is in
/// the system directories.
pub fn find(name: &str, also: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    find_in(name, &path, also)
}

/// The same, with the search path given rather than read from the process.
///
/// Separate so a test can pose as a Finder launch without writing to the
/// environment every test running beside it is reading.
pub fn find_in(name: &str, path: &std::ffi::OsStr, also: &[&str]) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    std::env::split_paths(path)
        .chain(also.iter().map(PathBuf::from))
        .map(|dir| dir.join(&exe))
        .find(|candidate| is_executable(candidate))
}

/// `ffmpeg`, found once.
///
/// A miss is as worth caching as a hit: this is asked again on every failed
/// cook, and a failed cook happens every frame.
pub fn ffmpeg() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| find("ffmpeg", MEDIA_DIRECTORIES))
        .as_deref()
}

/// `ffprobe`, found once.
pub fn ffprobe() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| find("ffprobe", MEDIA_DIRECTORIES))
        .as_deref()
}

/// Whether both media tools are present. Lets a caller say "ffmpeg is
/// missing" rather than guessing at why a file would not open.
pub fn media_tools_installed() -> bool {
    ffmpeg().is_some() && ffprobe().is_some()
}

/// One wording for the one thing the user has to do about it.
pub fn missing_ffmpeg() -> String {
    "ffmpeg is not installed, or not where this app can find it \
     (macOS: brew install ffmpeg)"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_is_found_with_the_path_a_double_clicked_app_gets() {
        // The bug this guards: launched from Finder, the app inherits only
        // the system directories, so a Homebrew ffmpeg vanishes and every
        // media node claims the tool is not installed on a machine where it
        // is plainly there.
        //
        // Skipped where ffmpeg genuinely is not installed — there is nothing
        // to find, and this is a search test, not an ffmpeg test.
        if ffmpeg().is_none() {
            return;
        }
        let bundle = std::ffi::OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin");
        let found = find_in("ffmpeg", bundle, MEDIA_DIRECTORIES);
        assert!(
            found.is_some(),
            "ffmpeg is installed but was not found with a Finder-style PATH"
        );
        assert!(is_executable(&found.unwrap()));
    }

    #[test]
    fn the_search_path_wins_over_the_fallbacks() {
        // A build somebody put on their PATH on purpose must not lose to
        // whatever an installer left in /usr/local/bin.
        let dir = std::env::temp_dir().join("otd-tools-test");
        let _ = std::fs::create_dir_all(&dir);
        let chosen = dir.join("otd-fake-tool");
        std::fs::write(&chosen, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&chosen, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let found = find_in(
            "otd-fake-tool",
            dir.as_os_str(),
            &["/usr/bin", "/usr/local/bin"],
        );
        assert_eq!(found.as_deref(), Some(chosen.as_path()));
        let _ = std::fs::remove_file(&chosen);
    }

    #[test]
    fn a_tool_that_is_not_anywhere_is_none() {
        assert!(find("otd-definitely-not-a-real-tool", MEDIA_DIRECTORIES).is_none());
    }
}
