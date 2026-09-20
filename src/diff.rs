//! Unified-diff parsing: which lines of which files did this PR add?

use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub struct FileDiff {
    pub path: String,
    pub is_new: bool,
    /// New-file line number -> added text. The rules only ever judge what a diff ADDS.
    pub added: BTreeMap<usize, String>,
    /// New-file line number -> unchanged text the hunk carried; the fallback when the full file is unavailable.
    pub context: BTreeMap<usize, String>,
}

pub fn parse(diff_text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut open = false; // is the last entry of `files` the file we are currently inside?
    let mut old_is_null = false; // '--- /dev/null' precedes the '+++' line it describes
    let mut new_line = 0usize;

    for raw in diff_text.lines() {
        if raw.starts_with("diff --git") {
            open = false;
            new_line = 0;
        } else if let Some(old) = raw.strip_prefix("--- ") {
            old_is_null = old.trim() == "/dev/null";
        } else if let Some(target) = raw.strip_prefix("+++ ") {
            let target = target.trim();
            open = target != "/dev/null"; // a deleted file adds nothing to review
            if open {
                let path = target.strip_prefix("b/").unwrap_or(target).to_string();
                files.push(FileDiff { path, is_new: old_is_null, ..Default::default() });
            }
            new_line = 0;
        } else if open && raw.starts_with("@@") {
            new_line = hunk_start(raw).unwrap_or(0);
        } else if open && new_line > 0 {
            let file = files.last_mut().expect("open implies a file");
            if let Some(text) = raw.strip_prefix('+') {
                file.added.insert(new_line, text.to_string());
                new_line += 1;
            } else if raw.starts_with(' ') || raw.is_empty() {
                file.context.insert(new_line, raw.get(1..).unwrap_or("").to_string());
                new_line += 1;
            }
            // '-' lines and '\ No newline' markers do not advance the new-file counter
        }
    }
    files.retain(|f| !f.added.is_empty());
    files
}

/// `@@ -12,3 +15,8 @@` -> 15
fn hunk_start(header: &str) -> Option<usize> {
    let plus = header.split_whitespace().find(|part| part.starts_with('+'))?;
    plus[1..].split(',').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_lines_carry_new_file_numbers() {
        let files = parse(include_str!("../examples/change.diff"));
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["orders/pricing.py", "web/cart.ts"]);
        assert_eq!(files[1].added[&10].trim(), "getTotal(): number {");
    }

    #[test]
    fn deleted_files_are_ignored() {
        let diff = "diff --git a/x.py b/x.py\n--- a/x.py\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a\n-b\n";
        assert!(parse(diff).is_empty());
    }

    #[test]
    fn new_file_is_marked() {
        let diff = "diff --git a/n.py b/n.py\nnew file mode 100644\n--- /dev/null\n+++ b/n.py\n@@ -0,0 +1 @@\n+x = 1\n";
        assert!(parse(diff)[0].is_new);
    }
}
