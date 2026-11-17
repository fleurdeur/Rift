use crate::model::{LyricLineData, Track};
use crate::paths;

pub fn parse_lrc(text: &str) -> Vec<LyricLineData> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut rest = line;
        let mut times = Vec::new();
        while rest.starts_with('[') {
            let Some(end) = rest.find(']') else { break };
            let inside = &rest[1..end];
            if let Some(t) = parse_timestamp(inside) {
                times.push(t);
                rest = rest[end + 1..].trim_start();
            } else {
                rest = "";
                break;
            }
        }
        if times.is_empty() || rest.is_empty() {
            continue;
        }
        for time in times {
            out.push(LyricLineData {
                time,
                text: rest.to_string(),
            });
        }
    }
    out.sort_by_key(|l| l.time);
    out
}

fn parse_timestamp(s: &str) -> Option<i64> {
    let (m, rest) = s.split_once(':')?;
    let minutes: i64 = m.parse().ok()?;
    let (secs, frac) = if let Some((a, b)) = rest.split_once('.') {
        (a, b)
    } else if let Some((a, b)) = rest.split_once(':') {
        (a, b)
    } else {
        (rest, "0")
    };
    let seconds: i64 = secs.parse().ok()?;
    let frac: String = frac.chars().take(3).collect();
    let millis: i64 = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 100,
        2 => frac.parse::<i64>().ok()? * 10,
        _ => frac.parse::<i64>().ok()?,
    };
    Some((minutes * 60 + seconds) * 1000 + millis)
}

fn plain_to_lines(plain: &str) -> Vec<LyricLineData> {
    plain
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !s.starts_with('['))
        .enumerate()
        .map(|(i, line)| LyricLineData {
            time: (i as i64) * 3200,
            text: line.to_string(),
        })
        .collect()
}

fn cache_path(track: &Track) -> std::path::PathBuf {
    paths::paths()
        .lyrics
        .join(format!("{}.json", crate::model::hash_id(&track.id)))
}

fn cached(track: &Track) -> Option<Vec<LyricLineData>> {
    let path = cache_path(track);
    crate::model::storage_load_json::<Vec<LyricLineData>>(&path).filter(|l| !l.is_empty())
}

fn save_cache(track: &Track, lines: &[LyricLineData]) {
    let _ = crate::model::storage_save_json(&cache_path(track), &lines.to_vec());
}

fn client() -> reqwest::blocking::Client {
    crate::images::http_client()
}

/// LRCLIB is the primary synced-lyrics source.
fn lrclib(track: &Track) -> anyhow::Result<Vec<LyricLineData>> {
    let c = client();
    let exact = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}&album_name={}&duration={}",
        urlencoding::encode(&track.artist),
        urlencoding::encode(&track.title),
        urlencoding::encode(&track.album),
        track.duration
    );
    let parse = |data: &serde_json::Value| -> Vec<LyricLineData> {
        if let Some(synced) = data.get("syncedLyrics").and_then(|x| x.as_str()) {
            let lines = parse_lrc(synced);
            if !lines.is_empty() {
                return lines;
            }
        }
        data.get("plainLyrics")
            .and_then(|x| x.as_str())
            .map(plain_to_lines)
            .unwrap_or_default()
    };
    if let Ok(resp) = c
        .get(&exact)
        .header("User-Agent", "Rift/1.0 (https://github.com/fleurdeur/Rift)")
        .send()
    {
        if resp.status().is_success() {
            if let Ok(data) = resp.json::<serde_json::Value>() {
                let lines = parse(&data);
                if !lines.is_empty() {
                    return Ok(lines);
                }
            }
        }
    }
    // Fuzzy lookup when the duration/album does not match exactly.
    let search = format!(
        "https://lrclib.net/api/search?artist_name={}&track_name={}",
        urlencoding::encode(&track.artist),
        urlencoding::encode(&track.title)
    );
    let resp = c
        .get(&search)
        .header("User-Agent", "Rift/1.0 (https://github.com/fleurdeur/Rift)")
        .send()?;
    if !resp.status().is_success() {
        anyhow::bail!("LRCLIB search failed");
    }
    let list: Vec<serde_json::Value> = resp.json()?;
    for item in &list {
        let lines = parse(item);
        if !lines.is_empty() {
            return Ok(lines);
        }
    }
    anyhow::bail!("LRCLIB has no match")
}

