extern crate chrono;
extern crate url;

#[macro_use]
extern crate serde_derive;

extern crate serde;
extern crate serde_json;
extern crate url_serde;

use url::Url;
use chrono::prelude::*;

#[derive(Serialize, Deserialize, Debug)]
pub struct Pin {
    #[serde(with = "url_serde", rename = "href")]
    pub url: Url,
    #[serde(rename = "description")]
    pub title: String,
    pub tags: String,
    pub shared: String,
    pub toread: String,
    pub extended: Option<String>,
    #[serde(default = "Utc::now")]
    time: DateTime<Utc>,
    meta: Option<String>,
    hash: Option<String>,
    #[serde(skip)]
    tag_list: Vec<String>,
}

impl Pin {
    pub fn new(
        url: Url,
        title: String,
        tags: Vec<String>,
        private: bool,
        read: bool,
        desc: Option<String>,
    ) -> Pin {
        let shared;
        let toread;
        if private {
            shared = "no";
        } else {
            shared = "yes";
        }
        if read {
            toread = "yes";
        } else {
            toread = "no";
        }
        Pin {
            url,
            title,
            tags: String::new(),
            shared: shared.to_string(),
            toread: toread.to_string(),
            extended: desc,
            time: Utc::now(),
            meta: None,
            hash: None,
            tag_list: tags,
        }
    }

    pub fn contains(&self, q: &str) -> bool {
        self.url.as_ref().contains(q) || self.title.contains(q) || self.tags.contains(q)
    }

    pub fn set_tags_str(&mut self, tags: &[&str]) -> () {
        self.tag_list = tags.iter().map(|s| s.to_string()).collect();
    }

    pub fn set_tags(&mut self, tags: Vec<String>) -> () {
        self.tag_list = tags;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_tags() {
        let url = Url::parse("https://githuуй.com/Здравствуйт?q=13#fragment").unwrap();
        let mut p = Pin::new(url, "title".to_string(), vec![], true, false, None);

        let tags = vec!["tag1", "tag2"];
        p.set_tags_str(&tags);
        assert_eq!(p.tag_list, tags);

        let tags = vec![String::from("tag3"), "tag4".to_string()];
        p.set_tags_str(
            tags.iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>()
                .as_slice(),
        );
        assert_eq!(p.tag_list, tags);

        let tags = vec![String::from("tag5"), "tag6".to_string()];
        p.set_tags(tags.clone());
        assert_eq!(p.tag_list, tags);
    }

    #[test]
    fn deserialize_a_pin() {
        let pin: Result<Pin, _> = serde_json::from_str(include_str!("../tests/PIN1.json"));
        assert!(pin.is_ok());
        let pin: Pin = pin.unwrap();
        println!("{:?}", pin);
        assert_eq!(
            pin.url,
            Url::parse("https://danielkeep.github.io/tlborm/book/README.html").unwrap()
        );
        assert_eq!(pin.title, "The Little Book of Rust Macros");
        assert_eq!(pin.time, Utc.ymd(2017, 5, 22).and_hms(17, 46, 54));
        assert_eq!(pin.tags, "Rust macros");

        let pin: Result<Pin, _> = serde_json::from_str(include_str!("../tests/PIN2.json"));
        assert!(pin.is_ok());
        let pin: Pin = pin.unwrap();
        println!("{:?}", pin);
        assert_eq!(
            pin.url,
            Url::parse(
                "http://tbaggery.com/2011/08/08/effortless-ctags-with-git.html",
            ).unwrap()
        );
        assert_eq!(pin.title, "tbaggery - Effortless Ctags with Git");
        assert_eq!(pin.time, Utc.ymd(2017, 10, 9).and_hms(7, 59, 36));
        assert_eq!(pin.tags, "git ctags vim");
    }

    #[test]
    fn deserialize_pins() {
        let input = format!(
            "[{},{}]",
            include_str!("../tests/PIN1.json"),
            include_str!("../tests/PIN2.json")
        );
        let pins: Result<Vec<Pin>, _> = serde_json::from_str(&input);
        if let Err(e) = pins {
            println!("{:?}", e);
            return;
        }
        assert!(pins.is_ok());
        let pins = pins.unwrap();
        assert_eq!(pins.len(), 2);
        println!("{:?}", pins);

        let input = include_str!("../sample.json");
        let pins: Result<Vec<Pin>, _> = serde_json::from_str(input);
        assert!(pins.is_ok());
        let pins = pins.unwrap();
        assert_eq!(pins.len(), 472);
    }

}
