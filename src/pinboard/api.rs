use std::borrow::Cow;

use reqwest;
use serde_json;

use chrono::prelude::*;
use url::Url;

use env_logger;

use std::collections::HashMap;
use std::io::Read;

use failure::{err_msg, Error};

use super::pin::Pin;
use super::tag::Tag;

#[cfg(not(test))]
const BASE_URL: &str = "https://api.pinboard.in/v1";

#[cfg(test)]
use mockito;
#[cfg(test)]
#[allow(deprecated)]
const BASE_URL: &str = mockito::SERVER_URL;

/// Struct to hold stringify results Pinboard API returns.
/// Sometimes it returns a json key of "result_code" & sometimes just "result"!!!
#[derive(Serialize, Deserialize, Debug)]
struct ApiResult {
    #[serde(default)]
    result_code: String,
    #[serde(default)]
    result: String,
}

impl ApiResult {
    fn ok(self) -> Result<(), Error> {
        if self.result_code == "done" || self.result == "done" {
            Ok(())
        } else if self.result_code != "" {
            bail!(self.result_code)
        } else {
            bail!(self.result)
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct UpdateTime {
    #[serde(rename = "update_time")]
    datetime: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Api<'api> {
    auth_token: Cow<'api, str>,
}

#[derive(Debug, Fail)]
pub enum ApiError {
    #[fail(display = "invalid url: {}", _0)]
    UrlError(String),
    #[fail(display = "invalid server response: {}", _0)]
    UnrecognizedResponse(String),
    #[fail(display = "Server couldn't fulfill request: {}", _0)]
    ServerError(String),
    #[fail(display = "network error: {}", _0)]
    Network(String),
    #[fail(display = "serde error: {}", _0)]
    SerdeError(String),
}

impl<'api, 'pin> Api<'api> {
    pub fn new<S>(auth_token: S) -> Self
    where
        S: Into<Cow<'api, str>>,
    {
        let _ = env_logger::try_init();
        Api {
            auth_token: auth_token.into(),
        }
    }

    pub fn all_pins(&self) -> Result<Vec<Pin<'pin>>, Error> {
        debug!("all_pins: starting.");
        let res =
            self.get_api_response([BASE_URL, "/posts/all"].concat().as_str(), HashMap::new())?;
        debug!("  received all bookmarks");

        let mut v: serde_json::Value = serde_json::from_str(res.as_str())?;
        let v = v.as_array_mut().ok_or_else(|| {
            ApiError::UnrecognizedResponse("array of bookmarks expected from server".to_string())
        })?;

        let v_len = v.len();

        let pins: Vec<Pin> = v
            .drain(..)
            .filter_map(|line| serde_json::from_value(line).ok())
            .filter(|p: &Pin| Url::parse(&p.url).is_ok())
            .collect();
        if pins.len() != v_len {
            info!(
                "couldn't parse {} bookmarks (out of {})",
                v_len - pins.len(),
                v_len
            );
        } else {
            info!("parsed all bookmarks. total: {}", pins.len());
        }

        Ok(pins)
    }

    pub fn suggest_tags<T: AsRef<str>>(&self, url: T) -> Result<Vec<String>, Error> {
        debug!("suggest_tags: starting.");
        let mut query = HashMap::new();
        query.insert("url", url.as_ref());

        Ok(self
            .get_api_response([BASE_URL, "/posts/suggest"].concat().as_str(), query)
            .and_then(|res| {
                serde_json::from_str::<Vec<serde_json::Value>>(&res)
                    .map_err(|e| ApiError::SerdeError(e.to_string()).into())
            })?
            .into_iter()
            .find(|item| !item["popular"].is_null())
            .map(|item| {
                item["popular"]
                    .as_array()
                    .unwrap_or(&vec![json!([])])
                    .iter()
                    .map(|v| v.as_str().unwrap_or("").to_string())
                    .collect::<Vec<String>>()
            })
            .ok_or_else(|| {
                ApiError::UnrecognizedResponse(
                    "Unrecognized response from API: posts/suggest".to_string(),
                )
            })?)
    }

    pub fn add_url(&self, p: Pin) -> Result<(), Error> {
        debug!("add_url: starting.");
        let url: &str = &p.url;
        let extended = &p.extended.unwrap_or_default();
        let mut map = HashMap::new();
        debug!(" url: {}", url);

        map.insert("url", url);
        map.insert("description", &p.title);
        map.insert("tags", &p.tags);
        map.insert("toread", &p.toread);
        map.insert("extended", extended);
        map.insert("shared", &p.shared);
        map.insert("replace", "yes");

        debug!("Sending payload to: {}/posts/add\n\t{:?}", BASE_URL, map);
        self.get_api_response([BASE_URL, "/posts/add"].concat().as_str(), map)
            .and_then(|res| {
                serde_json::from_str::<ApiResult>(&res)
                    .map_err(|e| From::from(ApiError::UnrecognizedResponse(e.to_string())))
            })
            .and_then(self::ApiResult::ok)
    }

    pub fn tag_rename<T: AsRef<str>>(&self, old: T, new: T) -> Result<(), Error> {
        debug!("tag_rename: starting.");
        let mut map = HashMap::new();
        map.insert("old", old.as_ref());
        map.insert("new", new.as_ref());
        self.get_api_response([BASE_URL, "/tags/rename"].concat(), map)
            .and_then(|res| {
                serde_json::from_str::<ApiResult>(&res)
                    .map_err(|e| From::from(ApiError::UnrecognizedResponse(e.to_string())))
            })
            .and_then(self::ApiResult::ok)
    }

    pub fn tag_delete<T: AsRef<str>>(&self, tag: T) -> Result<(), Error> {
        debug!("tag_rename: starting.");
        let mut map = HashMap::new();
        map.insert("tag", tag.as_ref());
        self.get_api_response([BASE_URL, "/tags/delete"].concat(), map)
            .and_then(|res| {
                serde_json::from_str::<ApiResult>(&res)
                    .map_err(|e| From::from(ApiError::UnrecognizedResponse(e.to_string())))
            })
            .and_then(self::ApiResult::ok)
    }

    /// Gets all tags with their usage frequency.
    pub fn tags_frequency(&self) -> Result<Vec<Tag>, Error> {
        // Pinboard API returns jsonn array when user has no tags, otherwise it returns an
        // object/map of tag:frequency!
        debug!("tags_frequency: starting.");
        let res =
            self.get_api_response([BASE_URL, "/tags/get"].concat().as_str(), HashMap::new())?;
        let raw_tags = serde_json::from_str::<HashMap<String, usize>>(&res);
        match raw_tags {
            Ok(res) => Ok(res
                .into_iter()
                .map(|(k, freq)| {
                    Tag::new(k, freq)
                })
                .collect()),
            Err(_) => {
                debug!("  trying to decode non-object empty tag list");
                let raw_tags = serde_json::from_str::<Vec<HashMap<String, String>>>(&res)?;
                assert!(raw_tags.is_empty());
                Ok(vec![])
            }
        }
    }

    pub fn delete<T: AsRef<str>>(&self, url: T) -> Result<(), Error> {
        debug!("delete: starting.");
        let mut map = HashMap::new();
        debug!(" url: {}", url.as_ref());
        map.insert("url", url.as_ref());

        self.get_api_response([BASE_URL, "/posts/delete"].concat().as_str(), map)
            .and_then(|res| {
                serde_json::from_str(&res)
                    .map_err(|e| From::from(ApiError::UnrecognizedResponse(e.to_string())))
            })
            .and_then(self::ApiResult::ok)
    }

    pub fn recent_update(&self) -> Result<DateTime<Utc>, Error> {
        debug!("recent_update: starting.");
        self.get_api_response(
            [BASE_URL, "/posts/update"].concat().as_str(),
            HashMap::new(),
        )
        .and_then(|res| {
            serde_json::from_str(&res).map_err(|e| From::from(ApiError::SerdeError(e.to_string())))
        })
        .and_then(|date: UpdateTime| Ok(date.datetime))
    }

    fn add_auth_token<T: AsRef<str>>(&self, url: T) -> Url {
        debug!("add_auth_token: starting.");
        // debug!("  token: `{}`", &self.auth_token);
        Url::parse_with_params(
            url.as_ref(),
            &[("format", "json"), ("auth_token", &self.auth_token)],
        )
        .expect("invalid parameters")
    }

    fn get_api_response<T: AsRef<str>>(
        &self,
        endpoint: T,
        params: HashMap<&str, &str>,
    ) -> Result<String, Error> {
        debug!("get_api_response: starting.");

        let endpoint_string = endpoint.as_ref().to_string();
        let mut base_url = Url::parse(endpoint.as_ref()).map_err(|_| {
            let api_err: Error = ApiError::UrlError(endpoint_string).into();
            api_err
        })?;
        // let mut base_url = endpoint.into_url().map_err(|_| {
        //     let api_err: Error = ApiError::UrlError(endpoint_string).into();
        //     api_err
        // })?;
        debug!("  url: {:?}", base_url);

        for (k, v) in params {
            base_url.query_pairs_mut().append_pair(k, v);
        }
        let api_url = self.add_auth_token(base_url);

        let client = reqwest::Client::new();
        let r = client.get(api_url).send();

        let mut resp = r.map_err(|e| {
            use std::io;
            let io_fail = e.get_ref().and_then(|k| k.downcast_ref::<io::Error>());
            if let Some(f) = io_fail {
                let m: String = f.to_string();
                debug!(" ERR: {:#?}", m);
                err_msg(m)
            } else {
                ApiError::Network(format!("Network request error: {:?}", e.to_string())).into()
            }
        })?;
        debug!(" resp is ok (no error)");

        if resp.status().is_success() {
            let mut content = String::with_capacity(2 * 1024);
            let _bytes_read = resp.read_to_string(&mut content)?;
            debug!(" string from resp ok");
            debug!("   {:?}", content.chars().take(10).collect::<Vec<char>>());
            Ok(content)
        } else {
            debug!("  response status indicates error");
            debug!("    {:?}", resp.status().as_str());
            debug!("    {:?}", resp.status().canonical_reason(),);
            let e = ApiError::ServerError(
                resp.status()
                    .canonical_reason()
                    .expect("UNKNOWN RESPONSE")
                    .to_string(),
            )
            .into();
            debug!("    ERR: {:?}", e);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::pinboard::mockito_helper::start_mockito_server;
    use crate::pinboard::mockito_helper::MockBodyGenerate;
    use crate::pinboard::pin::PinBuilder;

    const TEST_URL: &str = "https://githuуй.com/Здравствуйт?q=13#fragment";
    #[test]
    fn get_latest_update_time() {
        let _ = env_logger::try_init();
        debug!("get_latest_update_time: starting.");
        let _m = start_mockito_server(
            r"^/posts/update.*$",
            200,
            r#"{"update_time":"2018-02-07T01:54:09Z"}"#,
        );
        let api = Api::new(include_str!("api_token.txt"));
        let r = api.recent_update();
        assert!(r.is_ok());
    }

    #[test]
    fn too_many_requests() {
        let _m1 = start_mockito_server(r"^/posts/delete.*$", 429, r#"Back off"#);
        let api = Api::new(include_str!("api_token.txt"));
        let r = api.delete(TEST_URL);
        assert_eq!(
            "Server couldn't fulfill request: Too Many Requests",
            r.expect_err("Expected Not Found")
                .find_root_cause()
                .to_string()
        );
    }

    #[test]
    fn delete_tag_test() {
        let _ = env_logger::try_init();
        debug!("delete_tag_test: starting.");
        let _m1 = start_mockito_server(r#"^/tags/delete.*$"#, 200, r#"{"result":"done"}"#);
        let api = Api::new(include_str!("api_token.txt"));
        let r = api.tag_delete("DUMMY");
        r.expect("Error in deleting a tag.");

        {
            // Deleting non-existing tag
            // Pinboard returns OK on this operation!!!
            let _m2 = start_mockito_server(
                r"^/tags/delete.+fucking\.way.*$",
                200,
                r#"{"result":"done"}"#,
            );
            let _ = api
                .tag_delete("http://no.fucking.way")
                .expect("pinboard OKs deleting a non-existing tag.");
        }

        {
            // Deleting empty string
            // Pinboard returns OK on this operation!!!
            let _m2 = start_mockito_server(r"^/tags/delete.*$", 200, r#"{"result":"done"}"#);
            let _ = api
                .tag_delete("")
                .expect("pinboard OKs deleting a non-existing tag.");
        }
    }

    #[test]
    fn rename_tag_test() {
        let _ = env_logger::try_init();
        debug!("rename_tag_test: starting");
        let _m1 = start_mockito_server(r#"^/tags/rename.*$"#, 200, r#"{"result":"done"}"#);
        let api = Api::new(include_str!("api_token.txt"));
        let r = api.tag_rename("old_tag", "new_tag");
        r.expect("Error in renaming a tag.");

        // Pinboard apparently can rename null to a new tag!!!
        let _ = api
            .tag_rename("", "iamjesus")
            .expect("Should be able to breath life into abyss");

        {
            // renaming to an empty tag
            let _m2 =
                start_mockito_server(r#"^/tags/rename.*$"#, 200, r#"{"result":"rename to null"}"#);
            let r = api
                .tag_rename("old_tag", "")
                .expect_err("renaming to empty tag should return error");
            assert_eq!("rename to null".to_string(), r.as_fail().to_string());
        }
    }

    #[test]
    fn delete_api_test() {
        let _ = env_logger::try_init();
        debug!("delete_a_pin: starting.");
        add_a_url();
        let _m1 = start_mockito_server(r#"^/posts/delete.*$"#, 200, r#"{"result_code":"done"}"#);
        let api = Api::new(include_str!("api_token.txt"));
        let r = api.delete(TEST_URL);
        r.expect("Error in deleting a pin.");

        {
            // Deleting non-existing bookmark
            let _m2 = start_mockito_server(
                r"^/posts/delete.+fucking\.way.*$",
                200,
                r#"{"result_code":"item not found"}"#,
            );
            let r = api
                .delete("http://no.fucking.way")
                .expect_err("Deleted non-existing pin");
            assert_eq!("item not found".to_string(), r.as_fail().to_string());
        }

        {
            // Deleting malformed url
            let _m2 = start_mockito_server(
                r"^/posts/delete.*$",
                200,
                r#"{"result_code":"item not found"}"#,
            );
            let r = api
                .delete(":// bad url/#")
                .expect_err("should not find a malformed url to delete");
            assert_eq!("item not found".to_string(), r.as_fail().to_string());
        }
    }

    #[test]
    fn add_a_url() {
        let _ = env_logger::try_init();
        debug!("add_a_url: starting.");
        let _m1 = start_mockito_server(r"^/posts/add.*$", 200, r#"{"result_code":"done"}"#);
        let api = Api::new(include_str!("api_token.txt"));
        let p = PinBuilder::new(TEST_URL, "test bookmark/pin")
            .tags("tagestan what")
            .description("russian website!")
            .shared("yes")
            .into_pin();
        let res = api.add_url(p);
        res.expect("Error in adding a pin.");

        {
            // Adding a malformed url
            let _m1 = start_mockito_server(
                r"^/posts/add.+bad_url.*$",
                200,
                r#"{"result_code":"missing url"}"#,
            );
            let p = PinBuilder::new(":// bad_url/#", "test bookmark/pin")
                .tags("tagestan what")
                .description("russian website!")
                .shared("yes")
                .into_pin();
            let r = api
                .add_url(p)
                .expect_err("server should not have accepted malformed url");
            assert_eq!("missing url", r.as_fail().to_string());
        }
    }

    #[test]
    fn suggest_tags() {
        let _ = env_logger::try_init();
        debug!("suggest_tags: starting.");
        let _m1 = start_mockito_server(
            r"^/posts/suggest.*$",
            200,
            PathBuf::from("tests/suggested_tags_mockito.json"),
        );
        let api = Api::new(include_str!("api_token.txt"));
        let url = "http://blog.com/";
        let res = api.suggest_tags(url);
        assert_eq!(
            vec!["datetime", "library", "rust"],
            res.expect("impossible")
        );
    }

    #[test]
    fn test_tag_freq() {
        let _ = env_logger::try_init();
        debug!("test_tag_freq: starting.");
        let _m1 = PathBuf::from("tests/all_tags_mockito.json")
            .create_mockito_server(r"^/tags/get.*$", 200);
        let api = Api::new(include_str!("api_token.txt"));
        let res = api.tags_frequency();
        let r = res.unwrap_or_else(|e| panic!("{:?}", e));
        assert_eq!(94, r.len());
    }

    #[test]
    fn test_tag_freq_empty() {
        let _ = env_logger::try_init();
        debug!("test_tag_freq_empty: starting.");
        {
            let _m1 = "[]".create_mockito_server(r"^/tags/get.*$", 201);
            let api = Api::new(include_str!("api_token.txt"));
            let res = api.tags_frequency();
            let r = res.unwrap_or_else(|e| panic!("{:?}", e));
            assert!(r.is_empty());
        }
        {
            let _m1 = "{}".create_mockito_server(r"^/tags/get.*$", 201);
            let api = Api::new(include_str!("api_token.txt"));
            let res = api.tags_frequency();
            let r = res.unwrap_or_else(|e| panic!("{:?}", e));
            assert!(r.is_empty());
        }
    }

    #[test]
    fn test_all_pins() {
        let _ = env_logger::try_init();
        debug!("test_all_pins: starting.");
        let _m1 = start_mockito_server(
            r"^/posts/all.*$",
            200,
            PathBuf::from("tests/all_pins_mockito.json"),
        );
        let api = Api::new(include_str!("api_token.txt"));
        let res = api.all_pins();

        assert_eq!(57, res.unwrap_or_else(|e| panic!("{:?}", e)).len());
    }

    #[test]
    fn test_all_pins_empty() {
        let _ = env_logger::try_init();
        debug!("test_all_pins: starting.");
        {
            let _m1 = "[]".create_mockito_server(r"^/posts/all.*$", 200);
            let api = Api::new(include_str!("api_token.txt"));
            let res = api.all_pins();

            assert_eq!(0, res.unwrap_or_else(|e| panic!("{:?}", e)).len());
        }
    }
}
