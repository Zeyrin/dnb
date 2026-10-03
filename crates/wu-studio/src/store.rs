//! Where the Studio keeps the player's tunes: one `project.ron` a tune under
//! `<data dir>/wheelup/studio/<name>/`, written crash-safe (a temporary file,
//! then an atomic rename), so a crash mid-save never leaves half a tune.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use wu_content::project::Project;

/// `<data dir>/wheelup/studio`, when the OS has a data directory.
pub fn studio_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "wheelup").map(|dirs| dirs.data_dir().join("studio"))
}

/// A folder name for a title: lower case, dashes between words.
pub fn slug(title: &str) -> String {
    let words: Vec<String> = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    if words.is_empty() {
        "tune".to_owned()
    } else {
        words.join("-")
    }
}

/// A folder under `dir` no tune has yet, named after `title`.
pub fn fresh_folder(dir: &Path, title: &str) -> PathBuf {
    let base = slug(title);
    let mut path = dir.join(&base);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{base}-{n}"));
        n += 1;
    }
    path
}

/// Writes `project` to `<folder>/project.ron`, crash-safe.
pub fn save(folder: &Path, project: &Project) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let text = ron::ser::to_string_pretty(project, ron::ser::PrettyConfig::default()).map_err(io::Error::other)?;
    let path = folder.join("project.ron");
    let temporary = folder.join("project.ron.tmp");
    let mut file = fs::File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, &path)
}

pub fn load(folder: &Path) -> io::Result<Project> {
    let text = fs::read_to_string(folder.join("project.ron"))?;
    Project::from_ron(&text).map_err(io::Error::other)
}

/// The tunes saved under `dir`, newest first.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut folders: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter_map(|path| {
            let modified = fs::metadata(path.join("project.ron")).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect();
    folders.sort_by_key(|f| std::cmp::Reverse(f.0));
    folders.into_iter().map(|(_, path)| path).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::templates;

    #[test]
    fn titles_make_folder_names() {
        assert_eq!(slug("New Jungle tune"), "new-jungle-tune");
        assert_eq!(slug("  Dub!  Plate?? "), "dub-plate");
        assert_eq!(slug("…"), "tune");
    }

    #[test]
    fn a_tune_saved_loads_back_the_same() {
        let dir = std::env::temp_dir().join(format!("wu-studio-store-{}", std::process::id()));
        let project = templates().remove(0).project;
        let folder = fresh_folder(&dir, &project.meta.title);
        save(&folder, &project).expect("saves");
        assert_eq!(load(&folder).expect("loads"), project);
        assert!(!folder.join("project.ron.tmp").exists(), "no temporary left behind");
        assert_ne!(
            fresh_folder(&dir, &project.meta.title),
            folder,
            "a second tune gets its own folder"
        );
        assert_eq!(list(&dir), vec![folder.clone()]);
        let _ = fs::remove_dir_all(&dir);
    }
}
