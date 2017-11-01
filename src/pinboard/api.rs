const TOKEN: &'static str = include_str!("auth_token.txt");

use serde_json;
use reqwest;
use reqwest::IntoUrl;

use chrono::prelude::*;
use url::Url;

use std::io::Read;
use std::collections::HashMap;

use Pin;

#[derive(Serialize, Deserialize, Debug)]
struct ApiResult {
    result_code: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct UpdateTime {
    #[serde(rename = "update_time")]
    datetime: DateTime<Utc>,
}

fn add_auth_token<T: IntoUrl>(url: T) -> Url {
    Url::parse_with_params(url.into_url().unwrap().as_ref(),
                           &[("format", "json"), ("auth_token", TOKEN)])
        .unwrap()
}

pub fn add_url(p: Pin) -> Result<(), String> {
    let mut map = HashMap::new();
    let url = p.url.into_string();

    map.insert("url", url.clone());
    map.insert("description", p.title);
    map.insert("toread", p.toread);
    map.insert("extended", p.extended.unwrap_or_default());
    map.insert("shared", p.shared);
    map.insert("replace", "yes".to_string());

    let res = get_api_response("https://api.pinboard.in/v1/posts/add", map)?;
    let res: Result<ApiResult, _> = serde_json::from_str(&res);

    println!("{:?}", res);
    match res {
        Ok(ref r) if r.result_code == "done" => Ok(()),
        Ok(r) => Err(r.result_code),
        Err(e) => Err(format!("Unrecognized response from server: {:?}", e)),
    }
}

pub fn delete<T: IntoUrl>(url: T) -> Result<(), String> {
    let mut map = HashMap::new();
    let url = url.into_url().unwrap().to_string();
    map.insert("url", url.clone());
    let resp = get_api_response("https://api.pinboard.in/v1/posts/delete", map)?;

    let resp: Result<ApiResult, _> = serde_json::from_str(&resp);
    print!("{:?}", resp);
    match resp {
        Ok(ref r) if r.result_code == "done" => Ok(()),
        Ok(_) | Err(_) => Err(format!("Couldn't delete {:?}", url)),
    }
}

pub fn recent_update() -> Result<DateTime<Utc>, String> {
    let content = get_api_response("https://api.pinboard.in/v1/posts/update", HashMap::new())?;
    let date: Result<UpdateTime, _> = serde_json::from_str(&content);
    match date {
        Ok(date) => Ok(date.datetime),
        Err(e) => Err(e.to_string()),
    }
}

fn get_api_response<T: IntoUrl>(endpoint: T,
                                params: HashMap<&str, String>)
                                -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut api_url = add_auth_token(endpoint);

    for (k, v) in &params {
        api_url.query_pairs_mut().append_pair(k, v);
    }
    let res = client.get(api_url).send();

    println!("{:?}", res);
    let mut resp = match res {
        Ok(msg) => msg,
        Err(e) => return Err(e.to_string()),
    };

    // TODO: check for error status codes and return them instead of panicking.
    assert!(resp.status().is_success());

    let mut content = String::new();
    if let Err(e) = resp.read_to_string(&mut content) {
        return Err(e.to_string());
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_latest_update_time() {
        let r = recent_update();
        assert!(r.is_ok());
        println!("{:?}", r.unwrap());
    }

    #[test]
    fn delete_a_pin() {
        let r = delete("http://git.hamid.cc");
        println!("{:?}", r);
    }

    #[test]
    fn add_a_url() {
        let url = Url::parse("https://githuуй.com/Здравствуйт?q=13#fragment").unwrap();
        let p = Pin::new(url, "what".to_string(), vec![], true, false, None);
        let res = add_url(p);
        res.expect("Error in adding");
    }
}