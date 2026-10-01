//! Just enough music theory to check a song stays in its key.

use crate::notes::parse_pitch;

/// The pitch classes (0 = C) of a key written like "F minor" or "A dorian".
pub fn scale(key: &str) -> Option<Vec<u8>> {
    let (tonic, mode) = key.trim().split_once(' ')?;
    let root = parse_pitch(&format!("{tonic}4"))? % 12;
    let steps: &[u8] = match mode.trim().to_lowercase().as_str() {
        "minor" | "aeolian" => &[0, 2, 3, 5, 7, 8, 10],
        "major" | "ionian" => &[0, 2, 4, 5, 7, 9, 11],
        "dorian" => &[0, 2, 3, 5, 7, 9, 10],
        "phrygian" => &[0, 1, 3, 5, 7, 8, 10],
        _ => return None,
    };
    Some(steps.iter().map(|step| (root + step) % 12).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_give_their_pitch_classes() {
        assert_eq!(scale("F minor"), Some(vec![5, 7, 8, 10, 0, 1, 3]));
        assert_eq!(scale("C major"), Some(vec![0, 2, 4, 5, 7, 9, 11]));
        assert_eq!(scale("A dorian"), Some(vec![9, 11, 0, 2, 4, 6, 7]));
        assert_eq!(scale("F# phrygian").map(|s| s[1]), Some(7));
        assert_eq!(scale("F lydian"), None);
        assert_eq!(scale("nonsense"), None);
    }
}
