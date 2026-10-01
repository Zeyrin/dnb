//! Every shipped file is accounted for in `content/licenses.ron`, and nothing
//! listed there has gone missing. No third-party audio, ever: see PROMPT.md §9.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const ALLOWED: [&str; 4] = ["OFL-1.1", "CC0-1.0", "CC-BY-4.0", "original"];
const MANIFEST: &str = "content/licenses.ron";

#[derive(Debug, Deserialize)]
struct Entry {
    path: String,
    source: String,
    author: String,
    license: String,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files_under(root: &Path, dir: &str, found: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(root.join(dir)) else {
        return;
    };
    for entry in entries.flatten() {
        let relative = format!("{dir}/{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            files_under(root, &relative, found);
        } else if !relative.ends_with(".gitkeep") && relative != MANIFEST {
            found.insert(relative);
        }
    }
}

#[test]
fn every_shipped_file_has_a_known_licence() {
    let root = workspace_root();
    let text = fs::read_to_string(root.join(MANIFEST)).expect("the manifest exists");
    let entries: Vec<Entry> = ron::from_str(&text).expect("the manifest parses");

    let mut shipped = BTreeSet::new();
    for dir in ["assets", "content"] {
        files_under(&root, dir, &mut shipped);
    }
    let listed: BTreeSet<String> = entries.iter().map(|e| e.path.clone()).collect();

    let unlisted: Vec<_> = shipped.difference(&listed).collect();
    assert!(unlisted.is_empty(), "add these to {MANIFEST}: {unlisted:?}");
    let missing: Vec<_> = listed.difference(&shipped).collect();
    assert!(missing.is_empty(), "listed in {MANIFEST} but not found: {missing:?}");
    for entry in &entries {
        assert!(
            ALLOWED.contains(&entry.license.as_str()),
            "{}: licence {}",
            entry.path,
            entry.license
        );
        assert!(
            !entry.source.is_empty() && !entry.author.is_empty(),
            "{}: source and author",
            entry.path
        );
    }
}
