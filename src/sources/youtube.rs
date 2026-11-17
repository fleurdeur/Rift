use crate::model::{Track, Source};
use serde_json::{json, Value};
use std::time::Duration;

const ANDROID_VR_UA: &str = "com.google.android.apps.youtube.vr.oculus/1.61.48 (Linux; U; Android 12; en_US; Quest 3; Build/SQ3A.220605.009.A1) gzip";

fn client() -> reqwest::blocking::Client {
    crate::images::http_client()
}

fn post_json(url: &str, body: Value, android: bool) -> anyhow::Result<Value> {
    let mut req = client()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("X-Goog-Api-Format-Version", "2");
    if android {
        req = req
            .header("User-Agent", ANDROID_VR_UA)
            .header("X-YouTube-Client-Name", "28")
            .header("X-YouTube-Client-Version", "1.61.48");
    } else {
        req = req.header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/124 Safari/537.36");
    }
    let resp = req.json(&body).send()?.error_for_status()?;
    Ok(resp.json()?)
}

fn web_context() -> Value {
    json!({
        "client": {
            "clientName": "WEB",
            "clientVersion": "2.20240101.00.00",
            "hl": "en",
            "gl": "US"
        }
    })
}

pub fn search(query: &str, limit: usize) -> anyhow::Result<Vec<Track>> {
    let body = json!({
        "context": web_context(),
        "query": query,
        "params": "EgIQAQ==",
        "contentCheckOk": true,
        "racyCheckOk": true
    });
    let data = post_json(
        "https://www.youtube.com/youtubei/v1/search?prettyPrint=false",
        body,
        false,
    )?;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_videos(&data, &mut out, &mut seen);
    if out.len() < limit {
        collect_playlists(&data, &mut out, &mut seen);
    }
    out.truncate(limit);
    if out.is_empty() {
        anyhow::bail!("No YouTube results");
    }
    Ok(out)
}

fn collect_videos(node: &Value, out: &mut Vec<Track>, seen: &mut std::collections::HashSet<String>) {
    match node {
        Value::Object(map) => {
            if let Some(v) = map.get("videoRenderer") {
                if let Some(t) = video_track(v) {
                    if seen.insert(t.id.clone()) {
                        out.push(t);
                    }
                }
            }
            for v in map.values() {
                collect_videos(v, out, seen);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_videos(v, out, seen);
            }
        }
        _ => {}
    }
}

fn collect_playlists(node: &Value, out: &mut Vec<Track>, seen: &mut std::collections::HashSet<String>) {
    match node {
        Value::Object(map) => {
            if let Some(v) = map.get("playlistRenderer") {
                if let Some(id) = v.get("playlistId").and_then(|x| x.as_str()) {
                    if seen.insert(format!("ytpl:{id}")) {
                        let title = text_of(v.get("title")).unwrap_or_else(|| "YouTube playlist".into());
                        let count = v
                            .get("videoCount")
                            .and_then(|x| x.as_str())
                            .unwrap_or("playlist");
                        let mut t = Track::new(
                            format!("ytpl:{id}"),
                            title,
                            format!("YouTube · {}", count),
                            Source::Youtube,
                        );
                        t.accent = crate::model::accent_for(&format!("ytpl:{id}"));
                        out.push(t);
                    }
                }
            }
            for v in map.values() {
                collect_playlists(v, out, seen);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_playlists(v, out, seen);
            }
        }
        _ => {}
    }
}

fn text_of(v: Option<&Value>) -> Option<String> {
    let v = v?;
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    if let Some(s) = v.get("simpleText").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }
    let runs = v.get("runs")?.as_array()?;
    let s: String = runs
        .iter()
        .filter_map(|r| r.get("text").and_then(|x| x.as_str()))
        .collect();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn parse_duration_label(label: &str) -> u64 {
    let parts: Vec<&str> = label.split(':').collect();
    match parts.len() {
        2 => parts[0].parse::<u64>().unwrap_or(0) * 60 + parts[1].parse::<u64>().unwrap_or(0),
        3 => {
            parts[0].parse::<u64>().unwrap_or(0) * 3600
                + parts[1].parse::<u64>().unwrap_or(0) * 60
                + parts[2].parse::<u64>().unwrap_or(0)
        }
        _ => 0,
    }
}