/// Plain-text fallback. lyrics.ovh has no timestamps, so lines are paced.
fn lyrics_ovh(track: &Track) -> anyhow::Result<Vec<LyricLineData>> {
    let url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        urlencoding::encode(&track.artist),
        urlencoding::encode(&track.title)
    );
    let resp = client().get(&url).send()?;
    if !resp.status().is_success() {
        anyhow::bail!("lyrics.ovh has no match");
    }
    let data: serde_json::Value = resp.json()?;
    let text = data.get("lyrics").and_then(|x| x.as_str()).unwrap_or("");
    let lines = plain_to_lines(text);
    if lines.is_empty() {
        anyhow::bail!("lyrics.ovh returned empty lyrics");
    }
    Ok(lines)
}

/// Community catalogue fallback with original-language and translated text.
fn netease(track: &Track) -> anyhow::Result<Vec<LyricLineData>> {
    let c = client();
    let query = format!("{} {}", track.title, track.artist);
    let url = format!(
        "https://music.163.com/api/search/get/web?s={}&type=1&limit=3&offset=0",
        urlencoding::encode(&query)
    );
    let resp = c
        .get(&url)
        .header("Referer", "https://music.163.com/")
        .header("User-Agent", "Mozilla/5.0 (Linux; Android 13) Rift/1.0")
        .send()?;
    if !resp.status().is_success() {
        anyhow::bail!("NetEase search failed");
    }
    let data: serde_json::Value = resp.json()?;
    let songs = data
        .get("result")
        .and_then(|r| r.get("songs"))
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    for song in songs {
        let Some(id) = song.get("id").and_then(|i| i.as_i64()) else { continue };
        let lyric_url = format!(
            "https://music.163.com/api/song/lyric?id={}&lv=1&kv=1&tv=-1",
            id
        );
        let Ok(r) = c
            .get(&lyric_url)
            .header("Referer", "https://music.163.com/")
            .header("User-Agent", "Mozilla/5.0 (Linux; Android 13) Rift/1.0")
            .send()
        else {
            continue;
        };
        let Ok(v) = r.json::<serde_json::Value>() else { continue };
        let text = v
            .get("lrc")
            .and_then(|l| l.get("lyric"))
            .and_then(|l| l.as_str())
            .unwrap_or("");
        let lines = parse_lrc(text);
        if !lines.is_empty() {
            return Ok(lines);
        }
    }
    anyhow::bail!("NetEase has no match")
}

pub fn load_for_track_with_source(track: &Track) -> anyhow::Result<(Vec<LyricLineData>, &'static str)> {
    if let Some(lrc) = crate::library::find_lrc_for(track) {
        if let Ok(text) = std::fs::read_to_string(lrc) {
            let lines = parse_lrc(&text);
            if !lines.is_empty() {
                return Ok((lines, "Local .lrc"));
            }
        }
    }
    if let Some(lines) = cached(track) {
        return Ok((lines, "Cache"));
    }
    let mut errors = Vec::new();
    for (provider, result) in [
        ("LRCLIB", lrclib(track)),
        ("lyrics.ovh", lyrics_ovh(track)),
        ("NetEase", netease(track)),
    ] {
        match result {
            Ok(lines) if !lines.is_empty() => {
                save_cache(track, &lines);
                return Ok((lines, provider));
            }
            Ok(_) => errors.push(format!("{provider}: empty")),
            Err(e) => errors.push(format!("{provider}: {e}")),
        }
    }
    anyhow::bail!("No lyrics found ({})", errors.join("; "))
}

pub fn load_for_track(track: &Track) -> anyhow::Result<Vec<LyricLineData>> {
    load_for_track_with_source(track).map(|(lines, _)| lines)
}
