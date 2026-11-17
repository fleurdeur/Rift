use crate::model::{Track, Source};
use serde_json::Value;
use std::time::Duration;

fn client() -> reqwest::blocking::Client {
    crate::images::http_client()
}

static CLIENT_ID: std::sync::OnceLock<std::sync::Mutex<Option<(String, i64)>>> =
    std::sync::OnceLock::new();

fn cache() -> &'static std::sync::Mutex<Option<(String, i64)>> {
    CLIENT_ID.get_or_init(|| std::sync::Mutex::new(None))
}

pub fn client_id() -> anyhow::Result<String> {
    if let Ok(guard) = cache().lock() {
        if let Some((id, ts)) = guard.as_ref() {
            if crate::model::now() - ts < 43_200 {
                return Ok(id.clone());
            }
        }
    }
    let id = scrape_client_id()?;
    if let Ok(mut guard) = cache().lock() {
        *guard = Some((id.clone(), crate::model::now()));
    }
    Ok(id)
}

fn scrape_client_id() -> anyhow::Result<String> {
    let html = client()
        .get("https://soundcloud.com/")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/124 Safari/537.36")
        .timeout(Duration::from_secs(25))
        .send()?
        .error_for_status()?
        .text()?;
    let mut assets: Vec<String> = Vec::new();
    let mut rest = html.as_str();
    while let Some(pos) = rest.find("<script") {
        rest = &rest[pos..];
        if let Some(end) = rest.find('>') {
            let tag = &rest[..end];
            if let Some(src_pos) = tag.find("src=\"") {
                let s = &tag[src_pos + 5..];
                if let Some(q) = s.find('"') {
                    let url = &s[..q];
                    if url.contains("sndcdn.com/assets") {
                        assets.push(if url.starts_with("http") {
                            url.to_string()
                        } else {
                            format!("https:{url}")
                        });
                    }
                }
            }
            rest = &rest[end..];
        } else {
            break;
        }
    }
    for url in assets.iter().rev() {
        let Ok(js) = client().get(url).send().and_then(|r| r.text()) else {
            continue;
        };
        for pat in ["client_id:\"", "client_id=\"", "client_id=\"", "\"client_id\":\""] {
            if let Some(pos) = js.find(pat) {
                let s = &js[pos + pat.len()..];
                let id: String = s
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if id.len() >= 20 {
                    return Ok(id);
                }
            }
        }
    }
    anyhow::bail!("Could not obtain SoundCloud client id")
}

fn api_get(url: &str) -> anyhow::Result<Value> {
    let text = client()
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/124 Safari/537.36")
        .header("Accept", "application/json, text/plain, */*")
        .header("Origin", "https://soundcloud.com")
        .header("Referer", "https://soundcloud.com/")
        .timeout(Duration::from_secs(30))
        .send()?
        .error_for_status()?
        .text()?;
    Ok(serde_json::from_str(&text)?)
}

pub fn search(query: &str, limit: usize) -> anyhow::Result<Vec<Track>> {
    let cid = client_id()?;
    let url = format!(
        "https://api-v2.soundcloud.com/search/tracks?q={}&client_id={}&limit={}&linked_partitioning=1",
        urlencoding::encode(query),
        cid,
        limit.max(1)
    );
    let data = api_get(&url)?;
    let Some(items) = data.get("collection").and_then(|x| x.as_array()) else {
        anyhow::bail!("No SoundCloud results");
    };
    let mut out = Vec::new();
    for item in items {
        if let Some(t) = track_from_json(item) {
            out.push(t);
        }
    }
    if out.is_empty() {
        anyhow::bail!("No SoundCloud results");
    }
    Ok(out)
}

fn track_from_json(v: &Value) -> Option<Track> {
    let id = v.get("id")?.as_i64()?;
    let title = v
        .get("title")
        .and_then(|x| x.as_str())
        .unwrap_or("Unknown title")
        .to_string();
    let artist = v
        .pointer("/user/username")
        .and_then(|x| x.as_str())
        .unwrap_or("SoundCloud")
        .to_string();
    let duration = v
        .get("duration")
        .and_then(|x| x.as_u64())
        .map(|ms| ms / 1000)
        .unwrap_or(0);
    let artwork = v
        .get("artwork_url")
        .and_then(|x| x.as_str())
        .or_else(|| v.pointer("/user/avatar_url").and_then(|x| x.as_str()))
        .map(|u| u.replace("-large", "-t500x500"));
    let permalink = v
        .get("permalink_url")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if !permalink.is_empty() {
        // Keep the permalink in stream_url for reference; actual playback resolves HLS.
    }
    let tid = format!("sc:{id}");
    let mut t = Track::new(tid.clone(), title, artist, Source::Soundcloud);
    t.duration = duration;
    t.artwork = artwork;
    t.stream_url = Some(permalink);
    t.accent = crate::model::accent_for(&tid);
    Some(t)
}

pub fn hls_url(track_id: i64) -> anyhow::Result<String> {
    let cid = client_id()?;
    let data = api_get(&format!(
        "https://api-v2.soundcloud.com/tracks/{track_id}?client_id={cid}"
    ))?;
    let transcodings = data
        .pointer("/media/transcodings")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut chosen = None;
    for t in &transcodings {
        let protocol = t
            .pointer("/format/protocol")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let mime = t
            .pointer("/format/mime_type")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if protocol == "hls" && mime.contains("mp4") {
            chosen = t.get("url").and_then(|x| x.as_str());
            break;
        }
    }
    let Some(url) = chosen else {
        anyhow::bail!("This SoundCloud track is not streamable");
    };
    let resolved = api_get(&format!(
        "{url}?client_id={cid}"
    ))?;
    let url = resolved
        .get("url")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("Could not resolve SoundCloud stream"))?;
    Ok(url.to_string())
}

pub fn extract_track_id(id: &str) -> Option<i64> {
    id.strip_prefix("sc:")?.parse().ok()
}

pub fn playlist_tracks(url: &str) -> anyhow::Result<(String, Vec<Track>)> {
    let cid = client_id()?;
    let api = format!(
        "https://api-v2.soundcloud.com/resolve?url={}&client_id={}",
        urlencoding::encode(url),
        cid
    );
    let data = api_get(&api)?;
    let title = data
        .get("title")
        .or_else(|| data.get("permalink"))
        .and_then(|x| x.as_str())
        .unwrap_or("Imported SoundCloud playlist")
        .to_string();
    let tracks = data
        .get("tracks")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for t in tracks {
        if let Some(track) = track_from_json(&t) {
            out.push(track);
        }
    }
    if out.is_empty() {
        anyhow::bail!("This SoundCloud set is empty");
    }
    Ok((title, out))
}
