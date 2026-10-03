//! A template per subgenre, taken from the songs that ship with the game: the
//! first of each subgenre lends its tempo, swing, kit, bass sounds and mix, its
//! first drop's beat and bass line, laid out as an intro, a drop and an outro.

use std::collections::BTreeMap;

use wu_content::project::{Meta, Pattern, Project, Section};
use wu_content::songs::BUILTIN;

/// The patterns a template starts from, by these names.
pub const BEAT: &str = "beat";
pub const BASS: &str = "bass";

/// A tune to start from.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub subgenre: String,
    pub project: Project,
}

/// One per subgenre, in the order the songs ship.
pub fn templates() -> Vec<Template> {
    let mut templates: Vec<Template> = Vec::new();
    for song in BUILTIN {
        let Ok(source) = Project::from_ron(song.project) else {
            continue;
        };
        let subgenre = source.meta.subgenre.clone();
        if subgenre.is_empty() || subgenre == "Lesson" || templates.iter().any(|t| t.subgenre == subgenre) {
            continue;
        }
        if let Some(project) = template_of(&source) {
            templates.push(Template { subgenre, project });
        }
    }
    templates
}

/// `source` cut down to a starter: its first drop's beat and bass line.
fn template_of(source: &Project) -> Option<Project> {
    let drop = source.arrangement.iter().find(|section| section.hype)?;
    let pattern = |wanted: fn(&Pattern) -> bool| {
        drop.play
            .iter()
            .filter_map(|name| source.patterns.get(name))
            .find(|&p| wanted(p))
            .cloned()
    };
    let beat = pattern(|p| matches!(p, Pattern::Drums { .. }))?;
    let mut patterns = BTreeMap::from([(BEAT.to_owned(), beat)]);
    if let Some(bass) = pattern(|p| matches!(p, Pattern::Bass { .. })) {
        patterns.insert(BASS.to_owned(), bass);
    }
    let with_bass: Vec<String> = patterns.keys().cloned().collect();
    let section = |name: &str, bars: i64, play: Vec<String>, hype: bool| Section {
        name: name.to_owned(),
        bars,
        play,
        hype,
        fill: None,
        gap: 0,
        lesson: None,
    };
    Some(Project {
        version: source.version,
        meta: Meta {
            title: format!("New {} tune", source.meta.subgenre),
            artist: "You".to_owned(),
            key: source.meta.key.clone(),
            subgenre: source.meta.subgenre.clone(),
        },
        bpm: source.bpm,
        swing: source.swing,
        kit: source.kit.clone(),
        mix: source.mix,
        bass: source.bass.clone(),
        tracks: BTreeMap::new(),
        patterns,
        arrangement: vec![
            section("Intro", 8, vec![BEAT.to_owned()], false),
            section("Drop", 16, with_bass, true),
            section("Outro", 8, vec![BEAT.to_owned()], false),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_subgenre_gives_a_template_that_compiles() {
        let templates = templates();
        assert!(templates.len() >= 6, "{} templates", templates.len());
        for template in &templates {
            let song = template.project.compile().expect("compiles");
            assert!(!song.drums.is_empty(), "{}: no drums", template.subgenre);
            assert!(!song.hype.is_empty(), "{}: no drop", template.subgenre);
            assert!(template.project.patterns.contains_key(BEAT));
        }
        let names: std::collections::BTreeSet<&str> = templates.iter().map(|t| t.subgenre.as_str()).collect();
        assert_eq!(names.len(), templates.len(), "one a subgenre");
    }
}
