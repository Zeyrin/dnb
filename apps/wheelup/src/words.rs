//! The game's words, in English and in French. A line on screen goes through
//! [`tr`]: its English is the key, the French table its translation, and a line
//! the table doesn't have stays in English. Lines with values in them are
//! templates, each `{}` filled in order by [`fill`].

use std::fmt::Display;

use wu_content::settings::Language;

/// `english`, in `language`.
pub fn tr(language: Language, english: &'static str) -> &'static str {
    match language {
        Language::English => english,
        Language::French => french(english).unwrap_or(english),
    }
}

/// `template` with each `{}` replaced by the next of `values`.
pub fn fill(template: &str, values: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16 * values.len());
    let mut values = values.iter();
    let mut rest = template;
    while let Some(at) = rest.find("{}") {
        out.push_str(&rest[..at]);
        if let Some(value) = values.next() {
            out.push_str(&value.to_string());
        }
        rest = &rest[at + 2..];
    }
    out.push_str(rest);
    out
}

/// `value` to `places` decimals, with a decimal comma in French.
pub fn decimal(language: Language, value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    match language {
        Language::English => text,
        Language::French => text.replace('.', ","),
    }
}

/// A song's key, in `language`: "F# minor" is "fa dièse mineur" in French. A key
/// it can't read stays as written.
pub fn key(language: Language, key: &str) -> String {
    let Language::French = language else {
        return key.to_owned();
    };
    let Some((tonic, mode)) = key.split_once(' ') else {
        return key.to_owned();
    };
    let (letter, accidental) = tonic.split_at(tonic.len().min(1));
    let note = match letter {
        "C" => "do",
        "D" => "ré",
        "E" => "mi",
        "F" => "fa",
        "G" => "sol",
        "A" => "la",
        "B" => "si",
        _ => return key.to_owned(),
    };
    let accidental = match accidental {
        "" => "",
        "#" => " dièse",
        "b" => " bémol",
        _ => return key.to_owned(),
    };
    let mode = match mode {
        "minor" => "mineur",
        "major" => "majeur",
        "dorian" => "dorien",
        "phrygian" => "phrygien",
        "lydian" => "lydien",
        "mixolydian" => "mixolydien",
        "aeolian" => "éolien",
        "locrian" => "locrien",
        _ => return key.to_owned(),
    };
    format!("{note}{accidental} {mode}")
}

/// The difficulty's name, in `language`.
pub fn difficulty(language: Language, difficulty: wu_chart::Difficulty) -> &'static str {
    tr(language, difficulty.name())
}

