//! Reading and writing the one file that holds every position.
//!
//! ponytail: one JSON file, whole-file rewrite, an fs4 lock around it. Right for a few dozen
//! positions typed by hand. When daily price history arrives this becomes the wrong shape and
//! SQLite is the upgrade; nothing else here changes, because callers only see load and save.

use std::fs::{self, File, OpenOptions};
use std::io::Write;

use fs4::fs_std::FileExt;

use crate::config::Config;
use crate::types::Store;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The file exists but is not a store. The original has been copied to .bak.
    #[error("portfolios.json is damaged and was copied to portfolios.json.bak: {0}")]
    Corrupt(String),
    #[error("store io: {0}")]
    Io(String),
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> StoreError {
        StoreError::Io(e.to_string())
    }
}

/// The lock file guarding both reads and writes of the store directory.
fn lock(cfg: &Config) -> Result<File, StoreError> {
    fs::create_dir_all(&cfg.store_dir)?;
    let f = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(cfg.store_dir.join(".lock"))?;
    FileExt::lock_exclusive(&f)?;
    Ok(f)
}

/// The store on disk, or an empty one when there is no file yet.
///
/// A file that is present but unparseable is NEVER treated as empty: that would hand back an
/// empty store which the next save would then write over the user's positions.
pub fn load(cfg: &Config) -> Result<Store, StoreError> {
    let _guard = lock(cfg)?;
    let path = cfg.portfolios_path();
    let raw = match fs::read(&path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Store::empty(cfg.scope.default_currency()))
        }
        Err(e) => return Err(e.into()),
    };
    match serde_json::from_slice::<Store>(&raw) {
        Ok(s) => Ok(s),
        Err(e) => {
            let bak = path.with_extension("json.bak");
            fs::write(&bak, &raw)?;
            Err(StoreError::Corrupt(e.to_string()))
        }
    }
}

/// Write the whole store. Temp file plus rename, so a crash mid-write leaves the old file intact.
pub fn save(cfg: &Config, s: &Store) -> Result<(), StoreError> {
    let _guard = lock(cfg)?;
    let path = cfg.portfolios_path();
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(s).map_err(|e| StoreError::Io(e.to_string()))?;
    {
        let mut f = File::create(&tmp)?;
        f.write_all(&body)?;
        // ponytail: fsync before rename. Without it a power loss can land the rename before the
        // bytes, which is the one way this design loses positions.
        f.sync_all()?;
    }
    fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Scope;
    use crate::types::{Holding, Portfolio};

    fn cfg() -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Config::new(dir.path().to_path_buf(), Scope::Scandinavia);
        (dir, cfg)
    }

    #[test]
    fn a_missing_file_loads_as_an_empty_store() {
        let (_d, cfg) = cfg();
        let s = load(&cfg).expect("missing file is not an error");
        assert!(s.portfolios.is_empty());
        assert_eq!(s.base_currency, "NOK");
    }

    #[test]
    fn what_is_saved_is_what_loads_back() {
        let (_d, cfg) = cfg();
        let mut s = Store::empty("NOK");
        s.portfolios.push(Portfolio {
            id: "p_1".into(),
            name: "Balanced".into(),
            owner: "Personal".into(),
            band_pct: 3.0,
            holdings: vec![Holding {
                id: "h_1".into(),
                ticker: "EQNR.OL".into(),
                name: "Equinor".into(),
                cls: "Equity, energy".into(),
                shares: 142.5,
                cost_basis: 38210.0,
                cost_currency: "NOK".into(),
                target_pct: 12.0,
            }],
        });
        save(&cfg, &s).expect("save");
        assert_eq!(load(&cfg).expect("load"), s);
    }

    #[test]
    fn a_corrupt_file_is_backed_up_and_reported_never_silently_replaced() {
        let (_d, cfg) = cfg();
        std::fs::create_dir_all(&cfg.store_dir).expect("mkdir");
        std::fs::write(cfg.portfolios_path(), b"{ this is not json").expect("write");
        match load(&cfg) {
            Err(StoreError::Corrupt(_)) => {}
            other => panic!("expected Corrupt, got {other:?}"),
        }
        let bak = cfg.portfolios_path().with_extension("json.bak");
        assert!(bak.exists(), "the damaged file must be kept");
        assert_eq!(std::fs::read(bak).expect("read bak"), b"{ this is not json");
    }

    #[test]
    fn a_save_leaves_no_partial_file_behind() {
        let (_d, cfg) = cfg();
        save(&cfg, &Store::empty("NOK")).expect("save");
        let strays: Vec<_> = std::fs::read_dir(&cfg.store_dir)
            .expect("readdir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(strays.is_empty(), "temp files left behind: {strays:?}");
    }

    #[test]
    fn a_portfolios_json_written_before_aliases_existed_still_loads() {
        // A store that fails to parse is treated as corrupt and moved aside, so adding a field
        // without a default would look to the user like losing every position they had.
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = Config::new(dir.path().to_path_buf(), crate::config::Scope::Scandinavia);
        std::fs::create_dir_all(&cfg.store_dir).expect("mkdir");
        std::fs::write(
            cfg.portfolios_path(),
            br#"{"version":1,"base_currency":"NOK","portfolios":[]}"#,
        )
        .expect("write");
        let s = load(&cfg).expect("loads");
        assert_eq!(s.base_currency, "NOK");
        assert!(s.aliases.is_empty());
    }
}
