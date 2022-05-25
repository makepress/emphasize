use std::{
    env::{self, VarError},
    fs::File,
    path::{Path, PathBuf},
    str::FromStr,
};

use eyre::eyre;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct Config {
    cache_dir: PathBuf,
    db: PathBuf,
    debug: bool,
    content_dir: PathBuf,
    operating_mode: OperatingMode,
}

pub fn none<T>() -> Option<T> {
    None
}

#[derive(Debug, Deserialize, Default)]
pub struct ConfigBuilder {
    #[serde(default = "none")]
    cache_dir: Option<PathBuf>,
    #[serde(default = "none")]
    db: Option<PathBuf>,
    #[serde(default = "none")]
    debug: Option<bool>,
    #[serde(default = "none")]
    content_dir: Option<PathBuf>,
    #[serde(default = "none")]
    operating_mode: Option<OperatingMode>,
}

impl ConfigBuilder {
    /// A empty config
    pub fn new() -> Self {
        Default::default()
    }

    /// Grab settings from environment variables, preferring them over the originals.
    pub fn with_envs(self) -> crate::Result<Self> {
        let cache_dir = match env::var("CACHE_DIR") {
            Ok(s) => Ok(Some(PathBuf::from_str(&s)?)),
            Err(VarError::NotPresent) => Ok(None),
            Err(e) => Err(e),
        }?;
        let db = match env::var("DB") {
            Ok(s) => Ok(Some(PathBuf::from_str(&s)?)),
            Err(VarError::NotPresent) => Ok(None),
            Err(e) => Err(e),
        }?;
        let debug = match env::var("DEBUG") {
            Ok(s) => {
                let parsed: usize = s.parse()?;
                Ok(Some(parsed == 1))
            }
            Err(VarError::NotPresent) => Ok(None),
            Err(e) => Err(e),
        }?;
        let content_dir = match env::var("CONTENT_DIR") {
            Ok(s) => Ok(Some(PathBuf::from_str(&s)?)),
            Err(VarError::NotPresent) => Ok(None),
            Err(e) => Err(e),
        }?;
        let operating_mode = match env::var("OPERATING_MODE") {
            Ok(s) => Ok(Some(s.parse()?)),
            Err(VarError::NotPresent) => Ok(None),
            Err(e) => Err(e),
        }?;
        let new = Self {
            cache_dir,
            db,
            debug,
            content_dir,
            operating_mode,
        };
        Ok(self.or(new))
    }

    /// Grab settings from a config file, preferring values from that over originals.
    pub fn with_file<P: AsRef<Path>>(self, file: P) -> crate::Result<Self> {
        let new = serde_yaml::from_reader(File::open(file)?)?;
        Ok(self.or(new))
    }

    /// Merge two configs, using the settings from `other` if set otherwise `self`.
    fn or(self, other: Self) -> Self {
        Self {
            cache_dir: other.cache_dir.or(self.cache_dir),
            db: other.db.or(self.db),
            debug: other.debug.or(self.debug),
            content_dir: other.content_dir.or(self.content_dir),
            operating_mode: other.operating_mode.or(self.operating_mode),
        }
    }

    /// Create a config from this builder, replacing missing values with defaults
    pub fn build_with_defaults(self) -> Config {
        let default_config = Config::default();
        Config {
            cache_dir: self.cache_dir.unwrap_or(default_config.cache_dir),
            db: self.db.unwrap_or(default_config.db),
            debug: self.debug.unwrap_or(false),
            content_dir: self.content_dir.unwrap_or(default_config.content_dir),
            operating_mode: self.operating_mode.unwrap_or(default_config.operating_mode),
        }
    }
}

impl Config {
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn db(&self) -> &Path {
        &self.db
    }

    pub fn debug(&self) -> bool {
        self.debug
    }

    pub fn content_dir(&self) -> &Path {
        &self.content_dir
    }

    pub fn operating_mode(&self) -> OperatingMode {
        self.operating_mode
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::from_str(".emphasize/cache").unwrap(),
            db: PathBuf::from_str(".emphasize/content.db").unwrap(),
            debug: false,
            content_dir: PathBuf::from_str("blog").unwrap(),
            operating_mode: Default::default(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, Clone, Copy)]
pub enum OperatingMode {
    ReadOnly,
    ReadWrite,
}

impl Default for OperatingMode {
    fn default() -> Self {
        OperatingMode::ReadWrite
    }
}

impl FromStr for OperatingMode {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Read" => Ok(OperatingMode::ReadOnly),
            "ReadWrite" => Ok(OperatingMode::ReadWrite),
            _ => Err(eyre!(format!(
                "{} is not valid option (Read or ReadWrite)",
                s
            ))),
        }
    }
}
