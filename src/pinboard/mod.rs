#![allow(dead_code)]
use std::io::prelude::*;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::env;
use std::fs::File;

use serde_json;
use serde::{Serialize, Deserialize};
use rmps;

use chrono::prelude::*;

use regex::Regex;

mod api;
pub mod pin;

pub use self::pin::{Pin, Tag};

#[derive(Debug)]
pub struct Config {
    pub cache_dir: PathBuf,
    pub tag_only_search: bool,
    pub fuzzy_search: bool,
    tags_cache_file: PathBuf,
    pins_cache_file: PathBuf,
}

impl Config {
    pub fn new() -> Result<Self, String> {

        fn get_app_dir() -> PathBuf {
            let mut dir = env::home_dir().unwrap_or_else(|| PathBuf::from(""));
            dir.push(".cache");
            dir.push("rusty-pin");
            dir
        }

        let cache_dir = get_app_dir();
        Config::create_cache_dir(cache_dir).and_then(|cache_dir| {
            Ok(Config {
                tag_only_search: false,
                fuzzy_search: false,
                tags_cache_file: cache_dir.join("tags.cache"),
                pins_cache_file: cache_dir.join("pins.cache"),
                cache_dir,
            })
        })
    }

    pub fn set_cache_dir<P: AsRef<Path>>(&mut self, p: &P) -> Result<(), String> {
        self.cache_dir = Config::create_cache_dir(p)?;
        self.tags_cache_file = self.cache_dir.join("tags.cache");
        self.pins_cache_file = self.cache_dir.join("pins.cache");
        Ok(())
    }

    pub fn enable_tag_only_search(&mut self, v: bool) {
        self.tag_only_search = v;
    }

    pub fn enable_fuzzy_search(&mut self, v: bool) {
        self.fuzzy_search = v;
    }

    fn create_cache_dir<P: AsRef<Path>>(cache_dir: P) -> Result<PathBuf, String> {
        use std::fs;
        fs::create_dir_all(&cache_dir)
            .map_err(|e| e.description().to_owned())
            .and_then(|_| Ok(cache_dir.as_ref().to_path_buf()))
    }
}

#[derive(Debug)]
pub struct Pinboard {
    api: api::Api,
    cfg: Config,
    cached_pins: Option<Vec<Pin>>,
    cached_tags: Option<Vec<Tag>>,
}

impl Pinboard {
    pub fn new(auth_token: String) -> Result<Self, String> {
        let cfg = Config::new()?;
        Ok(Pinboard {
            api: api::Api::new(auth_token),
            cfg,
            cached_pins: None,
            cached_tags: None,
        })
    }

    pub fn add(self, p: Pin) -> Result<(), String> {
        self.api.add_url(p)
    }

    pub fn search_items(&mut self, q: &str) -> Result<Option<Vec<&Pin>>, String> {
        if self.cfg.pins_cache_file.exists() {

            self.get_cached_pins()?;

            if self.cached_pins.is_none() {
                return Ok(None)
            }

            let r = if !self.cfg.fuzzy_search {
                let q = &q.to_lowercase();
                self.cached_pins.as_ref().unwrap()
                    .into_iter()
                    .filter(|item| item.contains(q))
                    .collect::<Vec<&Pin>>()
            } else {
                // Build a string for regex: "HAMID" => "H.*A.*M.*I.*D"
                let mut fuzzy_string = q.chars()
                    .map(|c| format!("{}", c))
                    .collect::<Vec<String>>()
                    .join(r".*");
                // Set case-insensitive regex option.
                fuzzy_string.insert_str(0, "(?i)");
                let re = Regex::new(&fuzzy_string).map_err(|_| {
                    "Can't search for given query!".to_owned()
                })?;
                self.cached_pins.as_ref().unwrap()
                    .into_iter()
                    .filter(|item| item.contains_fuzzy(&re))
                    .collect::<Vec<&Pin>>()
            };
            match r.len() {
                0 => Ok(None),
                _ => Ok(Some(r)),
            }
        } else {
            Err(format!(
                "pins cache file not present: {}",
                self.cfg.pins_cache_file.to_str().unwrap_or("")
            ))
        }
    }

    pub fn search_tags(&mut self, q: &str) -> Result<Option<Vec<&Tag>>, String> {
        if self.cfg.tags_cache_file.exists() {

            self.get_cached_tags()?;
            if self.cached_tags.is_none() {
                return Ok(None)
            }

            let r = if !self.cfg.fuzzy_search {
                let q = &q.to_lowercase();
                self.cached_tags.as_ref().unwrap()
                    .into_iter()
                    .filter(|item| item.0.to_lowercase().contains(q))
                    .collect::<Vec<&Tag>>()
            } else {
                // Build a string for regex: "HAMID" => "H.*A.*M.*I.*D"
                let mut fuzzy_string = q.chars()
                    .map(|c| format!("{}", c))
                    .collect::<Vec<String>>()
                    .join(r".*");
                // Set case-insensitive regex option.
                fuzzy_string.insert_str(0, "(?i)");
                let re = Regex::new(&fuzzy_string).map_err(|_| {
                    "Can't search for given query!".to_owned()
                })?;
                self.cached_tags.as_ref().unwrap()
                    .into_iter()
                    .filter(|item| re.captures(&item.0).is_some())
                    .collect::<Vec<&Tag>>()
            };
            match r.len() {
                0 => Ok(None),
                _ => Ok(Some(r)),
            }
        } else {
            Err(format!(
                "tags cache file not present: {}",
                self.cfg.tags_cache_file.to_str().unwrap_or("")
            ))
        }
    }