/// English, then French: every line the game says in French.
const FRENCH: &[(&str, &str)] = &[
    // The tabs and the header.
    ("SONGS", "MORCEAUX"),
    ("TOUR", "TOURNÉE"),
    ("JAM", "JAM"),
    ("CONTROLLER", "MANETTE"),
    ("CALIBRATE", "CALIBRAGE"),
    ("SETTINGS", "RÉGLAGES"),
    ("Tab / CREATE", "Tab / CREATE"),
    (
        "a junglist rhythm game & controller-first DAW",
        "un jeu de rythme junglist et un studio à la manette",
    ),
    // Difficulties.
    ("Beginner", "Débutant"),
    ("Easy", "Facile"),
    ("Medium", "Moyen"),
    ("Hard", "Difficile"),
    ("Junglist", "Junglist"),
    // The notice.
    ("PHOTOSENSITIVITY WARNING", "AVERTISSEMENT PHOTOSENSIBILITÉ"),
    (
        "A very small share of people may have a seizure when they see certain light\n\
         patterns or flashing lights, even with no history of epilepsy. If you or anyone\n\
         in your family has an epileptic condition, ask a doctor before playing. Stop\n\
         at once if you feel dizzy, your sight blurs, your eyes or muscles twitch or you\n\
         feel disoriented, and see a doctor.\n\n\
         WHEEL UP! never flashes more than three times a second: nothing flashes on the\n\
         beat, the lights only swell. Play in a lit room, sitting back from the screen,\n\
         and take a break every hour.",
        "Chez une très petite partie des gens, certains motifs lumineux ou lumières\n\
             clignotantes peuvent provoquer une crise, même sans antécédent d'épilepsie. Si\n\
             vous ou un proche êtes épileptique, demandez l'avis d'un médecin avant de jouer.\n\
             Arrêtez tout de suite en cas de vertige, de vue trouble, de contractions des yeux\n\
             ou des muscles ou de désorientation, et consultez un médecin.\n\n\
             WHEEL UP! ne flashe jamais plus de trois fois par seconde : rien ne flashe sur le\n\
             temps, les lumières ne font qu'enfler. Jouez dans une pièce éclairée, à bonne\n\
             distance de l'écran, et faites une pause toutes les heures.",
    ),
    ("✕ / Space: carry on", "✕ / Espace : continuer"),
    // The settings.
    ("Language", "Langue"),
    ("Controller layout", "Disposition"),
    ("Note speed", "Vitesse des notes"),
    ("Audio", "Audio"),
    ("WHEEL UP! flare", "Flash WHEEL UP!"),
    ("Motion", "Mouvement"),
    ("Reel: ↑ kick, ↓ snare", "Reel : ↑ kick, ↓ caisse"),
    ("Drummer: ↓ kick, ↑ snare", "Batteur : ↓ kick, ↑ caisse"),
    ("full", "à fond"),
    ("half", "à moitié"),
    ("off", "non"),
    ("reduced", "réduit"),
    ("Bass on the triggers", "Basse aux gâchettes"),
    (
        "Off: the pads play the drums and the bass plays itself. On: from Medium up,\n\
         the bass line falls on L2 and R2 too, held as long as each note.",
        "Non : les pads jouent la batterie et la basse joue toute seule. Oui : dès Moyen,\n\
         la basse tombe aussi sur L2 et R2, tenue tant que dure chaque note.",
    ),
    ("Live", "Live"),
    ("Classic", "Classique"),
    ("The language the game speaks.", "La langue du jeu."),
    (
        "Which D-pad button plays the kick: the reel's own mapping, or the kick under\n\
         the thumb's resting point like a drummer's foot.",
        "Quel bouton de la croix joue le kick : la disposition de la vidéo d'origine,\n\
             ou le kick sous le pouce au repos, comme le pied d'un batteur.",
    ),
    (
        "How fast notes fall: 1× shows two seconds of the song ahead, 2× one.",
        "La vitesse des notes : à 1×, la piste montre deux secondes du morceau, à 2× une seule.",
    ),
    (
        "Live: your presses play your part. Classic: the whole song plays and a miss\n\
         mutes your part, for outputs too slow to play along to (Bluetooth, TVs).",
        "Live : tes frappes jouent ta partie. Classique : tout le morceau joue et un raté\n\
             coupe ta partie, pour les sorties trop lentes pour jouer en direct (Bluetooth, TV).",
    ),
    (
        "The one warm glow and colour split a WHEEL UP! throws, as bright as you like.",
        "La lueur chaude et la séparation des couleurs d'un WHEEL UP!, aussi fortes que tu veux.",
    ),
    (
        "Reduced: lasers and searchlights hold still, no tape band rolls by, the crowd\n\
         and the speakers stop moving, and no record leaps over the highway.",
        "Réduit : lasers et projecteurs immobiles, plus de bande VHS, la foule et les\n\
             enceintes ne bougent plus, et aucun vinyle ne bondit au-dessus de la piste.",
    ),
    (
        "↑ ↓ choose · ← → change · saved at once",
        "↑ ↓ choisir · ← → changer · enregistré aussitôt",
    ),
    // The songs screen.
    ("SELECT A TUNE", "CHOISIS UN MORCEAU"),
    ("Song", "Morceau"),
    ("Difficulty", "Difficulté"),
    ("Practice", "Practice"),
    ("Tempo", "Tempo"),
    ("Autoplay (selecta bot)", "Autoplay (selecta bot)"),
    ("No-Fail", "No-Fail"),
    ("on", "oui"),
    ("always", "toujours"),
    ("a lesson", "une leçon"),
    ("Recorded", "Enregistré"),
    ("Lesson", "Leçon"),
    ("Your tune", "Ton morceau"),
    ("{} of {}: {}", "{} sur {} : {}"),
    ("loop {}, bars {}–{}", "boucle {}, mes. {}–{}"),
    ("off: the whole song", "non : tout le morceau"),
    (
        "practice: {}'s {} notes, round and round until you leave · no fail, no record",
        "practice : {} en boucle ({} notes) · sans échec ni record",
    ),
    (
        "{} lessons, {} notes: each control in turn, no fail, timing loose",
        "{} leçons, {} notes : chaque geste à son tour, sans échec, timing large",
    ),
    ("{} notes on {} pads", "{} notes sur {} pads"),
    (" · 1 roll (L1 / R1 join in)", " · 1 roulement (L1 / R1 s'y joignent)"),
    (
        " · {} rolls (L1 / R1 join in)",
        " · {} roulements (L1 / R1 s'y joignent)",
    ),
    (" · the bass on R2: {} holds", " · la basse sur R2 : {} tenues"),
    (
        " · the bass on L2 and R2: {} holds",
        " · la basse sur L2 et R2 : {} tenues",
    ),
    ("HOW TO PLAY\nstart here", "COMMENT JOUER\ncommence ici"),
    ("no record yet on {}", "pas encore de record en {}"),
    ("BEST ON {}\n{}", "RECORD EN {}\n{}"),
    (
        "Your tune plays as recorded; a miss muffles it until your next hit",
        "Ton morceau joue tel qu'enregistré ; un raté l'étouffe jusqu'à ta prochaine frappe",
    ),
    ("Getting your tune ready…", "Préparation de ton morceau…"),
    (
        "Bluetooth output: its delay makes playing live hard. Try Audio: Classic",
        "Sortie Bluetooth : son retard rend le jeu en direct difficile. Essaie Audio : Classique",
    ),
    (
        "Live audio: your presses play your part",
        "Audio live : tes frappes jouent ta partie",
    ),
    (
        "Classic audio: the whole song plays; a miss mutes your part until your next hit",
        "Audio classique : tout le morceau joue ; un raté coupe ta partie jusqu'à ta prochaine frappe",
    ),
    (
        "↑ ↓ choose · ← → change · ✕ / Space play · drop a tune on this window to play it",
        "↑ ↓ choisir · ← → changer · ✕ / Espace jouer · glisse un morceau sur la fenêtre pour le jouer",
    ),
    // The tour.
    (
        "PIRATE RADIO TOUR · {} ◆ earned on {}",
        "PIRATE RADIO TOUR · {} ◆ gagnés en {}",
    ),
    ("on the way", "à venir"),
    ("{} ◆ to get in", "{} ◆ pour entrer"),
    (
        "Its tunes are still being cut.",
        "Ses morceaux sont encore en cours d'écriture.",
    ),
    ("SET", "SETLIST"),
    ("ENCORE", "RAPPEL"),
    (
        "ENCORE: {} ◆ from the set opens it",
        "RAPPEL : {} ◆ de la setlist pour l'ouvrir",
    ),
    ("CHALLENGE: {}", "DÉFI : {}"),
    ("  ✓ done", "  ✓ réussi"),
    (
        "Closed: {} ◆ on the tour opens it ({} so far).",
        "Fermé : {} ◆ sur la tournée pour l'ouvrir ({} pour l'instant).",
    ),
    (
        "a full combo on any tune of the set",
        "un full combo sur un morceau de la setlist",
    ),
    (
        "every tune of the set at {} or better",
        "tous les morceaux de la setlist en {} ou mieux",
    ),
    ("{} stars from this stop", "{} étoiles sur cette étape"),
    (
        "↑ ↓ choose · ← → difficulty, or a stop's tune · ✕ / Space play it · every real run counts here, on the tour or not",
        "↑ ↓ choisir · ← → difficulté ou morceau · ✕ / Espace jouer · toute vraie partie compte, tournée ou non",
    ),
    // The results.
    ("FIRST RECORD ON THIS TUNE", "PREMIER RECORD SUR CE MORCEAU"),
    ("NEW BEST!   was {}", "NOUVEAU RECORD !   l'ancien : {}"),
    ("best {}", "record {}"),
    (
        "lessons set no records: pick a tune and a difficulty next",
        "une leçon ne pose pas de record : choisis maintenant un morceau et une difficulté",
    ),
    ("no record: {}", "pas de record : {}"),
    ("the selecta bot played", "le selecta bot a joué"),
    ("No-Fail was on", "No-Fail était activé"),
    ("practice tempo", "tempo d'entraînement"),
    ("PLUG PULLED", "DÉBRANCHÉ"),
    ("LESSON COMPLETE", "LEÇON TERMINÉE"),
    ("TUNE COMPLETE", "MORCEAU TERMINÉ"),
    (
        "score      {}\naccuracy   {} %\nmax combo  {} / {}\n{}\noverhits   {}{}",
        "score      {}\nprécision  {} %\ncombo max  {} / {}\n{}\nfrappes en trop {}{}",
    ),
    (
        "\nholds      {} / {} kept to the end",
        "\ntenues     {} / {} jusqu'au bout",
    ),
    ("on average {} ms late", "en moyenne {} ms en retard"),
    ("on average {} ms early", "en moyenne {} ms en avance"),
    ("right on the beat", "pile sur le temps"),
    (
        "early  ←   timing of every hit, 10 ms per bar   →  late      {}",
        "en avance  ←   timing de chaque frappe, 10 ms par barre   →  en retard      {}",
    ),
    (
        "✕ / Space play again · ○ / L back\n{}",
        "✕ / Espace rejouer · ○ / L retour\n{}",
    ),
    ("replay saved: {}", "replay enregistré : {}"),
    ("replay not saved: {}", "replay non enregistré : {}"),
    (
        "replay not saved: no data folder",
        "replay non enregistré : pas de dossier de données",
    ),
    (" · selecta bot", " · selecta bot"),
    // The highway.
    ("VIBE", "VIBE"),
    ("Count-in", "Décompte"),
    ("{} ms late", "{} ms en retard"),
    ("{} ms early", "{} ms en avance"),
    ("PRACTICE · {}", "PRACTICE · {}"),
    ("pass {}", "passage {}"),
    (" · best {} %\n{} %", " · record {} %\n{} %"),
    ("LESSON {} OF {}", "LEÇON {} SUR {}"),
    ("{} combo · ×{}\naccuracy {} %", "{} combo · ×{}\nprécision {} %"),
    ("PAUSED", "PAUSE"),
    ("Wait for my hit", "Attendre ma frappe"),
    ("in practice", "en practice"),
    ("Resume", "Reprendre"),
    ("Restart the song", "Recommencer le morceau"),
    ("Leave the song", "Quitter le morceau"),
    ("Quit WHEEL UP!", "Quitter WHEEL UP!"),
    ("WHEEL UP!  multiplier doubled", "WHEEL UP!  multiplicateur doublé"),
    ("pulling up…", "on remonte…"),
    ("HYPE {} %  ·  L3 + R3: WHEEL UP!", "HYPE {} %  ·  L3 + R3 : WHEEL UP!"),
    ("HYPE {} %", "HYPE {} %"),
    // Calibration.
    (
        "any pad, Space or OPTIONS: continue · Tab / CREATE: next screen · Esc menu",
        "n'importe quel pad, Espace ou OPTIONS : continuer · Tab / CREATE : écran suivant · Échap menu",
    ),
    (
        "Two short tests. First: tap any pad exactly on each click you hear.\nPress a pad to start.",
        "Deux petits tests. D'abord : frappe un pad pile sur chaque clic que tu entends.\nFrappe un pad pour commencer.",
    ),
    (
        "Tap any pad on every click. Listen, don't look.",
        "Frappe un pad sur chaque clic. Écoute, ne regarde pas.",
    ),
    (
        "Now the screen: tap on every flash of the circle, without sound.\nPress a pad to start.",
        "Maintenant l'écran : frappe à chaque fois que le cercle s'allume, sans le son.\nFrappe un pad pour commencer.",
    ),
    (
        "Tap any pad on every flash.",
        "Frappe un pad à chaque fois que le cercle s'allume.",
    ),
    (
        "Done. Press a pad to save these offsets for this audio output.",
        "Terminé. Frappe un pad pour enregistrer ces réglages pour cette sortie audio.",
    ),
    (
        "Not steady enough to save. Press a pad to start again.",
        "Pas assez régulier pour être enregistré. Frappe un pad pour recommencer.",
    ),
    (
        "Saved. Press a pad to run it again.",
        "Enregistré. Frappe un pad pour recommencer.",
    ),
    ("sound", "son"),
    ("screen", "écran"),
    (
        "{}: taps too uneven (spread {} ms, want under {}), try again",
        "{} : frappes trop irrégulières (écart {} ms, il faut moins de {}), recommence",
    ),
    (
        "{}: you tap {} ms after it (spread {} ms, {} taps)",
        "{} : tu frappes {} ms après (écart {} ms, {} frappes)",
    ),
    (
        "{}: you tap {} ms before it (spread {} ms, {} taps)",
        "{} : tu frappes {} ms avant (écart {} ms, {} frappes)",
    ),
    (
        "{}: not enough steady taps, try again",
        "{} : pas assez de frappes régulières, recommence",
    ),
    (
        "saved for \"{}\": sound {} ms, screen {} ms, visuals lead by {} ms",
        "enregistré pour « {} » : son {} ms, écran {} ms, l'image devance de {} ms",
    ),
    // Warnings, imports, the jam and the controller.
    ("no sound: {}", "pas de son : {}"),
    (
        "Bluetooth output: 100 ms+ of latency, use a wired output to play",
        "Sortie Bluetooth : 100 ms de latence ou plus, branche une sortie filaire pour jouer",
    ),
    (
        "{} is a drum stem: drop its tune, and the stem beside it is heard with it",
        "{} est un stem batterie : dépose son morceau, le stem posé à côté sera écouté avec",
    ),
    (
        "Still listening to {}: drop {} again after",
        "J'écoute encore {} : redépose {} après",
    ),
    (
        "Can't import: this system has no data folder to keep tunes in",
        "Import impossible : ce système n'a pas de dossier de données pour garder les morceaux",
    ),
    ("Can't import {}: {}", "Import impossible de {} : {}"),
    (
        "Ready to play: {} · {} BPM · {} bars · {} drop{}",
        "Prêt à jouer : {} · {} BPM · {} mesures · {} drop{}",
    ),
    ("Couldn't import {}: {}", "Échec de l'import de {} : {}"),
    ("Can't play {}: {}", "Lecture impossible de {} : {}"),
    ("{}: {}…", "{} : {}…"),
    ("{}: {}… {} %", "{} : {}… {} %"),
    ("listening", "écoute"),
    ("finding the beat", "recherche du tempo"),
    ("hearing the drums", "écoute de la batterie"),
    ("hearing the bass line", "écoute de la ligne de basse"),
    ("finding the drops", "recherche des drops"),
    ("measuring the level", "mesure du volume"),
    ("can't open it: {}", "impossible de l'ouvrir : {}"),
    (
        "not an audio file this game can read (MP3, WAV, FLAC, OGG or M4A): {}",
        "ce n'est pas un fichier audio que le jeu sait lire (MP3, WAV, FLAC, OGG ou M4A) : {}",
    ),
    ("it has no audio in it", "il n'y a pas de son dedans"),
    (
        "there is no steady beat in it to play along to",
        "pas de rythme assez régulier pour jouer dessus",
    ),
    (
        "it is too short to play: {} bars (a song needs at least {})",
        "trop court pour être joué : {} mesures (il en faut au moins {})",
    ),
    ("can't keep it: {}", "impossible de le garder : {}"),
    ("its song file is damaged: {}", "son fichier de morceau est abîmé : {}"),
    (
        "it was heard by a newer listener than this game's",
        "il a été écouté par une version plus récente du jeu",
    ),
    (
        "{}: listening again with the new listener…",
        "{} : réécoute avec la nouvelle oreille…",
    ),
    ("{}: listening again, {}… {} %", "{} : réécoute, {}… {} %"),
    (
        "{}: heard again, its drums where they sound",
        "{} : réécouté, ses drums là où ils sonnent",
    ),
    ("Couldn't listen again to {}: {}", "Échec de la réécoute de {} : {}"),
    ("Can't read the tune: {}", "Impossible de lire le morceau : {}"),
    (
        "Space / OPTIONS play·stop · R restart · pads: a controller, or ↑ ↓ ← → and I J K L · Esc menu",
        "Espace / OPTIONS lecture·stop · R recommencer · pads : une manette, ou ↑ ↓ ← → et I J K L · Échap menu",
    ),
    ("bar {}   beat {}   {} BPM", "mesure {}   temps {}   {} BPM"),
    (
        "stopped: press Space or OPTIONS",
        "à l'arrêt : appuie sur Espace ou OPTIONS",
    ),
    (
        "layout: {} · L3 (or X) switches · notes follow the reel: the General MIDI drum map",
        "disposition : {} · L3 (ou X) change · les notes suivent la vidéo : la table de batterie General MIDI",
    ),
    (
        "move a stick to measure the controller's report rate and jitter",
        "bouge un stick pour mesurer la cadence et la gigue de la manette",
    ),
    ("no controller backend: {}", "pas de pilote de manette : {}"),
    ("KEYS", "CLAVIER"),
    ("[{}]  {}  ·  {}  ·  {} connected", "[{}]  {}  ·  {}  ·  {} connecté(s)"),
    (
        "No controller found ({}). Keyboard: ↑ ↓ ← → and I J K L, E/O = L1/R1, Z/N = L2/R2",
        "Aucune manette trouvée ({}). Clavier : ↑ ↓ ← → et I J K L, E/O = L1/R1, Z/N = L2/R2",
    ),
    (
        "event interval: median {} ms · 95th percentile {} ms · ≈ {} reports/s ({} samples)",
        "intervalle entre événements : médiane {} ms · 95e centile {} ms · ≈ {} rapports/s ({} mesures)",
    ),
    ("pressed  {}", "appuyé  {}"),
    ("released {}", "relâché {}"),
    ("connected", "connecté"),
    ("disconnected", "déconnecté"),
    ("keys", "clavier"),
    ("Drummer", "Batteur"),
    ("FULL COMBO", "FULL COMBO"),
];

