use crate::images;
use crate::model::{hash_id, Track, Source};
use crate::paths;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;
use std::io::Write;
use std::path::{Path, PathBuf};

const AUDIO_EXTS: [&str; 7] = ["mp3", "flac", "m4a", "aac", "wav", "ogg", "oga"];

pub fn scan_all(core: &mut crate::model::Core) {
    let mut found: Vec<Track> = Vec::new();
    for dir in paths::local_music_dirs() {
        scan_dir(&dir, 0, &mut found, 16);
    }
    // Keep existing tracks to preserve metadata and local paths.
    let mut ids = Vec::new();
    for mut t in found {
        if let Some(existing) = core.tracks.get(&t.id) {
            t.liked = existing.liked;
            t.play_count = existing.play_count;
            if t.artwork_path.is_none() {
                t.artwork_path = existing.artwork_path.clone();
            }
            if t.accent.is_empty() {
                t.accent = existing.accent.clone();
            }
        }
        ids.push(core.add_track(t));
    }
    ids.sort();
    ids.dedup();
    core.local_ids = ids;
    core.local_ids.sort_by(|a, b| {
        let ta = core.tracks.get(a).map(|t| t.title.to_lowercase()).unwrap_or_default();
        let tb = core.tracks.get(b).map(|t| t.title.to_lowercase()).unwrap_or_default();
        ta.cmp(&tb)
    });
    core.save_library(paths::paths());
}

fn scan_dir(dir: &Path, depth: usize, out: &mut Vec<Track>, max_depth: usize) {
    if depth > max_depth {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with('$') {
            continue;
        }
        if path.is_dir() {
            if name.eq_ignore_ascii_case("Android/data")
                || name.eq_ignore_ascii_case("WhatsApp")
                || name.eq_ignore_ascii_case("Telegram")
            {
                continue;
            }
            scan_dir(&path, depth + 1, out, max_depth);
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !AUDIO_EXTS.contains(&ext.as_str()) {
            continue;
        }
        if let Some(track) = read_track(&path) {
            out.push(track);
        }
    }
}

fn read_track(path: &Path) -> Option<Track> {
    let path_str = path.to_string_lossy().to_string();
    let id = format!("local:{}", hash_id(&path_str));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Unknown track".into());
    let mut track = Track::new(id.clone(), stem.clone(), "Unknown artist", Source::Local);
    track.local_path = Some(path_str);
    track.accent = crate::model::accent_for(&id);
    if let Ok(tagged) = lofty::read_from_path(path) {
        let props = tagged.properties();
        track.duration = props.duration().as_secs();
        if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
            if let Some(t) = tag.title().filter(|s| !s.trim().is_empty()) {
                track.title = t.to_string();
            }
            if let Some(a) = tag.artist().filter(|s| !s.trim().is_empty()) {
                track.artist = a.to_string();
            }
            if let Some(al) = tag.album().filter(|s| !s.trim().is_empty()) {
                track.album = al.to_string();
            }
            if let Some(pic) = tag.pictures().first() {
                if let Some(saved) = images::save_embedded(pic.data(), &id) {
                    track.artwork_path = Some(saved.clone());
                    if let Some(color) = images::image_accent(Path::new(&saved)) {
                        track.accent = color;
                    }
                }
            }
        }
    }
    Some(track)
}

