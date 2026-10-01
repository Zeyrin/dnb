//! Every bundled song is mastered to the same loudness, with headroom left for
//! the converter: −16 LUFS ± 1 LU integrated, true peak at most −1 dBTP.

use wu_content::mastering::{MAX_TRUE_PEAK_DB, TARGET_LUFS, TOLERANCE_LU, measure};
use wu_content::songs::BUILTIN;

#[test]
fn every_song_ships_at_the_target_loudness() {
    for builtin in BUILTIN {
        let song = builtin.load().expect("compiles");
        let loudness = measure(&song, 48_000).expect("not silent");
        assert!(
            loudness.excess().abs() <= TOLERANCE_LU,
            "{}: {:.1} LUFS, the target is {TARGET_LUFS} ± {TOLERANCE_LU}",
            builtin.id,
            loudness.integrated
        );
        assert!(
            loudness.true_peak_db <= MAX_TRUE_PEAK_DB,
            "{}: true peak {:.2} dBTP",
            builtin.id,
            loudness.true_peak_db
        );
    }
}
