use crate::{
    error::{AppError, Result},
    persistence,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Light,
    Dark,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub language: String,
    pub theme: Theme,
    pub reduced_motion: bool,
    pub check_updates_on_startup: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: "en".into(),
            theme: Theme::Light,
            reduced_motion: false,
            check_updates_on_startup: true,
        }
    }
}
impl Preferences {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = persistence::read_bounded(path, 16 * 1024)?;
        serde_json::from_slice(&bytes).map_err(|e| AppError::detail("preferences_error", e))
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::detail("preferences_error", e))?;
        }
        persistence::atomic_write(
            path,
            &serde_json::to_vec_pretty(self)
                .map_err(|e| AppError::detail("preferences_error", e))?,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_keep_startup_opt_out_and_locale_persistent() {
        let p = Preferences {
            language: "fr".into(),
            theme: Theme::Dark,
            reduced_motion: true,
            check_updates_on_startup: false,
        };
        let bytes = serde_json::to_vec(&p).unwrap();
        let q: Preferences = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(p, q);
        assert!(Preferences::default().check_updates_on_startup);
    }
}
