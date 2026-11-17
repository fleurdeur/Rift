use std::io::Write;
use std::time::Duration;

fn client() -> reqwest::blocking::Client {
    crate::images::http_client()
}

fn get_text(url: &str) -> anyhow::Result<String> {
    let mut last = None;
    for attempt in 0..3 {
        match client()
            .get(url)
            .timeout(Duration::from_secs(45))
            .header("User-Agent", "Mozilla/5.0 (Linux; Android 13) Rift/1.0")
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
        {
            Ok(t) => return Ok(t),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(400 * (attempt + 1)));
            }
        }
    }
    Err(anyhow::anyhow!(
        "HLS request failed: {}",
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

fn get_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let mut last = None;
    for attempt in 0..4 {
        match client()
            .get(url)
            .timeout(Duration::from_secs(60))
            .header("User-Agent", "Mozilla/5.0 (Linux; Android 13) Rift/1.0")
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes())
        {
            Ok(b) => return Ok(b.to_vec()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(500 * (attempt + 1)));
            }
        }
    }
    Err(anyhow::anyhow!(
        "HLS segment failed: {}",
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

fn join_url(base: &str, rel: &str) -> String {
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return rel.to_string();
    }
    let clean = base.split(['?', '#']).next().unwrap_or(base);
    if rel.starts_with('/') {
        if let Some(pos) = clean.find("://") {
            let after = &clean[pos + 3..];
            let host_end = after.find('/').map(|i| pos + 3 + i).unwrap_or(clean.len());
            return format!("{}{}", &clean[..host_end], rel);
        }
    }
    let dir = clean.rfind('/').map(|i| &clean[..i]).unwrap_or(clean);
    format!("{dir}/{rel}")
}

pub fn download_hls(url: &str, dest: &std::path::Path) -> anyhow::Result<()> {
    let mut playlist = get_text(url)?;
    // Follow a master playlist if present.
    if playlist.contains("#EXT-X-STREAM-INF") {
        let mut best: Option<(u64, String)> = None;
        let lines: Vec<&str> = playlist.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
                let bw = rest
                    .split(',')
                    .find_map(|p| p.trim().strip_prefix("BANDWIDTH="))
                    .and_then(|v| v.split(',').next())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
                if let Some(next) = lines.get(i + 1).map(|s| s.trim()) {
                    if !next.is_empty() && !next.starts_with('#') {
                        if best.as_ref().map(|b| bw > b.0).unwrap_or(true) {
                            best = Some((bw, join_url(url, next)));
                        }
                    }
                }
            }
        }
        if let Some((_, next)) = best {
            playlist = get_text(&next)?;
            return download_media_playlist(&next, &playlist, dest);
        }
    }
    download_media_playlist(url, &playlist, dest)
}

fn download_media_playlist(
    playlist_url: &str,
    playlist: &str,
    dest: &std::path::Path,
) -> anyhow::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::fs::File::create(dest)?;
    let mut init_url: Option<String> = None;
    let mut segments: Vec<String> = Vec::new();
    for line in playlist.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            let uri = rest
                .split(',')
                .find_map(|p| p.trim().strip_prefix("URI=\""))
                .and_then(|p| p.split('"').next())
                .map(|p| join_url(playlist_url, p));
            init_url = uri;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with("http") || line.ends_with(".ts") || line.ends_with(".m4s") || line.ends_with(".aac") || line.contains(".m4s?") {
            segments.push(join_url(playlist_url, line));
        } else if !line.contains('=') {
            segments.push(join_url(playlist_url, line));
        }
    }
    if segments.is_empty() {
        anyhow::bail!("HLS playlist has no segments");
    }
    if let Some(init) = init_url {
        let bytes = get_bytes(&init)?;
        out.write_all(&bytes)?;
    }
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 && i % 20 == 0 {
            // Keep the OS write cache under control for long tracks.
            out.flush().ok();
        }
        let bytes = get_bytes(seg)?;
        out.write_all(&bytes)?;
    }
    out.flush()?;
    if std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0) < 16_384 {
        anyhow::bail!("HLS download was too small");
    }
    Ok(())
}
