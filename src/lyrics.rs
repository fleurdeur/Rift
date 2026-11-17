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
                // metadata tag, skip whole line
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

pub fn load_for_track(track: &Track) -> anyhow::Result<Vec<LyricLineData>> {
    if let Some(lrc) = crate::library::find_lrc_for(track) {
        if let Ok(text) = std::fs::read_to_string(lrc) {
            let lines = parse_lrc(&text);
            if !lines.is_empty() {
                return Ok(lines);
            }
        }
    }
    let cache_path = paths::paths()
        .lyrics
        .join(format!("{}.json", crate::model::hash_id(&track.id)));
    if let Some(lines) = crate::model::storage_load_json::<Vec<LyricLineData>>(&cache_path) {
        if !lines.is_empty() {
            return Ok(lines);
        }
    }
    let client = crate::images::http_client();
    let url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}&album_name={}&duration={}",
        urlencoding::encode(&track.artist),
        urlencoding::encode(&track.title),
        urlencoding::encode(&track.album),
        track.duration
    );
    let resp = client
        .get(&url)
        .header("User-Agent", "Rift/1.0 (https://github.com/fleurdeur/Rift)")
        .send()?;
    if !resp.status().is_success() {
        anyhow::bail!("No lyrics found");
    }
    let data: serde_json::Value = resp.json()?;
    let mut lines = Vec::new();
    if let Some(synced) = data.get("syncedLyrics").and_then(|x| x.as_str()) {
        lines = parse_lrc(synced);
    }
    if lines.is_empty() {
        if let Some(plain) = data.get("plainLyrics").and_then(|x| x.as_str()) {
            for (i, line) in plain.lines().enumerate() {
                if !line.trim().is_empty() {
                    lines.push(LyricLineData {
                        time: (i as i64) * 3000,
                        text: line.to_string(),
                    });
                }
            }
        }
    }
    if lines.is_empty() {
        anyhow::bail!("No lyrics found");
    }
    let _ = crate::model::storage_save_json(&cache_path, &lines);
    Ok(lines)
}
