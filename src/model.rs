use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const APP_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Youtube,
    Soundcloud,
    Local,
    Demo,
    Spotify,
}

impl Source {
    pub fn label(&self) -> &'static str {
        match self {
            Source::Youtube => "YouTube",
            Source::Soundcloud => "SoundCloud",
            Source::Local => "Local",
            Source::Demo => "Rift",
            Source::Spotify => "Spotify",
        }
    }
    pub fn prefix(&self) -> &'static str {
        match self {
            Source::Youtube => "yt",
            Source::Soundcloud => "sc",
            Source::Local => "local",
            Source::Demo => "demo",
            Source::Spotify => "spotify",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration: u64,
    #[serde(default)]
    pub artwork: Option<String>,
    #[serde(default)]
    pub artwork_path: Option<String>,
    pub source: Source,
    #[serde(default)]
    pub stream_url: Option<String>,
    #[serde(default)]
    pub local_path: Option<String>,
    #[serde(default)]
    pub liked: bool,
    #[serde(default)]
    pub added_at: i64,
    #[serde(default)]
    pub play_count: u64,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub hls: bool,
}

impl Track {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        artist: impl Into<String>,
        source: Source,
    ) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            duration: 0,
            artwork: None,
            artwork_path: None,
            source,
            stream_url: None,
            local_path: None,
            liked: false,
            added_at: now(),
            play_count: 0,
            accent: accent_for(&id),
            hls: false,
        }
    }

    pub fn duration_label(&self) -> String {
        format_time(self.duration)
    }

    pub fn subtitle(&self) -> String {
        if self.album.is_empty() {
            self.source.label().to_string()
        } else {
            format!("{} · {}", self.album, self.source.label())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub track_ids: Vec<String>,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub imported_from: String,
}

impl Playlist {
    pub fn new(name: String) -> Self {
        let id = format!("pl:{}", now_millis());
        Self {
            id: id.clone(),
            name,
            subtitle: "Your playlist".into(),
            track_ids: Vec::new(),
            accent: accent_for(&id),
            created_at: now(),
            imported_from: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub track_id: String,
    pub played_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EqProfile {
    pub name: String,
    pub gains: [f32; 10],
    pub bass: f32,
    pub reverb: f32,
    pub speed: f32,
    pub pitch: f32,
    pub compressor: bool,
    pub comp_threshold: f32,
    pub comp_ratio: f32,
    pub comp_attack: f32,
    pub comp_release: f32,
    pub comp_makeup: f32,
    pub limiter: bool,
    pub limiter_ceiling: f32,
    pub normalizer: bool,
    pub gate: bool,
    pub gate_threshold: f32,
    pub gate_release: f32,
    pub mbc: bool,
    pub mbc_low_threshold: f32,
    pub mbc_low_ratio: f32,
    pub mbc_mid_threshold: f32,
    pub mbc_mid_ratio: f32,
    pub mbc_high_threshold: f32,
    pub mbc_high_ratio: f32,
    pub width: f32,
}

impl Default for EqProfile {
    fn default() -> Self {
        Self {
            name: "Profile".into(),
            gains: [0.0; 10],
            bass: 0.0,
            reverb: 0.0,
            speed: 1.0,
            pitch: 0.0,
            compressor: false,
            comp_threshold: -18.0,
            comp_ratio: 3.0,
            comp_attack: 12.0,
            comp_release: 120.0,
            comp_makeup: 2.0,
            limiter: true,
            limiter_ceiling: -1.0,
            normalizer: false,
            gate: false,
            gate_threshold: -55.0,
            gate_release: 160.0,
            mbc: false,
            mbc_low_threshold: -20.0,
            mbc_low_ratio: 2.5,
            mbc_mid_threshold: -18.0,
            mbc_mid_ratio: 2.0,
            mbc_high_threshold: -16.0,
            mbc_high_ratio: 2.0,
            width: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme_light: bool,
    pub dynamic: bool,
    pub accent_index: usize,
    pub font_scale: f32,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: i32,
    pub normalizer: bool,
    pub visualizer: bool,
    pub audio_quality: String,
    pub download_quality: String,
    pub account_name: String,
    pub eq_gains: [f32; 10],
    pub speed: f32,
    pub pitch: f32,
    pub bass: f32,
    pub reverb: f32,
    pub search_history: Vec<String>,
    pub visualizer_on: bool,
    pub sleep_minutes: i32,
    pub eq_preset: i32,
    pub compressor: bool,
    pub comp_threshold: f32,
    pub comp_ratio: f32,
    pub comp_attack: f32,
    pub comp_release: f32,
    pub comp_makeup: f32,
    pub limiter: bool,
    pub limiter_ceiling: f32,
    pub gate: bool,
    pub gate_threshold: f32,
    pub gate_release: f32,
    pub mbc: bool,
    pub mbc_low_threshold: f32,
    pub mbc_low_ratio: f32,
    pub mbc_mid_threshold: f32,
    pub mbc_mid_ratio: f32,
    pub mbc_high_threshold: f32,
    pub mbc_high_ratio: f32,
    pub width: f32,
    pub profiles: Vec<EqProfile>,
    pub spotify_client_id: String,
    pub spotify_client_secret: String,
    pub spotify_access_token: String,
    pub spotify_token_expires: i64,
    pub theme_index: usize,
    pub amoled: bool,
    pub headphone_index: i32,
    pub headphone_profile: String,
    pub visualizer_mode: String,
    pub eq_tab: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme_light: false,
            dynamic: false,
            accent_index: 0,
            font_scale: 1.0,
            volume: 0.8,
            shuffle: false,
            repeat: 0,
            normalizer: false,
            visualizer: true,
            audio_quality: "High".into(),
            download_quality: "Best".into(),
            account_name: "Listener".into(),
            eq_gains: [0.0; 10],
            speed: 1.0,
            pitch: 0.0,
            bass: 0.0,
            reverb: 0.0,
            search_history: Vec::new(),
            visualizer_on: true,
            sleep_minutes: 0,
            eq_preset: 0,
            compressor: false,
            comp_threshold: -18.0,
            comp_ratio: 3.0,
            comp_attack: 12.0,
            comp_release: 120.0,
            comp_makeup: 2.0,
            limiter: true,
            limiter_ceiling: -1.0,
            gate: false,
            gate_threshold: -55.0,
            gate_release: 160.0,
            mbc: false,
            mbc_low_threshold: -20.0,
            mbc_low_ratio: 2.5,
            mbc_mid_threshold: -18.0,
            mbc_mid_ratio: 2.0,
            mbc_high_threshold: -16.0,
            mbc_high_ratio: 2.0,
            width: 1.0,
            profiles: Vec::new(),
            spotify_client_id: String::new(),
            spotify_client_secret: String::new(),
            spotify_access_token: String::new(),
            spotify_token_expires: 0,
            theme_index: 0,
            amoled: true,
            headphone_index: 0,
            headphone_profile: "Flat / reference".into(),
            visualizer_mode: "Bars".into(),
            eq_tab: "Equalizer".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LibraryFile {
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub playlists: Vec<Playlist>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

#[derive(Debug, Clone)]
pub struct Core {
    pub tracks: HashMap<String, Track>,
    pub playlists: Vec<Playlist>,
    pub history: Vec<HistoryEntry>,
    pub settings: Settings,
    pub queue: Vec<String>,
    pub queue_index: Option<usize>,
    pub current: Option<String>,
    pub view: String,
    pub view_stack: Vec<String>,
    pub page_title: String,
    pub page_subtitle: String,
    pub page_id: String,
    pub page_accent: String,
    pub detail_ids: Vec<String>,
    pub liked_ids: Vec<String>,
    pub local_ids: Vec<String>,
    pub home_recent: Vec<String>,
    pub home_made: Vec<String>,
    pub home_trending: Vec<String>,
    pub home_quick: Vec<String>,
    pub search_results: Vec<String>,
    pub search_source: String,
    pub search_query: String,
    pub search_loading: bool,
    pub lyrics: Vec<LyricLineData>,
    pub lyrics_offset: i64,
    pub lyrics_track: Option<String>,
    pub loading_home: bool,
    pub offline: bool,
    pub scan_busy: bool,
    pub import_busy: bool,
    pub import_progress: i32,
    pub import_status: String,
    pub import_url: String,
    pub import_source: String,
    pub account_name: String,
    pub cache_size: String,
    pub toast_until: i64,
    pub toast_text: String,
    pub pending_seek: Option<i64>,
    pub sleep_until: Option<i64>,
    pub last_ended_handled: bool,
    pub queued_track_ids: Vec<String>,
}

impl Core {
    pub fn new(settings: Settings) -> Self {
        Self {
            tracks: HashMap::new(),
            playlists: Vec::new(),
            history: Vec::new(),
            settings,
            queue: Vec::new(),
            queue_index: None,
            current: None,
            view: "home".into(),
            view_stack: Vec::new(),
            page_title: String::new(),
            page_subtitle: String::new(),
            page_id: String::new(),
            page_accent: "#8b7bff".into(),
            detail_ids: Vec::new(),
            liked_ids: Vec::new(),
            local_ids: Vec::new(),
            home_recent: Vec::new(),
            home_made: Vec::new(),
            home_trending: Vec::new(),
            home_quick: Vec::new(),
            search_results: Vec::new(),
            search_source: "All".into(),
            search_query: String::new(),
            search_loading: false,
            lyrics: Vec::new(),
            lyrics_offset: 0,
            lyrics_track: None,
            loading_home: false,
            offline: false,
            scan_busy: false,
            import_busy: false,
            import_progress: 0,
            import_status: String::new(),
            import_url: String::new(),
            import_source: "YouTube".into(),
            account_name: "Listener".into(),
            cache_size: "0 MB".into(),
            toast_until: 0,
            toast_text: String::new(),
            pending_seek: None,
            sleep_until: None,
            last_ended_handled: false,
            queued_track_ids: Vec::new(),
        }
    }

    pub fn add_track(&mut self, mut track: Track) -> String {
        if let Some(existing) = self.tracks.get(&track.id) {
            let liked = existing.liked;
            track.liked = liked;
            if track.artwork_path.is_none() {
                track.artwork_path = existing.artwork_path.clone();
            }
        }
        let id = track.id.clone();
        self.tracks.insert(id.clone(), track);
        id
    }

    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.get(id)
    }

    pub fn liked_tracks(&self) -> Vec<Track> {
        let mut v: Vec<Track> = self
            .liked_ids
            .iter()
            .filter_map(|id| self.tracks.get(id).cloned())
            .collect();
        v.reverse();
        v
    }

    pub fn local_tracks(&self) -> Vec<Track> {
        self.local_ids
            .iter()
            .filter_map(|id| self.tracks.get(id).cloned())
            .collect()
    }

    pub fn queue_tracks(&self) -> Vec<Track> {
        self.queue
            .iter()
            .filter_map(|id| self.tracks.get(id).cloned())
            .collect()
    }

    pub fn history_tracks(&self) -> Vec<Track> {
        let mut seen = std::collections::HashSet::new();
        self.history
            .iter()
            .rev()
            .filter(|h| seen.insert(h.track_id.clone()))
            .filter_map(|h| self.tracks.get(&h.track_id).cloned())
            .collect()
    }

    pub fn albums(&self) -> Vec<(String, Vec<Track>)> {
        let mut map: HashMap<String, Vec<Track>> = HashMap::new();
        for t in self
            .liked_tracks()
            .into_iter()
            .chain(self.local_tracks().into_iter())
        {
            let key = if t.album.is_empty() {
                t.artist.clone()
            } else {
                format!("{} — {}", t.artist, t.album)
            };
            map.entry(key).or_default().push(t);
        }
        let mut out: Vec<(String, Vec<Track>)> = map.into_iter().collect();
        out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        out
    }

    pub fn playlist(&self, id: &str) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    pub fn set_view(&mut self, view: &str) {
        if self.view != view && !self.view.is_empty() {
            self.view_stack.push(self.view.clone());
            if self.view_stack.len() > 30 {
                self.view_stack.remove(0);
            }
        }
        self.view = view.to_string();
    }

    pub fn go_back(&mut self) {
        if let Some(prev) = self.view_stack.pop() {
            self.view = prev;
        } else {
            self.view = "home".into();
        }
    }

    pub fn push_history(&mut self, id: &str) {
        self.history.push(HistoryEntry {
            track_id: id.to_string(),
            played_at: now(),
        });
        if self.history.len() > 500 {
            self.history.remove(0);
        }
        if let Some(t) = self.tracks.get_mut(id) {
            t.play_count += 1;
        }
    }

    pub fn like_track(&mut self, id: &str) {
        if let Some(t) = self.tracks.get_mut(id) {
            t.liked = !t.liked;
            if t.liked {
                if !self.liked_ids.contains(&id.to_string()) {
                    self.liked_ids.push(id.to_string());
                }
            } else {
                self.liked_ids.retain(|x| x != id);
            }
        }
    }

    pub fn lyrics_active(&self) -> Option<usize> {
        let pos = self.pending_seek.unwrap_or(0) as i64;
        let pos = pos + self.lyrics_offset;
        let mut idx = None;
        for (i, line) in self.lyrics.iter().enumerate() {
            if line.time <= pos {
                idx = Some(i);
            } else {
                break;
            }
        }
        idx
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast_text = msg.into();
        self.toast_until = now_millis() + 2600;
    }

    pub fn save_settings(&self, paths: &crate::paths::Paths) {
        let _ = storage_save_json(&paths.settings_file, &self.settings);
    }

    pub fn save_library(&self, paths: &crate::paths::Paths) {
        let file = LibraryFile {
            tracks: self.tracks.values().cloned().collect(),
            playlists: self.playlists.clone(),
            history: self.history.clone(),
        };
        let _ = storage_save_json(&paths.library_file, &file);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricLineData {
    pub time: i64,
    pub text: String,
}

pub fn storage_save_json<T: Serialize>(path: &std::path::Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    let data = serde_json::to_vec_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&tmp, data)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

pub fn storage_load_json<T: for<'de> Deserialize<'de>>(path: &std::path::Path) -> Option<T> {
    let data = std::fs::read(path).ok()?;
    serde_json::from_slice(&data).ok()
}

pub fn format_time(seconds: u64) -> String {
    let m = seconds / 60;
    let s = seconds % 60;
    format!("{}:{:02}", m, s)
}

pub fn format_ms(ms: u64) -> String {
    format_time(ms / 1000)
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn hash_id(input: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

pub fn accent_for(seed: &str) -> String {
    const ACCENTS: [&str; 10] = [
        "#1db954", "#1ed760", "#8b7bff", "#ff6b9d", "#ff8a5c", "#ffd166", "#48d597", "#4fd1c5",
        "#54a9ff", "#b47cff",
    ];
    let h = hash_id(seed);
    let n = u64::from_str_radix(&h[..h.len().min(8)], 16).unwrap_or(0);
    ACCENTS[(n as usize) % ACCENTS.len()].to_string()
}

pub fn accent_rgb(hex: &str) -> (u8, u8, u8) {
    let s = hex.trim_start_matches('#');
    if s.len() >= 6 {
        (
            u8::from_str_radix(&s[0..2], 16).unwrap_or(139),
            u8::from_str_radix(&s[2..4], 16).unwrap_or(123),
            u8::from_str_radix(&s[4..6], 16).unwrap_or(255),
        )
    } else {
        (139, 123, 255)
    }
}
