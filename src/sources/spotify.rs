use crate::model::{Track, Source};
use serde_json::Value;

fn client() -> reqwest::blocking::Client {
    crate::images::http_client()
}

pub fn is_spotify(url: &str) -> bool {
    url.contains("open.spotify.com") || url.starts_with("spotify:")
}

pub fn playlist_tracks(url: &str) -> anyhow::Result<(String, Vec<Track>)> {
    let id = url
        .split("playlist/")
        .nth(1)
        .or_else(|| url.strip_prefix("spotify:playlist:"))
        .map(|s| s.split(['?', '/', '#']).next().unwrap_or(s))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Invalid Spotify playlist link"))?;
    let html = client()
        .get(format!("https://open.spotify.com/embed/playlist/{id}"))
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/124 Safari/537.36")
        .send()?
        .error_for_status()?
        .text()?;
    let start = html
        .find("<script id=\"__NEXT_DATA__\"")
        .and_then(|p| html[p..].find('>').map(|q| p + q + 1))
        .ok_or_else(|| anyhow::anyhow!("Spotify page format changed"))?;
    let end = html[start..]
        .find("</script>")
        .map(|q| start + q)
        .ok_or_else(|| anyhow::anyhow!("Spotify page format changed"))?;
    let data: Value = serde_json::from_str(&html[start..end])?;
    let entity = data
        .pointer("/props/pageProps/state/data/entity")
        .ok_or_else(|| anyhow::anyhow!("Could not read Spotify playlist"))?;
    let title = entity
        .get("title")
        .or_else(|| entity.get("name"))
        .and_then(|x| x.as_str())
        .unwrap_or("Imported Spotify playlist")
        .to_string();
    let cover = entity
        .pointer("/coverArt/sources/0/url")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let list = entity
        .get("trackList")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for item in list {
        let Some(tid) = item
            .get("uri")
            .and_then(|x| x.as_str())
            .and_then(|u| u.strip_prefix("spotify:track:"))
        else {
            continue;
        };
        let title = item
            .get("title")
            .and_then(|x| x.as_str())
            .unwrap_or("Unknown title")
            .to_string();
        let artist = item
            .get("subtitle")
            .and_then(|x| x.as_str())
            .unwrap_or("Spotify")
            .replace('\u{fffd}', ",")
            .replace('\u{200b}', "");
        let duration = item
            .get("duration")
            .and_then(|x| x.as_u64())
            .map(|ms| ms / 1000)
            .unwrap_or(0);
        let preview = item
            .pointer("/audioPreview/url")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let id = format!("sp:{tid}");
        let mut t = Track::new(id.clone(), title, artist, Source::Spotify);
        t.duration = duration.min(30);
        t.artwork = cover.clone();
        t.stream_url = preview;
        t.accent = crate::model::accent_for(&id);
        t.album = "Spotify preview".into();
        out.push(t);
    }
    if out.is_empty() {
        anyhow::bail!("This Spotify playlist has no public tracks");
    }
    Ok((title, out))
}

pub fn extract_track_id(id: &str) -> Option<&str> {
    id.strip_prefix("sp:")
}

/// Official Spotify Web API (Client Credentials flow). This is the supported,
/// public API path: the user supplies their own Spotify developer app id/secret.
/// It provides catalog metadata and 30-second previews. Spotify does not expose
/// full-length or lossless streams to third-party apps, so lossless playback is
/// only possible for local/owned files (matched automatically below).
pub fn client_token(client_id: &str, client_secret: &str) -> anyhow::Result<(String, i64)> {
    let res = client()
        .post("https://accounts.spotify.com/api/token")
        .basic_auth(client_id, Some(client_secret))
        .form(&[("grant_type", "client_credentials")])
        .send()?;
    let status = res.status();
    let body: Value = res.json()?;
    if !status.is_success() {
        anyhow::bail!(
            "Spotify auth failed: {}",
            body.get("error_description")
                .or_else(|| body.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("invalid credentials")
        );
    }
    let token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Spotify returned no token"))?;
    let expires = body.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);
    Ok((token.to_string(), crate::model::now_millis() + expires * 1000 - 60_000))
}

pub fn search_tracks(token: &str, query: &str, limit: u32) -> anyhow::Result<Vec<Track>> {
    let res = client()
        .get("https://api.spotify.com/v1/search")
        .bearer_auth(token)
        .query(&[("q", query), ("type", "track"), ("limit", &limit.to_string())])
        .header("Accept", "application/json")
        .send()?;
    let status = res.status();
    let body: Value = res.json()?;
    if !status.is_success() {
        anyhow::bail!(
            "Spotify search failed: {}",
            body.pointer("/error/message")
                .and_then(|v| v.as_str())
                .unwrap_or("request rejected")
        );
    }
    let items = body
        .pointer("/tracks/items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for item in items {
        let Some(tid) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let title = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown title");
        let artist = item
            .get("artists")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.get("name").and_then(|n| n.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let album = item
            .pointer("/album/name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let duration = item
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .map(|ms| ms / 1000)
            .unwrap_or(0);
        let artwork = item
            .pointer("/album/images/0/url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let preview = item
            .get("preview_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let id = format!("sp:{tid}");
        let mut t = Track::new(id.clone(), title, artist, Source::Spotify);
        t.album = album;
        t.duration = duration;
        t.artwork = artwork;
        t.stream_url = preview;
        t.accent = crate::model::accent_for(&id);
        out.push(t);
    }
    if out.is_empty() {
        anyhow::bail!("Spotify returned no tracks");
    }
    Ok(out)
}

pub fn playlist_tracks_api(token: &str, url: &str) -> anyhow::Result<(String, Vec<Track>)> {
    let id = url
        .split("playlist/")
        .nth(1)
        .or_else(|| url.strip_prefix("spotify:playlist:"))
        .map(|s| s.split(['?', '/', '#']).next().unwrap_or(s))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Invalid Spotify playlist link"))?;
    let res = client()
        .get(format!("https://api.spotify.com/v1/playlists/{id}"))
        .bearer_auth(token)
        .query(&[("fields", "name,tracks.items(track(id,name,artists(name),album(name,images),duration_ms,preview_url))")])
        .send()?;
    let status = res.status();
    let body: Value = res.json()?;
    if !status.is_success() {
        anyhow::bail!("Spotify playlist failed: HTTP {status}");
    }
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("Spotify playlist").to_string();
    let items = body.pointer("/tracks/items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for entry in items {
        let Some(item) = entry.get("track") else { continue };
        let Some(tid) = item.get("id").and_then(|v| v.as_str()) else { continue };
        let title = item.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown title");
        let artist = item.get("artists").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.get("name").and_then(|n| n.as_str())).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let album = item.pointer("/album/name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let artwork = item.pointer("/album/images/0/url").and_then(|v| v.as_str()).map(|s| s.to_string());
        let duration = item.get("duration_ms").and_then(|v| v.as_u64()).map(|ms| ms/1000).unwrap_or(0);
        let preview = item.get("preview_url").and_then(|v| v.as_str()).map(|s| s.to_string());
        let id = format!("sp:{tid}");
        let mut t = Track::new(id.clone(), title, artist, Source::Spotify);
        t.album = album; t.duration = duration; t.artwork = artwork; t.stream_url = preview;
        t.accent = crate::model::accent_for(&id);
        out.push(t);
    }
    if out.is_empty() { anyhow::bail!("This Spotify playlist has no public tracks"); }
    Ok((name, out))
}