pub fn create_demo_tracks(core: &mut crate::model::Core) {
    let demos = [
        ("1", "Velvet Skyline", "Rift Sessions", [220.0, 277.18, 329.63], "#8b7bff"),
        ("2", "Midnight Circuit", "Rift Sessions", [174.61, 220.0, 261.63], "#4fd1c5"),
        ("3", "Golden Hour", "Rift Sessions", [196.0, 246.94, 293.66], "#ff8a5c"),
    ];
    for (n, title, artist, chord, accent) in demos {
        let id = format!("demo:{n}");
        let path = paths::paths().audio.join(format!("riff-demo-{n}.wav"));
        if !path.exists() {
            if generate_wav(&path, &chord).is_err() {
                continue;
            }
        }
        let art_path = paths::paths().art.join(format!("riff-demo-{n}.png"));
        if !art_path.exists() {
            let _ = generate_art(&art_path, accent, n.parse::<u32>().unwrap_or(1));
        }
        let mut t = Track::new(
            id.clone(),
            title,
            artist,
            Source::Demo,
        );
        t.album = "Offline Sessions".into();
        t.duration = 48;
        t.local_path = Some(path.to_string_lossy().to_string());
        t.artwork_path = Some(art_path.to_string_lossy().to_string());
        t.accent = accent.to_string();
        if core.tracks.get(&id).is_some() {
            continue;
        }
        core.add_track(t);
        if !core.local_ids.contains(&id) {
            core.local_ids.push(id);
        }
    }
}

fn generate_wav(path: &Path, chord: &[f32; 3]) -> anyhow::Result<()> {
    let sample_rate = 44_100u32;
    let seconds = 48u32;
    let total = (sample_rate * seconds) as usize;
    let mut file = std::fs::File::create(path)?;
    let data_size = (total * 2) as u32;
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_size).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&(sample_rate * 2).to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;
    let mut phase = [0.0f32; 3];
    for i in 0..total {
        let t = i as f32 / sample_rate as f32;
        // slow evolving pad, chord changes every 12 seconds
        let section = (t / 12.0).floor() as usize % 3;
        let root = chord[section] * 0.5;
        let env = (t * 0.35).sin().abs() * 0.4 + 0.6;
        let mut sample = 0.0f32;
        for (k, mul) in [1.0f32, 1.25, 1.5, 2.0].iter().enumerate() {
            let freq = root * mul;
            let idx = k % 3;
            phase[idx] += 2.0 * std::f32::consts::PI * freq / sample_rate as f32;
            sample += phase[idx].sin() * (0.22 / (k as f32 + 1.0));
        }
        // gentle pulse
        let pulse = ((t * 1.4).sin() * 0.5 + 0.5) * 0.08;
        sample += (phase[0] * 2.0).sin() * pulse;
        let fade_in = (t / 2.0).min(1.0);
        let fade_out = ((seconds as f32 - t) / 3.0).min(1.0).max(0.0);
        sample = (sample * env * fade_in * fade_out * 0.75).clamp(-1.0, 1.0);
        let v = (sample * 32767.0) as i16;
        file.write_all(&v.to_le_bytes())?;
    }
    Ok(())
}

fn generate_art(path: &Path, accent: &str, seed: u32) -> anyhow::Result<()> {
    use image::{ImageBuffer, Rgba};
    let (r, g, b) = crate::model::accent_rgb(accent);
    let w = 512u32;
    let h = 512u32;
    let mut img = ImageBuffer::from_pixel(w, h, Rgba([r, g, b, 255]));
    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let wave = ((fx * 8.0 + seed as f32).sin() * 0.5 + 0.5) * 0.25;
            let d = ((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt();
            let glow = (1.0 - d * 1.6).max(0.0) * 0.5;
            let rr = (r as f32 * (0.35 + wave + glow) + 30.0).min(255.0) as u8;
            let gg = (g as f32 * (0.30 + wave * 0.7 + glow * 0.6) + 18.0).min(255.0) as u8;
            let bb = (b as f32 * (0.45 + wave * 0.9 + glow * 0.5) + 26.0).min(255.0) as u8;
            img.put_pixel(x, y, Rgba([rr, gg, bb, 255]));
        }
    }
    img.save(path)?;
    Ok(())
}

pub fn find_lrc_for(track: &Track) -> Option<PathBuf> {
    let path = track.local_path.as_ref()?;
    let p = Path::new(path);
    let lrc = p.with_extension("lrc");
    if lrc.exists() {
        return Some(lrc);
    }
    None
}
