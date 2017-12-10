#![allow(dead_code)]
use std::io::prelude::*;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::env;
use std::fs::File;
use std::borrow::Cow;

use serde::{Serialize, Deserialize};
use rmps::{Serializer, Deserializer};

use chrono::prelude::*;

use regex::Regex;

mod api;
mod config;
pub mod pin;

use self::config::Config;

pub use self::pin::{Pin, Tag};

#[derive(Debug)]
pub struct Pinboard<'a> {
    api: api::Api<'a>,
    cfg: Config,
    cached_pins: Option<Vec<Pin>>,
    cached_tags: Option<Vec<Tag>>,
}

impl<'a> Pinboard<'a> {
    pub fn new<S>(auth_token: S) -> Result<Self, String>
        where S: Into<Cow<'a, str>> {
        let cfg = Config::new()?;
        let mut pinboard = Pinboard {
            api: api::Api::new(auth_token),
            cfg,
            cached_pins: None,
            cached_tags: None,
        };
        pinboard.get_cached_pins()?;
        pinboard.get_cached_tags()?;
        Ok(pinboard)
    }

    pub fn set_cache_dir<P: AsRef<Path>>(&mut self, p: &P) -> Result<(), String> {
        self.cfg.set_cache_dir(p)
    }

    pub fn enable_tag_only_search(&mut self, v: bool) {
        self.cfg.tag_only_search = v;
    }

    pub fn enable_fuzzy_search(&mut self, v: bool) {
        self.cfg.fuzzy_search = v;
    }

    pub fn enable_private_new_pin(&mut self, v: bool) {
        self.cfg.private_new_pin = v;
    }

    pub fn enable_toread_new_pin(&mut self, v: bool) {
        self.cfg.toread_new_pin = v;
    }

    pub fn add(self, p: Pin) -> Result<(), String> {
        self.api.add_url(p)
    }

    pub fn is_cache_outdated(&self, last_update: DateTime<Utc>) -> Result<bool, String> {
        self.api.recent_update().and_then(
            |res| Ok(last_update < res),
        )
    }

    pub fn update_cache(&self) -> Result<(), String> {
        //TODO: cache all searchable text in lowercase format to make "pin.contains()" efficient.
        // Write all pins
        let mut f = File::create(&self.cfg.pins_cache_file).map_err(|e| {
            e.description().to_owned()
        })?;
        self.api
            .all_pins()
            .and_then(|pins: Vec<Pin>| {
                let mut buf: Vec<u8> = Vec::new();
                pins.serialize(&mut Serializer::new(&mut buf))
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
            .and_then(|tags_tuple| {
                let mut buf: Vec<u8> = Vec::new();
                tags_tuple.serialize(&mut Serializer::new(&mut buf))
                    .map_err(|e| e.description().to_owned())?;
                Ok(buf)
            })
            .and_then(|data| f.write_all(&data).map_err(|e| e.description().to_owned()))
    }
}

// Search functions
impl<'a> Pinboard<'a> {
    /// Searches all the fields within bookmarks to filter them.
    /// This function honors [pinboard::config::Config] settings for fuzzy search.
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

    /// Only looks up q within the `tag` field of each bookmark.
    /// This function honors [pinboard::config::Config] settings for fuzzy search.
    pub fn search_tag_field(&mut self, q: &str) -> Result<Option<Vec<&Tag>>, String> {
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

    /// Returns list of all Tags (tag, frequency)
    pub fn tag_pairs(&mut self) -> &Option<Vec<Tag>> {
        &self.cached_tags
    }

    /// Returns list of all bookmarks
    pub fn bookmarks(&mut self) -> &Option<Vec<Pin>> {
        &self.cached_pins
    }
}

/// private implementations
impl<'a> Pinboard<'a> {
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
        // TODO: if pins_cache_file not present, call update_cache
        match self.cached_pins {
            Some(_) => Ok(()),
            None => {
                let fp = File::open(&self.cfg.pins_cache_file)
                    .map_err(|e| e.description().to_owned())?;
                let mut de = Deserializer::from_read(fp);
                self.cached_pins = Deserialize::deserialize(&mut de)
                    .map_err(|e| e.description().to_owned())?;
                Ok(())
            }
        }
    }

    fn get_cached_tags(&mut self) -> Result<(), String> {
        // TODO: if tags_cache_file not present, call update_cache
        match self.cached_tags {
            Some(_) => Ok(()),
            None => {
                let fp = File::open(&self.cfg.tags_cache_file)
                    .map_err(|e| e.description().to_owned())?;
                let mut de = Deserializer::from_read(fp);
                self.cached_tags = Deserialize::deserialize(&mut de)
                    .map_err(|e| e.description().to_owned())?;

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
        let mut pinboard = Pinboard::new(include_str!("auth_token.txt")).unwrap();
        pinboard.enable_fuzzy_search(false);

        {
            let tags = pinboard.search_tag_field("django").unwrap_or_else(|e| panic!(e));
            assert!(tags.is_some());
        }

        {
            // non-fuzzy search test
            let tags = pinboard.search_tag_field("non-existence-tag").unwrap_or_else(
                |e| panic!(e),
            );
            assert!(tags.is_none());
        }
        {
            // fuzzy search test
            pinboard.enable_fuzzy_search(true);
            let tags = pinboard.search_tag_field("non-existence-tag").unwrap_or_else(
                |e| panic!(e),
            );
            assert!(tags.is_none());
        }

        {
            // non-fuzzy search test
            let tags = pinboard.search_tag_field("Lumia920").unwrap_or_else(
                |e| panic!(e),
            );
            assert!(tags.is_some());
            let tags = tags.unwrap();
            assert_eq!(tags.len(), 1);
            assert_eq!(tags[0].1, 2);
        }

        {
            // fuzzy search test
            pinboard.enable_fuzzy_search(true);
            let tags = pinboard.search_tag_field("Lumia920").unwrap_or_else(
                |e| panic!(e),
            );
            assert!(tags.is_some());
            let tags = tags.unwrap();
            assert_eq!(tags.len(), 1);
            assert_eq!(tags[0].1, 2);
        }

    }

    #[test]
    fn list_tags() {
        let pinboard = Pinboard::new(include_str!("auth_token.txt"));
        println!("{:?}", pinboard);
        assert!(pinboard.unwrap().tag_pairs().is_some());
    }

    #[test]
    fn list_bookmarks() {
        let pinboard = Pinboard::new(include_str!("auth_token.txt"));
        assert!(pinboard.unwrap().bookmarks().is_some());
    }


    #[ignore]
    #[test]
    fn test_update_cache() {
        let pinboard = Pinboard::new(include_str!("auth_token.txt"));
        pinboard.unwrap().update_cache().unwrap_or_else(
            |e| panic!(e),
        );
    }
}