fn french(english: &str) -> Option<&'static str> {
    FRENCH
        .iter()
        .find(|(key, _)| *key == english)
        .map(|(_, french)| *french)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_fill_in_order_and_lines_without_french_stay_english() {
        assert_eq!(fill("{} of {}: {}", &[&1, &8, &"First Steps"]), "1 of 8: First Steps");
        assert_eq!(fill("no values", &[]), "no values");
        assert_eq!(fill("{} and {}", &[&"one"]), "one and ");
        assert_eq!(tr(Language::French, "SONGS"), "MORCEAUX");
        assert_eq!(tr(Language::English, "SONGS"), "SONGS");
        assert_eq!(tr(Language::French, "never translated"), "never translated");
        assert_eq!(decimal(Language::English, 97.345, 1), "97.3");
        assert_eq!(decimal(Language::French, 97.345, 1), "97,3");
    }

    #[test]
    fn keys_read_in_french_with_the_notes_named_and_the_mode_after() {
        assert_eq!(key(Language::French, "F# minor"), "fa dièse mineur");
        assert_eq!(key(Language::French, "D dorian"), "ré dorien");
        assert_eq!(key(Language::French, "Bb major"), "si bémol majeur");
        assert_eq!(key(Language::English, "G# phrygian"), "G# phrygian");
        assert_eq!(key(Language::French, "somewhere odd"), "somewhere odd");
        assert_eq!(key(Language::French, ""), "");
    }

    #[test]
    fn every_french_line_has_as_many_blanks_as_its_english_and_none_twice() {
        for (i, (english, french)) in FRENCH.iter().enumerate() {
            assert_eq!(english.matches("{}").count(), french.matches("{}").count(), "{english}");
            assert!(FRENCH[..i].iter().all(|(other, _)| other != english), "{english} twice");
        }
    }
}