fn video_track(v: &Value) -> Option<Track> {
    let video_id = v.get("videoId")?.as_str()?;
    let title = text_of(v.get("title")).unwrap_or_else(|| "Unknown title".into());
    let artist = text_of(v.get("ownerText"))
        .or_else(|| text_of(v.get("longBylineText")))
        .or_else(|| text_of(v.get("shortBylineText")))
        .unwrap_or_else(|| "YouTube".into());
    let duration = v
        .get("lengthText")
        .and_then(|x| x.get("simpleText"))
        .and_then(|x| x.as_str())
        .map(parse_duration_label)
        .unwrap_or(0);
    let id = format!("yt:{video_id}");
    let mut t = Track::new(id.clone(), title, artist, Source::Youtube);
    t.duration = duration;
    t.artwork = Some(format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"));
    t.stream_url = Some(format!("https://www.youtube.com/watch?v={video_id}"));
    t.accent = crate::model::accent_for(&id);
    Some(t)
}

pub struct StreamInfo {
    pub url: String,
    pub mime: String,
    pub bitrate: u64,
    pub hls: bool,
    pub duration: u64,
}

pub fn stream_info(video_id: &str, quality: &str) -> anyhow::Result<StreamInfo> {
    let context = json!({
        "client": {
            "clientName": "ANDROID_VR",
            "clientVersion": "1.61.48",
            "deviceMake": "Oculus",
            "deviceModel": "Quest 3",
            "osName": "Android",
            "osVersion": "12",
            "androidSdkVersion": 32,
            "userAgent": ANDROID_VR_UA,
            "hl": "en",
            "gl": "US"
        }
    });
    let body = json!({
        "context": context,
        "videoId": video_id,
        "contentCheckOk": true,
        "racyCheckOk": true
    });
    let data = post_json(
        "https://www.youtube.com/youtubei/v1/player?prettyPrint=false",
        body,
        true,
    )?;
    if let Some(status) = data.pointer("/playabilityStatus/status").and_then(|x| x.as_str()) {
        if status != "OK" {
            let reason = data
                .pointer("/playabilityStatus/reason")
                .and_then(|x| x.as_str())
                .unwrap_or(status);
            anyhow::bail!("YouTube: {reason}");
        }
    }
    let duration = data
        .pointer("/videoDetails/lengthSeconds")
        .and_then(|x| x.as_str())
        .and_then(|x| x.parse::<u64>().ok())
        .unwrap_or(0);
    let Some(formats) = data.pointer("/streamingData/adaptiveFormats").and_then(|x| x.as_array()) else {
        anyhow::bail!("No audio streams available");
    };
    let mut candidates: Vec<(u64, String, String, bool)> = Vec::new();
    for f in formats {
        let mime = f.get("mimeType").and_then(|x| x.as_str()).unwrap_or("");
        if !mime.starts_with("audio/") {
            continue;
        }
        let bitrate = f.get("bitrate").and_then(|x| x.as_u64()).unwrap_or(0);
        let is_hls = f
            .get("url")
            .and_then(|x| x.as_str())
            .map(|u| u.contains(".m3u8"))
            .unwrap_or(false)
            || mime.contains("mpegURL");
        let url = f
            .get("url")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                f.get("signatureCipher")
                    .or_else(|| f.get("cipher"))
                    .and_then(|x| x.as_str())
                    .and_then(|c| {
                        c.split('&')
                            .find_map(|p| p.strip_prefix("url="))
                            .map(|u| urlencoding::decode(u).map(|s| s.to_string()).unwrap_or_else(|_| u.to_string()))
                    })
            });
        if let Some(url) = url {
            candidates.push((bitrate, url, mime.to_string(), is_hls));
        }
    }
    if candidates.is_empty() {
        anyhow::bail!("No playable audio stream found");
    }
    let aac: Vec<_> = candidates
        .iter()
        .filter(|(_, _, m, h)| m.contains("mp4a") || *h)
        .cloned()
        .collect();
    let mut pool = if aac.is_empty() { candidates } else { aac };
    pool.sort_by_key(|c| c.0);
    let chosen = match quality {
        "Low" => pool.first().cloned().unwrap_or_else(|| pool.last().cloned().unwrap()),
        "High" => {
            let target = 160_000u64;
            pool.iter()
                .min_by_key(|c| c.0.abs_diff(target))
                .cloned()
                .unwrap_or_else(|| pool.last().cloned().unwrap())
        }
        _ => pool.last().cloned().unwrap(),
    };
    Ok(StreamInfo {
        url: chosen.1,
        mime: chosen.2,
        bitrate: chosen.0,
        hls: chosen.3,
        duration,
    })
}