    pub fn is_cache_outdated(&self, last_update: DateTime<Utc>) -> Result<bool, String> {
        self.api.recent_update().and_then(
            |res| Ok(last_update < res),
        )
    }

    fn update_cache(&self) -> Result<(), String> {
        //TODO: cache all searchable text in lowercase format to make "pin.contains()" efficient.
        // Write all pins
        let mut f = File::create(&self.cfg.pins_cache_file).map_err(|e| {
            e.description().to_owned()
        })?;
        self.api
            .all_pins()
            .and_then(|pins: Vec<Pin>| {
                let mut buf: Vec<u8> = Vec::new();
                pins.serialize(&mut rmps::Serializer::new(&mut buf))
                    .map_err(|e| e.description().to_owned())?;
                Ok(buf)
            })
            .and_then(|data| f.write_all(&data).map_err(|e| e.description().to_owned()))?;

        // Write all tags
        let mut f = File::create(&self.cfg.tags_cache_file).map_err(|e| {
            e.description().to_owned()
        })?;
        self.api
            .tags_frequency()
            .and_then(|tags| serde_json::to_vec(&tags).map_err(|e| e.description().to_owned()))
            .and_then(|data| f.write_all(&data).map_err(|e| e.description().to_owned()))
    }
}

/// private implementations
impl Pinboard {
    fn read_file<P: AsRef<Path>>(&self, p: P) -> Result<String, String> {

        File::open(p)
            .map_err(|e| e.description().to_owned())
            .and_then(|mut f| {
                let mut content = String::new();
                f.read_to_string(&mut content)
                    .map_err(|e| e.description().to_owned())
                    .and_then(|_| Ok(content))
            })
    }

    fn get_cached_pins(&mut self) -> Result<(), String> {
        // TODO: Use rmp-serde in cache files
        match self.cached_pins {
            Some(_) => Ok(()),
            None => {
                let cached_pins = self.read_file(&self.cfg.pins_cache_file)?;
                self.cached_pins = serde_json::from_str(&cached_pins).map_err(|e| {
                    e.description().to_owned()
                })?;
                Ok(())
            }
        }
    }

    fn get_cached_tags(&mut self) -> Result<(), String> {
        // TODO: Use rmp-serde in cache files
        match self.cached_tags {
            Some(_) => Ok(()),
            None => {
                let cached_tags = self.read_file(&self.cfg.tags_cache_file)?;
                self.cached_tags = serde_json::from_str(&cached_tags).map_err(|e| {
                    e.description().to_owned()
                })?;
                Ok(())
            }
        }
    }
}


#[cfg(test)]
mod tests {
    // TODO: Add tests for case insensitivity searches of tags/pins
    use super::*;

    #[test]
    fn test_config() {
        let mut h = env::home_dir().unwrap();
        h.push(".cache");
        h.push("rusty-pin");
        let c = Config::new().expect("Can't initiate 'Config'.");
        assert_eq!(c.cache_dir, h);

        h.push("pins");
        h.set_extension("cache");
        assert_eq!(c.pins_cache_file, h);

        h.set_file_name("tags");
        h.set_extension("cache");
        assert_eq!(c.tags_cache_file, h);
    }

    #[test]
    fn test_set_cache_dir() {
        let mut h = env::home_dir().unwrap();
        let mut c = Config::new().expect("Can't initiate 'Config'.");

        h.push(".cache");
        h.push("rustypin");
        c.set_cache_dir(&h).expect("Can't change cache path.");

        h.push("tags.cache");
        assert_eq!(c.tags_cache_file, h);

        h.set_file_name("pins.cache");
        assert_eq!(c.pins_cache_file, h);
    }

    #[test]
    fn test_search_tags() {
        let mut pinboard = Pinboard::new(include_str!("auth_token.txt").to_string()).unwrap();
        pinboard.cfg.enable_fuzzy_search(false);

        {
            let tags = pinboard.search_tags("django").unwrap_or_else(|e| panic!(e));
            assert!(tags.is_some());
        }

        {
            // non-fuzzy search test
            let tags = pinboard.search_tags("non-existence-tag").unwrap_or_else(
                |e| panic!(e),
            );
            assert!(tags.is_none());
        }
        {
            // fuzzy search test
            pinboard.cfg.enable_fuzzy_search(true);
            let tags = pinboard.search_tags("non-existence-tag").unwrap_or_else(
                |e| panic!(e),
            );
            assert!(tags.is_none());
        }

        {
            // non-fuzzy search test
            let tags = pinboard.search_tags("Lumia920").unwrap_or_else(
                |e| panic!(e),
            );
            assert!(tags.is_some());
            let tags = tags.unwrap();
            assert_eq!(tags.len(), 1);
            assert_eq!(tags[0].1, 2);
        }

        {
            // fuzzy search test
            pinboard.cfg.enable_fuzzy_search(true);
            let tags = pinboard.search_tags("Lumia920").unwrap_or_else(
                |e| panic!(e),
            );
            assert!(tags.is_some());
            let tags = tags.unwrap();
            assert_eq!(tags.len(), 1);
            assert_eq!(tags[0].1, 2);
        }

    }

    #[ignore]
    #[test]
    fn test_update_cache() {
        let pinboard = Pinboard::new(include_str!("auth_token.txt").to_string());
        pinboard.unwrap().update_cache().unwrap_or_else(
            |e| panic!(e),
        );
    }
}
