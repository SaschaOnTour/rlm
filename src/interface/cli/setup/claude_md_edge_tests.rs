//! Edge-case tests for `claude_md.rs` (CRLF / corrupt / ordering).
//!
//! Split out of `claude_md_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). Happy-path marker tests
//! stay in `claude_md_tests.rs`; this file collects the Windows CRLF
//! regressions, the self-healing corrupt-block cases, and the
//! `normalize_trailing_newline` helper tests.

use super::{
    build_updated_markdown, normalize_trailing_newline, remove_marker_block, MARKER_BEGIN,
    MARKER_END,
};

#[test]
fn build_markdown_preserves_crlf_in_windows_authored_file() {
    // Regression: the rendered rlm block used to be hardcoded LF,
    // so a CRLF-authored CLAUDE.local.md ended up with mixed
    // \r\n\n line endings and a noisy Windows diff. Detecting the
    // EOL from `existing` keeps the output all-CRLF.
    let existing =
        "# Project\r\n\r\n<!-- rlm:begin -->\r\nOLD\r\n<!-- rlm:end -->\r\n\r\n## Footer\r\n";
    let out = build_updated_markdown(existing);
    let bytes = out.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'\n' {
            assert!(
                i > 0 && bytes[i - 1] == b'\r',
                "orphaned \\n at byte {i} in output: {out:?}"
            );
        }
    }
    assert!(out.contains("## Footer"));
    assert!(out.contains("rlm Workflow Instructions"));
}

#[test]
fn build_markdown_crlf_append_when_no_markers() {
    // Same EOL concern, append-to-EOF path.
    let existing = "# Project\r\nLine 1\r\n";
    let out = build_updated_markdown(existing);
    let bytes = out.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'\n' {
            assert!(
                i > 0 && bytes[i - 1] == b'\r',
                "orphaned \\n at byte {i} in output: {out:?}"
            );
        }
    }
}

#[test]
fn build_markdown_heals_begin_without_end() {
    // Corrupt: a user hand-edited the file and deleted the end marker.
    // Instead of appending a second block, we treat begin..EOF as the
    // broken block and replace it cleanly.
    let existing = "# Project\n\n<!-- rlm:begin -->\ngarbage without end marker\n";
    let out = build_updated_markdown(existing);
    assert!(out.starts_with("# Project"));
    assert_eq!(
        out.matches(MARKER_BEGIN).count(),
        1,
        "corrupt block must be replaced, not duplicated"
    );
    assert_eq!(out.matches(MARKER_END).count(), 1);
    assert!(!out.contains("garbage without end marker"));
}

#[test]
fn remove_marker_block_heals_begin_without_end() {
    // --remove on a file with corrupt block should drop begin..EOF so
    // future runs are self-healing.
    let existing = "# A\n\n<!-- rlm:begin -->\nbroken content\nno end marker ever\n";
    let out = remove_marker_block(existing);
    assert!(!out.contains(MARKER_BEGIN));
    assert!(!out.contains("broken content"));
    assert!(out.contains("# A"));
}

#[test]
fn normalize_trailing_newline_handles_crlf() {
    // Regression: CRLF files ending in `\r\n\r\n` must collapse to a
    // single `\r\n`, just like LF files collapse `\n\n` to `\n`.
    let mut crlf = String::from("body\r\n\r\n");
    normalize_trailing_newline(&mut crlf, "\r\n");
    assert_eq!(crlf, "body\r\n");

    let mut crlf_multi = String::from("body\r\n\r\n\r\n");
    normalize_trailing_newline(&mut crlf_multi, "\r\n");
    assert_eq!(crlf_multi, "body\r\n");

    let mut lf = String::from("body\n\n");
    normalize_trailing_newline(&mut lf, "\n");
    assert_eq!(lf, "body\n");

    let mut already_clean_lf = String::from("body\n");
    normalize_trailing_newline(&mut already_clean_lf, "\n");
    assert_eq!(already_clean_lf, "body\n");

    let mut already_clean_crlf = String::from("body\r\n");
    normalize_trailing_newline(&mut already_clean_crlf, "\r\n");
    assert_eq!(already_clean_crlf, "body\r\n");

    let mut no_newline = String::from("body");
    normalize_trailing_newline(&mut no_newline, "\n");
    assert_eq!(no_newline, "body\n");
}

#[test]
fn normalize_uses_detected_eol_on_no_newline_input() {
    // Regression: a CRLF-authored file whose footer lacked a final
    // EOL used to get a lone `\n` tacked on by normalize_trailing_newline,
    // producing mixed line endings at EOF. The detected EOL now
    // propagates so we append `\r\n` instead.
    let mut crlf_no_eol = String::from("body");
    normalize_trailing_newline(&mut crlf_no_eol, "\r\n");
    assert_eq!(crlf_no_eol, "body\r\n");
}

#[test]
fn build_markdown_preserves_crlf_when_footer_has_no_final_eol() {
    // End-to-end version of the regression above: Windows-authored
    // CLAUDE.local.md whose last line (a footer after the rlm
    // block) has no trailing newline must still produce an
    // all-CRLF output after build_updated_markdown.
    let existing =
        "# Project\r\n\r\n<!-- rlm:begin -->\r\nOLD\r\n<!-- rlm:end -->\r\n\r\n## Footer";
    let out = build_updated_markdown(existing);
    let bytes = out.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'\n' {
            assert!(
                i > 0 && bytes[i - 1] == b'\r',
                "orphaned \\n at byte {i} in output: {out:?}"
            );
        }
    }
    assert!(out.ends_with("\r\n"), "output should end with CRLF");
}

#[test]
fn remove_marker_block_handles_crlf_line_endings() {
    // Regression: Windows-authored files with CRLF must not leave a
    // stray `\r` after trimming preceding newlines.
    let existing =
        "# Project\r\n\r\n<!-- rlm:begin -->\r\nbody\r\n<!-- rlm:end -->\r\n\r\n# After\r\n";
    let out = remove_marker_block(existing);
    assert_eq!(
        out, "# Project\r\n\r\n# After\r\n",
        "CRLF trim must not leave stray \\r; got {out:?}"
    );
    // Extra sanity: no `\r` that isn't followed by `\n`.
    let bytes = out.as_bytes();
    for i in 0..bytes.len().saturating_sub(1) {
        if bytes[i] == b'\r' {
            assert_eq!(
                bytes[i + 1],
                b'\n',
                "orphaned \\r at byte {i} in output: {out:?}"
            );
        }
    }
}

#[test]
fn remove_marker_block_handles_wrong_order() {
    // End before begin: also treat as corrupt, strip from the first marker.
    // (Unlikely in practice, but exercise the branch.)
    let existing = "# A\n<!-- rlm:end -->\nfoo\n<!-- rlm:begin -->\n";
    let out = remove_marker_block(existing);
    // The first-found begin is at the later position; we trim from there.
    assert!(out.contains("# A"));
    // End marker that was BEFORE begin stays because it's before our cut point.
    assert!(out.contains(MARKER_END));
    assert!(!out.contains(MARKER_BEGIN));
}
