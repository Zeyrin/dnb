//! Kits, baked once and kept: in memory for the session, and on disk between
//! launches under their fingerprint, which changes whenever anything that
//! shapes their sound does (see `KitDef::fingerprint`). A stale or damaged file
//! is simply baked again.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use wu_instruments::kits::Baked;
use wu_instruments::{Break, KITS, Kit, KitDef, kit_def};

/// The first bytes of a baked-kit file, and its format's version.
const MAGIC: &[u8; 6] = b"WUKIT\x01";

#[derive(Default)]
struct Cache {
    dir: Option<PathBuf>,
    kits: HashMap<(String, u32), Kit>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

/// Where baked kits are kept between launches. Without one they are baked
/// once per session.
pub fn set_cache_dir(dir: impl Into<PathBuf>) {
    if let Ok(mut cache) = cache().lock() {
        cache.dir = Some(dir.into());
    }
}

/// The game's own cache folder, if the platform has one.
pub fn default_cache_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "wheelup").map(|dirs| dirs.cache_dir().join("kits"))
}

/// The kit called `id`, baked for `sample_rate`: from memory, else from disk,
/// else baked now (and kept). `None` for an unknown kit.
pub fn kit(id: &str, sample_rate: u32) -> Option<Kit> {
    let def = kit_def(id)?;
    let key = (id.to_owned(), sample_rate);
    let dir = {
        let cache = cache().lock().ok()?;
        if let Some(kit) = cache.kits.get(&key) {
            return Some(kit.clone());
        }
        cache.dir.clone()
    };
    // Baking takes a moment: never while holding the lock.
    let kit = load_or_bake(def, sample_rate, dir.as_deref());
    if let Ok(mut cache) = cache().lock() {
        cache.kits.insert(key, kit.clone());
    }
    Some(kit)
}

/// How songs name a break: "Rough Rider" is `rough-rider`.
pub fn break_id(name: &str) -> String {
    name.to_lowercase().replace(' ', "-")
}

/// Whether some kit has the break `id`.
pub fn has_break(id: &str) -> bool {
    KITS.iter().any(|kit| kit.breaks.iter().any(|b| break_id(b.name) == id))
}

/// The break `id`, whole, from the first kit that has it (see `kit`).
pub fn break_loop(id: &str, sample_rate: u32) -> Option<Break> {
    let owner = KITS
        .iter()
        .find(|kit| kit.breaks.iter().any(|b| break_id(b.name) == id))?;
    kit(owner.id, sample_rate)?
        .breaks
        .into_iter()
        .find(|b| break_id(&b.name) == id)
}

/// Bakes (or loads) every kit, so no song waits for one later.
pub fn warm_up(sample_rate: u32) {
    for def in &KITS {
        kit(def.id, sample_rate);
    }
}

fn file_for(def: &KitDef, sample_rate: u32, dir: &Path) -> PathBuf {
    dir.join(format!(
        "{}-{sample_rate}-{:016x}.wukit",
        def.id,
        def.fingerprint(sample_rate)
    ))
}

fn load_or_bake(def: &KitDef, sample_rate: u32, dir: Option<&Path>) -> Kit {
    if let Some(dir) = dir {
        let path = file_for(def, sample_rate, dir);
        if let Some(kit) = read(&path).and_then(|baked| def.assemble(sample_rate, &baked)) {
            return kit;
        }
        if let Ok(baked) = def.bake(sample_rate) {
            // Kits from older builds, or for other rates, are dead weight.
            remove_stale(def, dir, &path);
            if let Err(error) = write(&path, &baked) {
                eprintln!("kit {}: not cached ({error})", def.id);
            }
            if let Some(kit) = def.assemble(sample_rate, &baked) {
                return kit;
            }
        }
    }
    def.kit(sample_rate)
}

fn remove_stale(def: &KitDef, dir: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let prefix = format!("{}-", def.id);
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path != keep && name.starts_with(&prefix) && name.ends_with(".wukit") {
            let _ = fs::remove_file(path);
        }
    }
}

/// Writes every sample, little-endian, through a temporary file so a crash
/// never leaves half a kit behind.
pub(crate) fn write(path: &Path, baked: &Baked) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(baked.len() as u32).to_le_bytes());
    for (name, samples) in baked {
        bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        for x in samples {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
    }
    let temporary = path.with_extension("partial");
    fs::File::create(&temporary)?.write_all(&bytes)?;
    fs::rename(temporary, path)
}

/// Reads a baked kit back; `None` if it is missing, damaged or of another format.
pub(crate) fn read(path: &Path) -> Option<Baked> {
    let mut bytes = Vec::new();
    fs::File::open(path).ok()?.read_to_end(&mut bytes).ok()?;
    let mut file = Cursor(bytes.strip_prefix(MAGIC.as_slice())?);
    let count = file.length()?;
    let mut baked = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        let len = file.length()?;
        let name = String::from_utf8(file.take(len)?.to_vec()).ok()?;
        let frames = file.length()?;
        let (words, _) = file.take(frames.checked_mul(4)?)?.as_chunks::<4>();
        let samples = words.iter().map(|word| f32::from_le_bytes(*word)).collect();
        baked.push((name, samples));
    }
    file.0.is_empty().then_some(baked)
}

/// What's left of a file being read.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, tail) = self.0.split_at_checked(n)?;
        self.0 = tail;
        Some(head)
    }

    fn length(&mut self) -> Option<usize> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?) as usize)
    }
}

#[cfg(test)]
mod tests {
    use wu_instruments::kits::{DARKSIDE_92, MINIMAL_ROLLER};

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wheelup-kits-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_baked_kit_survives_the_disk_bit_for_bit() {
        let dir = scratch("round-trip");
        let baked = MINIMAL_ROLLER.bake(48_000).expect("bakes");
        let path = dir.join("roller.wukit");
        write(&path, &baked).expect("writes");
        assert_eq!(read(&path), Some(baked));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_file_is_baked_again() {
        let dir = scratch("damaged");
        let path = file_for(&DARKSIDE_92, 48_000, &dir);
        fs::create_dir_all(&dir).expect("dir");
        fs::write(&path, b"WUKIT\x01 not a kit").expect("writes");
        assert_eq!(read(&path), None);
        let kit = load_or_bake(&DARKSIDE_92, 48_000, Some(&dir));
        assert_eq!(kit.name, "Darkside '92");
        assert!(read(&path).is_some(), "rewritten whole");
        // Loading it again gives the same sounds as baking from scratch.
        let again = load_or_bake(&DARKSIDE_92, 48_000, Some(&dir));
        let fresh = DARKSIDE_92.kit(48_000);
        for (a, b) in again.pads.iter().zip(&fresh.pads) {
            assert_eq!(a.sample.data(), b.sample.data(), "{}", a.name);
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn kits_and_breaks_are_found_by_id() {
        assert!(kit("ragga-93", 48_000).is_some());
        assert!(kit("kazoo", 48_000).is_none());
        assert_eq!(break_id("Sunday Service"), "sunday-service");
        assert!(has_break("half-step") && !has_break("amen"));
        let half = break_loop("half-step", 48_000).expect("Halftime Heavy has it");
        assert_eq!((half.name.as_str(), half.bars), ("Half Step", 2));
    }
}
