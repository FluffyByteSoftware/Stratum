//! File:     stratum-networking/src/client_version.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! Which client versions the server lets in.  The list lives in
//! expected_client_version.txt at the repo root, one version a line, so
//! changing it never means digging through code.
//!
//! The file gets built into the server rather than read while it runs.  A
//! different list is a different server, so it comes with the build: edit
//! the file, rebuild, and the new list is in.
//!
//! Nothing in here logs or touches the network, same as protocol.rs.

// Rust note: include_str! pulls the whole file into the program as text
// when it compiles.  The path is from this file's folder, so two folders up
// is the repo root.  Cargo notices when the file changes and rebuilds.
const LIST: &str = include_str!("../../expected_client_version.txt");

/// Makes sure the list has at least one version on it.  Called by start(),
/// so a list with nothing on it stops the server with a sentence for the
/// admin, rather than a server that turns every client away.
pub fn check() -> Result<(), String> {
    if versions_in(LIST).is_empty() {
        return Err("expected_client_version.txt has no client versions on it, \
        so no client could get in.  Put at least one in and rebuild.".to_string());
    }
    Ok(())
}

/// True if a client saying it is this version may come in.  It has to match
/// a line on the list exactly.
pub fn accepted(version: &str) -> bool {
    versions_in(LIST).iter().any(|listed| listed == version)
}

/// Every version on a list, in the order they're written.  Blank lines and
/// lines starting with # are skipped, and the spaces around each one come
/// off (a Windows line ending too, if the file ever picks one up).
fn versions_in(text: &str) -> Vec<String> {
    let mut versions = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        versions.push(line.to_string());
    }
    versions
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// These run on lists written right here, not the real file, so changing the
// real list never breaks a test.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let text = "# The versions we let in.\n\n0.0.1\n\n# Old, going soon.\n0.0.0\n";
        assert_eq!(versions_in(text), vec!["0.0.1".to_string(), "0.0.0".to_string()]);
    }

    #[test]
    fn spaces_and_windows_line_endings_come_off() {
        let text = "  0.0.1  \r\n0.0.2\r\n";
        assert_eq!(versions_in(text), vec!["0.0.1".to_string(), "0.0.2".to_string()]);
    }

    #[test]
    fn a_list_of_only_comments_is_empty() {
        let text = "# Nothing here yet.\n\n   \n# Still nothing.\n";
        assert!(versions_in(text).is_empty());
    }
}