pub fn playlist_tracks(list_id: &str) -> anyhow::Result<(String, Vec<Track>)> {
    let body = json!({
        "context": web_context(),
        "browseId": format!("VL{list_id}"),
        "contentCheckOk": true,
        "racyCheckOk": true
    });
    let data = post_json(
        "https://www.youtube.com/youtubei/v1/browse?prettyPrint=false",
        body,
        false,
    )?;
    let title = data
        .pointer("/metadata/playlistMetadataRenderer/title")
        .and_then(|x| x.as_str())
        .or_else(|| {
            data.pointer("/header/playlistHeaderRenderer/title")
                .and_then(|x| text_of(Some(x)))
                .map(|s| Box::leak(s.into_boxed_str()) as &str)
        })
        .unwrap_or("Imported YouTube playlist")
        .to_string();
    let mut tracks = Vec::new();
    let mut seen = std::collections::HashSet::new();
    collect_playlist_videos(&data, &mut tracks, &mut seen);
    if tracks.is_empty() {
        anyhow::bail!("This playlist is empty or private");
    }
    Ok((title, tracks))
}

fn collect_playlist_videos(
    node: &Value,
    out: &mut Vec<Track>,
    seen: &mut std::collections::HashSet<String>,
) {
    match node {
        Value::Object(map) => {
            if let Some(v) = map.get("playlistVideoRenderer") {
                if let Some(video_id) = v.get("videoId").and_then(|x| x.as_str()) {
                    let id = format!("yt:{video_id}");
                    if seen.insert(id.clone()) {
                        let title =
                            text_of(v.get("title")).unwrap_or_else(|| "Unknown title".into());
                        let artist = text_of(v.get("shortBylineText"))
                            .or_else(|| text_of(v.get("longBylineText")))
                            .unwrap_or_else(|| "YouTube".into());
                        let duration = v
                            .get("lengthText")
                            .and_then(|x| x.get("simpleText"))
                            .and_then(|x| x.as_str())
                            .map(parse_duration_label)
                            .unwrap_or(0);
                        let mut t = Track::new(id.clone(), title, artist, Source::Youtube);
                        t.duration = duration;
                        t.artwork =
                            Some(format!("https://i.ytimg.com/vi/{video_id}/hqdefault.jpg"));
                        t.stream_url = Some(format!("https://www.youtube.com/watch?v={video_id}"));
                        t.accent = crate::model::accent_for(&id);
                        out.push(t);
                    }
                }
            }
            for v in map.values() {
                collect_playlist_videos(v, out, seen);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_playlist_videos(v, out, seen);
            }
        }
        _ => {}
    }
}

pub fn extract_playlist_id(url: &str) -> Option<String> {
    if let Some(pos) = url.find("list=") {
        let rest = &url[pos + 5..];
        let id = rest.split(['&', '#']).next().unwrap_or("");
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    None
}

pub fn download_audio(
    info: &StreamInfo,
    dest: &std::path::Path,
) -> anyhow::Result<()> {
    if dest.exists() && std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0) > 32_000 {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if info.hls {
        crate::sources::hls::download_hls(&info.url, dest)?;
        return Ok(());
    }
    let bytes = client()
        .get(&info.url)
        .header("User-Agent", ANDROID_VR_UA)
        .header("Accept", "*/*")
        .header("Origin", "https://www.youtube.com")
        .header("Referer", "https://www.youtube.com/")
        .timeout(Duration::from_secs(120))
        .send()?
        .error_for_status()?
        .bytes()?;
    if bytes.len() < 4096 {
        anyhow::bail!("Audio download was too small");
    }
    std::fs::write(dest, &bytes)?;
    Ok(())
}

pub fn get_video_id(id: &str) -> Option<&str> {
    id.strip_prefix("yt:")
}
