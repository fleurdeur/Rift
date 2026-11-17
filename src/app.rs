use crate::model::*;
use crate::paths;
use crate::player::{Cmd as PlayerCmd, Dynamics, PlayerHandle};
use crate::sources;
use crate::{
    android, audio_presets, images, library, lyrics, themes, AlbumItem, App, AppWindow,
    LyricLine, PlaylistItem, Theme, ThemeItem, TrackItem,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;


pub struct AppState {
    pub core: Arc<Mutex<Core>>,
    pub player: PlayerHandle,
    pub spectrum: Vec<f32>,
    pub lyrics_ui: Mutex<Option<(String, usize, usize)>>, // track, line count, active
    pub home_loading: Arc<Mutex<bool>>,
    pub theme_applied: Mutex<Option<(usize, bool)>>,
    pub lyrics_source: Mutex<String>,
}

impl AppState {
    pub fn new(core: Core) -> Arc<Self> {
        Arc::new(Self {
            core: Arc::new(Mutex::new(core)),
            player: crate::player::spawn(),
            spectrum: vec![0.0; 32],
            lyrics_ui: Mutex::new(None),
            home_loading: Arc::new(Mutex::new(false)),
            theme_applied: Mutex::new(None),
            lyrics_source: Mutex::new(String::new()),
        })
    }
}

pub fn run() -> anyhow::Result<()> {
    paths::init_root();
    let settings: Settings =
        storage_load_json(&paths::paths().settings_file).unwrap_or_default();
    let mut core = Core::new(settings.clone());
    core.account_name = settings.account_name.clone();
    if let Some(file) = storage_load_json::<LibraryFile>(&paths::paths().library_file) {
        let LibraryFile {
            tracks,
            playlists,
            history,
        } = file;
        core.playlists = playlists;
        core.history = history;
        for t in tracks {
            if t.liked {
                core.liked_ids.push(t.id.clone());
            }
            if t.source == Source::Local {
                core.local_ids.push(t.id.clone());
            }
            core.tracks.insert(t.id.clone(), t);
        }
    }
    let state = AppState::new(core);
    {
        let c = state.core.lock().unwrap();
        state.player.send(PlayerCmd::Volume(c.settings.volume));
        state.player.send(PlayerCmd::Eq(c.settings.eq_gains));
        state.player.send(PlayerCmd::Bass(c.settings.bass));
        state.player.send(PlayerCmd::Reverb(c.settings.reverb));
        state.player.send(PlayerCmd::Normalizer(c.settings.normalizer));
        state.player.send(PlayerCmd::Speed(
            c.settings.speed * 2f32.powf(c.settings.pitch / 12.0),
        ));
        send_dynamics(&c, &state.player);
    }
    library::create_demo_tracks(&mut state.core.lock().unwrap());
    #[cfg(not(target_os = "android"))]
    if let Ok(view) = std::env::var("RIFF_VIEW") {
        if !view.is_empty() {
            state.core.lock().unwrap().set_view(&view);
        }
    }
    {
        let mut c = state.core.lock().unwrap();
        if let Some(id) = c
            .history
            .last()
            .map(|h| h.track_id.clone())
            .filter(|id| c.tracks.contains_key(id))
            .or_else(|| c.liked_ids.first().cloned())
            .or_else(|| c.local_ids.first().cloned())
        {
            c.current = Some(id);
        }
        c.cache_size = paths::cache_size_label();
        c.save_settings(paths::paths());
        c.save_library(paths::paths());
    }

    let ui = AppWindow::new()?;
    {
        let c = state.core.lock().unwrap();
        apply_theme(&ui, &c.settings);
    }
    setup_callbacks(&ui, state.clone());
    refresh(&ui, &state);
    start_timer(ui.as_weak(), state.clone());

    #[cfg(target_os = "android")]
    android::request_media_permission();

    fetch_home(ui.as_weak(), state.clone());
    ui.run()?;
    state.player.send(PlayerCmd::Shutdown);
    Ok(())
}

fn setup_callbacks(ui: &AppWindow, state: Arc<AppState>) {
    let app = ui.global::<App>();

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_navigate(move |view| {
            let view = view.to_string();
            {
                let mut c = state.core.lock().unwrap();
                match view.as_str() {
                    "liked" => {
                        c.detail_ids = c.liked_ids.iter().rev().cloned().collect();
                        c.page_title = "Liked songs".into();
                        c.page_subtitle = format!("{} tracks you love", c.liked_ids.len());
                    }
                    "local" => {
                        c.detail_ids = c.local_ids.clone();
                        c.page_title = "My files".into();
                        c.page_subtitle = format!("{} local tracks", c.local_ids.len());
                    }
                    "history" => {
                        c.detail_ids = c.history_tracks().iter().map(|t| t.id.clone()).collect();
                        c.page_title = "History".into();
                        c.page_subtitle = "Recently played".into();
                    }
                    "albums" => {
                        c.detail_ids = c
                            .liked_tracks()
                            .into_iter()
                            .chain(c.local_tracks())
                            .map(|t| t.id)
                            .collect();
                        c.page_title = "Albums".into();
                    }
                    "queue" => {
                        c.page_title = "Queue".into();
                        c.page_subtitle = format!("{} tracks up next", c.queue.len());
                    }
                    _ => {}
                }
                c.set_view(&view);
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_go_back(move || {
            {
                let mut c = state.core.lock().unwrap();
                c.go_back();
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_play_id(move |id| {
            let id = id.to_string();
            play_track(&state, &weak, &id);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_play_page(move || {
            let (ids, first) = {
                let c = state.core.lock().unwrap();
                let ids = page_ids(&c);
                (ids.clone(), ids.first().cloned())
            };
            if let Some(first) = first {
                {
                    let mut c = state.core.lock().unwrap();
                    c.queue = ids;
                    c.queue_index = Some(0);
                }
                play_track(&state, &weak, &first);
            } else {
                toast(&weak, &state, "Nothing to play here yet");
            }
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_shuffle_page(move || {
            let ids = {
                let c = state.core.lock().unwrap();
                let mut ids = page_ids(&c);
                use rand::seq::SliceRandom;
                ids.shuffle(&mut rand::rng());
                ids
            };
            if let Some(first) = ids.first().cloned() {
                {
                    let mut c = state.core.lock().unwrap();
                    c.queue = ids;
                    c.queue_index = Some(0);
                    c.settings.shuffle = true;
                }
                play_track(&state, &weak, &first);
            }
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_toggle_play(move || {
            let has = state.core.lock().unwrap().current.is_some();
            if !has {
                let first = {
                    let c = state.core.lock().unwrap();
                    page_ids(&c).first().cloned()
                };
                if let Some(id) = first {
                    play_track(&state, &weak, &id);
                }
                return;
            }
            state.player.send(PlayerCmd::Toggle);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_next_track(move || {
            next_track(&state, &weak);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_previous_track(move || {
            previous_track(&state, &weak);
        });
    }

    {
        let state = state.clone();
        app.on_seek(move |ms| {
            state.player.send(PlayerCmd::Seek(ms.max(0) as u64));
            if let Ok(mut c) = state.core.lock() {
                c.pending_seek = Some(ms.max(0) as i64);
            }
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_volume(move |v| {
            let v = v.clamp(0.0, 1.0);
            state.player.send(PlayerCmd::Volume(v));
            {
                let mut c = state.core.lock().unwrap();
                c.settings.volume = v;
                c.save_settings(paths::paths());
            }
            let _ = weak;
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_toggle_like(move |id: slint::SharedString| {
            {
                let mut c = state.core.lock().unwrap();
                c.like_track(&id);
                c.save_library(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_queue_add(move |id| {
            {
                let mut c = state.core.lock().unwrap();
                let id = id.to_string();
                if !c.queue.contains(&id) {
                    c.queue.push(id);
                }
                c.toast("Added to queue");
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_play_queue_id(move |id| {
            let id = id.to_string();
            {
                let mut c = state.core.lock().unwrap();
                if let Some(i) = c.queue.iter().position(|x| *x == id) {
                    c.queue_index = Some(i);
                }
            }
            play_track(&state, &weak, &id);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_remove_queue_id(move |id| {
            {
                let mut c = state.core.lock().unwrap();
                let id = id.to_string();
                c.queue.retain(|x| *x != id);
                if c.queue.is_empty() {
                    c.queue_index = None;
                }
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_clear_queue(move || {
            {
                let mut c = state.core.lock().unwrap();
                c.queue.clear();
                c.queue_index = None;
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_search(move |q| {
            let q = q.trim().to_string();
            if q.is_empty() {
                return;
            }
            let source = {
                let mut c = state.core.lock().unwrap();
                c.search_results.clear();
                c.search_query = q.clone();
                c.search_loading = true;
                c.settings.search_history.retain(|x| x != &q);
                c.settings.search_history.insert(0, q.clone());
                c.settings.search_history.truncate(8);
                c.save_settings(paths::paths());
                c.search_source.clone()
            };
            refresh_weak(&weak, &state);
            run_search(state.clone(), weak.clone(), q, source);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_search_source(move |s| {
            {
                let mut c = state.core.lock().unwrap();
                c.search_source = s.to_string();
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_open_playlist(move |id| {
            {
                let mut c = state.core.lock().unwrap();
                let id = id.to_string();
                if let Some(p) = c.playlist(&id).cloned() {
                    c.detail_ids = p.track_ids.clone();
                    c.page_title = p.name.clone();
                    c.page_subtitle = p.subtitle.clone();
                    c.page_id = p.id.clone();
                    c.page_accent = p.accent.clone();
                    c.page_id = p.track_ids.first().cloned().unwrap_or_else(|| p.id.clone());
                    c.set_view("playlist");
                }
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_open_album(move |name| {
            {
                let mut c = state.core.lock().unwrap();
                let name = name.to_string();
                let mut ids = Vec::new();
                for (key, tracks) in c.albums() {
                    if key == name {
                        ids = tracks.iter().map(|t| t.id.clone()).collect();
                    }
                }
                if ids.is_empty() {
                    for t in c.liked_tracks().into_iter().chain(c.local_tracks()) {
                        if t.album == name {
                            ids.push(t.id.clone());
                        }
                    }
                }
                c.detail_ids = ids;
                c.page_title = name.clone();
                c.page_subtitle = "Album".into();
                c.page_id = name;
                if let Some(id) = c.detail_ids.first().cloned() {
                    if let Some(t) = c.tracks.get(&id) {
                        c.page_accent = t.accent.clone();
                    }
                }
                c.set_view("album");
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_open_artist(move |name| {
            {
                let mut c = state.core.lock().unwrap();
                let name = name.to_string();
                let ids: Vec<String> = c
                    .liked_tracks()
                    .into_iter()
                    .chain(c.local_tracks())
                    .filter(|t| t.artist == name)
                    .map(|t| t.id)
                    .collect();
                c.detail_ids = ids;
                c.page_title = name.clone();
                c.page_subtitle = "Artist".into();
                c.page_id = name.clone();
                c.set_view("artist");
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_create_playlist(move || {
            let id = {
                let mut c = state.core.lock().unwrap();
                let n = c.playlists.len() + 1;
                let p = Playlist::new(format!("New playlist {n}"));
                let id = p.id.clone();
                c.playlists.insert(0, p);
                c.save_library(paths::paths());
                id
            };
            {
                let mut c = state.core.lock().unwrap();
                if let Some(p) = c.playlist(&id).cloned() {
                    c.detail_ids = p.track_ids.clone();
                    c.page_title = p.name.clone();
                    c.page_subtitle = "Your playlist".into();
                    c.page_id = p.id.clone();
                    c.page_accent = p.accent.clone();
                }
                c.set_view("playlist");
            }
            refresh_weak(&weak, &state);
            toast(&weak, &state, "Playlist created");
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_delete_playlist(move |id| {
            {
                let mut c = state.core.lock().unwrap();
                c.playlists.retain(|p| p.id != id.as_str());
                c.save_library(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_add_to_playlist(move |id| {
            {
                let mut c = state.core.lock().unwrap();
                let id = id.to_string();
                if let Some(p) = c.playlists.first_mut() {
                    if !p.track_ids.contains(&id) {
                        p.track_ids.push(id);
                        p.subtitle = format!("{} tracks", p.track_ids.len());
                    }
                } else {
                    let mut p = Playlist::new("My Rift Mix".into());
                    p.track_ids.push(id);
                    p.subtitle = "1 track".into();
                    c.playlists.insert(0, p);
                }
                c.save_library(paths::paths());
                c.toast("Added to playlist");
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_scan_library(move || {
            {
                let mut c = state.core.lock().unwrap();
                c.scan_busy = true;
                c.toast("Scanning for local music…");
            }
            refresh_weak(&weak, &state);
            let state2 = state.clone();
            let weak2 = weak.clone();
            std::thread::spawn(move || {
                {
                    let mut c = state2.core.lock().unwrap();
                    library::scan_all(&mut c);
                    c.scan_busy = false;
                    let n = c.local_ids.len();
                    c.toast(format!("Library updated · {n} local tracks"));
                }
                refresh_weak(&weak2, &state2);
            });
        });
    }

    {
        let state = state.clone();
        app.on_set_import_url(move |url| {
            state.core.lock().unwrap().import_url = url.to_string();
        });
    }

    {
        let state = state.clone();
        app.on_set_import_source(move |s| {
            state.core.lock().unwrap().import_source = s.to_string();
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_import_playlist(move || {
            let (url, busy) = {
                let c = state.core.lock().unwrap();
                (c.import_url.clone(), c.import_busy)
            };
            if busy {
                return;
            }
            if url.trim().is_empty() {
                toast(&weak, &state, "Paste a playlist link first");
                return;
            }
            {
                let mut c = state.core.lock().unwrap();
                c.import_busy = true;
                c.import_progress = 4;
                c.import_status = "Reading playlist…".into();
            }
            refresh_weak(&weak, &state);
            let state2 = state.clone();
            let weak2 = weak.clone();
            std::thread::spawn(move || {
                let result = import_playlist(&state2, &url);
                let mut c = state2.core.lock().unwrap();
                c.import_busy = false;
                c.import_progress = 100;
                match result {
                    Ok((pname, count, pid)) => {
                        c.import_status = format!("Imported {count} tracks from {pname}");
                        c.toast(format!("Imported {pname}"));
                        if let Some(p) = c.playlist(&pid).cloned() {
                            c.detail_ids = p.track_ids.clone();
                            c.page_title = p.name.clone();
                            c.page_subtitle = p.subtitle.clone();
                            c.page_id = p.id.clone();
                            c.page_accent = p.accent.clone();
                            c.set_view("playlist");
                        }
                    }
                    Err(e) => {
                        c.import_status = format!("Import failed: {e}");
                        c.toast(format!("Import failed: {e}"));
                    }
                }
                c.save_library(paths::paths());
                drop(c);
                refresh_weak(&weak2, &state2);
            });
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_eq(move |i, v| {
            {
                let mut c = state.core.lock().unwrap();
                if let Some(g) = c.settings.eq_gains.get_mut(i as usize) {
                    *g = v.clamp(-12.0, 12.0);
                }
                let gains = c.settings.eq_gains;
                state.player.send(PlayerCmd::Eq(gains));
                c.settings.eq_preset = 0;
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_apply_preset(move |idx| {
            apply_preset(&state, idx as usize);
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        app.on_set_speed(move |v| {
            state.player.send(PlayerCmd::Speed(v));
            {
                let mut c = state.core.lock().unwrap();
                c.settings.speed = v;
                c.save_settings(paths::paths());
            }
        });
    }

    {
        let state = state.clone();
        app.on_set_pitch(move |v| {
            let effective = {
                let mut c = state.core.lock().unwrap();
                c.settings.pitch = v;
                let eff = c.settings.speed * 2f32.powf(v / 12.0);
                c.save_settings(paths::paths());
                eff
            };
            state.player.send(PlayerCmd::Speed(effective));
        });
    }

    {
        let state = state.clone();
        app.on_set_bass(move |v| {
            state.player.send(PlayerCmd::Bass(v));
            {
                let mut c = state.core.lock().unwrap();
                c.settings.bass = v;
                c.save_settings(paths::paths());
            }
        });
    }

    {
        let state = state.clone();
        app.on_set_reverb(move |v| {
            state.player.send(PlayerCmd::Reverb(v));
            {
                let mut c = state.core.lock().unwrap();
                c.settings.reverb = v;
                c.save_settings(paths::paths());
            }
        });
    }

    {
        let state = state.clone();
        app.on_toggle_normalizer(move || {
            let n = {
                let mut c = state.core.lock().unwrap();
                c.settings.normalizer = !c.settings.normalizer;
                c.save_settings(paths::paths());
                c.settings.normalizer
            };
            state.player.send(PlayerCmd::Normalizer(n));
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_compressor(move |on| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.compressor = on;
                c.save_settings(paths::paths());
                send_dynamics(&c, &state.player);
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_comp_threshold(move |v| {
            update_dyn(&state, |s| s.comp_threshold = v.clamp(-48.0, 0.0));
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_comp_ratio(move |v| {
            update_dyn(&state, |s| s.comp_ratio = v.clamp(1.0, 20.0));
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_comp_attack(move |v| {
            update_dyn(&state, |s| s.comp_attack = v.clamp(1.0, 200.0));
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_comp_release(move |v| {
            update_dyn(&state, |s| s.comp_release = v.clamp(20.0, 1000.0));
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_comp_makeup(move |v| {
            update_dyn(&state, |s| s.comp_makeup = v.clamp(0.0, 24.0));
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_limiter(move |on| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.limiter = on;
                c.save_settings(paths::paths());
                send_dynamics(&c, &state.player);
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_limiter_ceiling(move |v| {
            update_dyn(&state, |s| s.limiter_ceiling = v.clamp(-24.0, 0.0));
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_save_profile(move || {
            let name = {
                let mut c = state.core.lock().unwrap();
                let n = c.settings.profiles.len() + 1;
                let name = format!("My preset {n}");
                let p = EqProfile {
                    name: name.clone(),
                    gains: c.settings.eq_gains,
                    bass: c.settings.bass,
                    reverb: c.settings.reverb,
                    speed: c.settings.speed,
                    pitch: c.settings.pitch,
                    compressor: c.settings.compressor,
                    comp_threshold: c.settings.comp_threshold,
                    comp_ratio: c.settings.comp_ratio,
                    comp_attack: c.settings.comp_attack,
                    comp_release: c.settings.comp_release,
                    comp_makeup: c.settings.comp_makeup,
                    limiter: c.settings.limiter,
                    limiter_ceiling: c.settings.limiter_ceiling,
                    normalizer: c.settings.normalizer,
                    gate: c.settings.gate,
                    gate_threshold: c.settings.gate_threshold,
                    gate_release: c.settings.gate_release,
                    mbc: c.settings.mbc,
                    mbc_low_threshold: c.settings.mbc_low_threshold,
                    mbc_low_ratio: c.settings.mbc_low_ratio,
                    mbc_mid_threshold: c.settings.mbc_mid_threshold,
                    mbc_mid_ratio: c.settings.mbc_mid_ratio,
                    mbc_high_threshold: c.settings.mbc_high_threshold,
                    mbc_high_ratio: c.settings.mbc_high_ratio,
                    width: c.settings.width,
                };
                c.settings.profiles.push(p);
                if c.settings.profiles.len() > 8 {
                    c.settings.profiles.remove(0);
                }
                c.save_settings(paths::paths());
                name
            };
            toast(&weak, &state, &format!("Saved {name}"));
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_apply_profile(move |idx| {
            {
                let mut c = state.core.lock().unwrap();
                let Some(p) = c.settings.profiles.get(idx.max(0) as usize).cloned() else { return };
                c.settings.eq_gains = p.gains;
                c.settings.bass = p.bass;
                c.settings.reverb = p.reverb;
                c.settings.speed = p.speed;
                c.settings.pitch = p.pitch;
                c.settings.compressor = p.compressor;
                c.settings.comp_threshold = p.comp_threshold;
                c.settings.comp_ratio = p.comp_ratio;
                c.settings.comp_attack = p.comp_attack;
                c.settings.comp_release = p.comp_release;
                c.settings.comp_makeup = p.comp_makeup;
                c.settings.limiter = p.limiter;
                c.settings.limiter_ceiling = p.limiter_ceiling;
                c.settings.normalizer = p.normalizer;
                c.settings.gate = p.gate;
                c.settings.gate_threshold = p.gate_threshold;
                c.settings.gate_release = p.gate_release;
                c.settings.mbc = p.mbc;
                c.settings.mbc_low_threshold = p.mbc_low_threshold;
                c.settings.mbc_low_ratio = p.mbc_low_ratio;
                c.settings.mbc_mid_threshold = p.mbc_mid_threshold;
                c.settings.mbc_mid_ratio = p.mbc_mid_ratio;
                c.settings.mbc_high_threshold = p.mbc_high_threshold;
                c.settings.mbc_high_ratio = p.mbc_high_ratio;
                c.settings.width = p.width;
                let eff = p.speed * 2f32.powf(p.pitch / 12.0);
                state.player.send(PlayerCmd::Eq(p.gains));
                state.player.send(PlayerCmd::Speed(eff));
                state.player.send(PlayerCmd::Bass(p.bass));
                state.player.send(PlayerCmd::Reverb(p.reverb));
                state.player.send(PlayerCmd::Normalizer(p.normalizer));
                send_dynamics(&c, &state.player);
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_spotify_client_id(move |v| {
            let mut c = state.core.lock().unwrap();
            c.settings.spotify_client_id = v.to_string();
            c.settings.spotify_access_token.clear();
            c.settings.spotify_token_expires = 0;
            c.save_settings(paths::paths());
            drop(c);
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_spotify_client_secret(move |v| {
            let mut c = state.core.lock().unwrap();
            c.settings.spotify_client_secret = v.to_string();
            c.settings.spotify_access_token.clear();
            c.settings.spotify_token_expires = 0;
            c.save_settings(paths::paths());
            drop(c);
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_connect_spotify(move || {
            let (id, secret) = {
                let c = state.core.lock().unwrap();
                (c.settings.spotify_client_id.trim().to_string(), c.settings.spotify_client_secret.trim().to_string())
            };
            if id.is_empty() || secret.is_empty() {
                toast(&weak, &state, "Enter your Spotify Client ID and Secret");
                return;
            }
            let state2 = state.clone();
            let weak2 = weak.clone();
            std::thread::spawn(move || {
                match sources::spotify::client_token(&id, &secret) {
                    Ok((token, exp)) => {
                        {
                            let mut c = state2.core.lock().unwrap();
                            c.settings.spotify_access_token = token;
                            c.settings.spotify_token_expires = exp;
                            c.save_settings(paths::paths());
                        }
                        toast(&weak2, &state2, "Spotify connected");
                    }
                    Err(e) => toast(&weak2, &state2, &format!("Spotify: {e}")),
                }
                refresh_weak(&weak2, &state2);
            });
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_sleep(move |minutes| {
            {
                let mut c = state.core.lock().unwrap();
                if minutes <= 0 {
                    c.sleep_until = None;
                } else {
                    c.sleep_until = Some(now_millis() + minutes as i64 * 60_000);
                }
                c.settings.sleep_minutes = minutes;
                c.toast(if minutes > 0 {
                    format!("Sleep timer set for {minutes} min")
                } else {
                    "Sleep timer off".into()
                });
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        app.on_set_quality(move |q| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.audio_quality = q.to_string();
                c.save_settings(paths::paths());
            }
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_toggle_shuffle(move || {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.shuffle = !c.settings.shuffle;
                if c.settings.shuffle && !c.queue.is_empty() {
                    use rand::seq::SliceRandom;
                    c.queue.shuffle(&mut rand::rng());
                }
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_cycle_repeat(move || {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.repeat = (c.settings.repeat + 1) % 3;
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_refresh_home(move || {
            fetch_home(weak.clone(), state.clone());
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_clear_cache(move || {
            let _ = paths::clear_cache();
            {
                let mut c = state.core.lock().unwrap();
                c.cache_size = paths::cache_size_label();
                c.toast("Cache cleared");
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_theme_light(move |v| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.theme_light = v;
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_dynamic(move |v| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.dynamic = v;
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_accent(move |i| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.accent_index = i.max(0) as usize;
                c.settings.dynamic = false;
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_font_scale(move |v| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.font_scale = v.clamp(0.8, 1.4);
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_account_name(move |name| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.account_name = name.to_string();
                c.account_name = name.to_string();
                c.save_settings(paths::paths());
            }
            let _ = weak;
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_open_lyrics(move || {
            {
                let mut c = state.core.lock().unwrap();
                c.set_view("lyrics");
            }
            refresh_weak(&weak, &state);
            load_lyrics_async(state.clone(), weak.clone());
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_load_lyrics(move || {
            load_lyrics_async(state.clone(), weak.clone());
        });
    }

    {
        let state = state.clone();
        app.on_seek_lyric(move |ms| {
            let offset = state.core.lock().unwrap().lyrics_offset;
            state.player.send(PlayerCmd::Seek((ms as i64 + offset).max(0) as u64));
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_nudge_lyrics(move |delta| {
            {
                let mut c = state.core.lock().unwrap();
                c.lyrics_offset += delta as i64;
                let off = c.lyrics_offset;
                c.toast(format!("Lyrics offset {off} ms"));
            }
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_fetch_related(move |_id| {
            fetch_home(weak.clone(), state.clone());
        });
    }

    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_toast_message(move |msg| {
            state.core.lock().unwrap().toast(msg.to_string());
            refresh_weak(&weak, &state);
        });
    }

    {
        let state = state.clone();
        app.on_save_settings(move || {
            state.core.lock().unwrap().save_settings(paths::paths());
        });
    }

    {
        app.on_open_url(move |url: slint::SharedString| {
            android::open_url(&url);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_theme(move |idx| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.theme_index = idx.max(0) as usize;
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_amoled(move |on| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.amoled = on;
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_apply_headphone(move |idx| {
            let i = idx.max(0) as usize;
            if let Some(p) = audio_presets::HEADPHONE_PROFILES.get(i) {
                {
                    let mut c = state.core.lock().unwrap();
                    c.settings.headphone_index = i as i32;
                    c.settings.headphone_profile = p.name.to_string();
                    c.settings.eq_gains = p.gains;
                    c.settings.bass = p.bass;
                    c.settings.eq_preset = -1;
                    c.save_settings(paths::paths());
                    state.player.send(PlayerCmd::Eq(p.gains));
                    state.player.send(PlayerCmd::Bass(p.bass));
                }
                refresh_weak(&weak, &state);
            }
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_visualizer_mode(move |mode| {
            {
                let mut c = state.core.lock().unwrap();
                c.settings.visualizer_mode = mode.to_string();
                c.save_settings(paths::paths());
            }
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_gate(move |on| {
            update_dyn(&state, |s| s.gate = on);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_gate_threshold(move |v| {
            update_dyn(&state, |s| s.gate_threshold = v);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_gate_release(move |v| {
            update_dyn(&state, |s| s.gate_release = v);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_mbc(move |on| {
            update_dyn(&state, |s| s.mbc = on);
            refresh_weak(&weak, &state);
        });
    }
    {
        let state = state.clone();
        let weak = ui.as_weak();
        app.on_set_mbc_band(move |band, threshold, ratio| {
            update_dyn(&state, |s| match band {
                0 => { s.mbc_low_threshold = threshold; s.mbc_low_ratio = ratio; }
                1 => { s.mbc_mid_threshold = threshold; s.mbc_mid_ratio = ratio; }
                _ => { s.mbc_high_threshold = threshold; s.mbc_high_ratio = ratio; }
            });
            refresh_weak(&weak, &state);
        });
    }
}

fn refresh_weak(weak: &slint::Weak<AppWindow>, state: &Arc<AppState>) {
    let weak = weak.clone();
    let state = state.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            refresh(&ui, &state);
        }
    });
}

fn toast(weak: &slint::Weak<AppWindow>, state: &Arc<AppState>, msg: &str) {
    state.core.lock().unwrap().toast(msg);
    refresh_weak(weak, state);
}

fn page_ids(c: &Core) -> Vec<String> {
    match c.view.as_str() {
        "search" => c.search_results.clone(),
        "liked" => c.liked_ids.iter().rev().cloned().collect(),
        "local" => c.local_ids.clone(),
        "history" => c.history_tracks().iter().map(|t| t.id.clone()).collect(),
        "queue" => c.queue.clone(),
        "playlist" | "album" | "artist" => c.detail_ids.clone(),
        "home" => {
            let mut ids = c.home_recent.clone();
            ids.extend(c.home_trending.clone());
            ids
        }
        _ => c.detail_ids.clone(),
    }
}

fn refresh(ui: &AppWindow, state: &Arc<AppState>) {
    let c = state.core.lock().unwrap();
    let app = ui.global::<App>();
    let theme = ui.global::<Theme>();

    let theme_index = c.settings.theme_index.min(themes::THEMES.len().saturating_sub(1));
    app.set_theme_index(theme_index as i32);
    app.set_amoled(c.settings.amoled);
    let theme_key = (theme_index, c.settings.amoled);
    let theme_changed = *state.theme_applied.lock().unwrap() != Some(theme_key);
    if theme_changed {
        apply_theme(ui, &c.settings);
        *state.theme_applied.lock().unwrap() = Some(theme_key);
    }
    if c.settings.dynamic {
        let accent = c
            .current
            .as_ref()
            .and_then(|id| c.tracks.get(id))
            .map(|t| t.accent.clone())
            .unwrap_or_else(|| themes::THEMES[theme_index].accent.to_string());
        theme.set_accent(images::color_from_hex(&accent));
    }
    theme.set_font_scale(c.settings.font_scale);
    theme.set_dynamic(c.settings.dynamic);

    app.set_view(c.view.clone().into());
    app.set_page_title(c.page_title.clone().into());
    app.set_page_subtitle(c.page_subtitle.clone().into());
    app.set_page_id(c.page_id.clone().into());
    app.set_page_accent(images::color_from_hex(&c.page_accent));
    app.set_page_art(model_art(&c.page_id, &c.tracks));
    app.set_liked_count(c.liked_ids.len() as i32);
    app.set_local_count(c.local_ids.len() as i32);
    app.set_queue_index(
        c.queue_index
            .map(|i| i as i32)
            .unwrap_or(-1),
    );
    app.set_offline(c.offline);
    app.set_loading_home(c.loading_home);
    app.set_scan_busy(c.scan_busy);
    app.set_import_busy(c.import_busy);
    app.set_import_progress(c.import_progress);
    app.set_import_status(c.import_status.clone().into());
    app.set_import_url(c.import_url.clone().into());
    app.set_import_source(c.import_source.clone().into());
    app.set_account_name(c.settings.account_name.clone().into());
    app.set_account_sub(if c.liked_ids.is_empty() {
        "Local profile".into()
    } else {
        format!("{} liked songs", c.liked_ids.len()).into()
    });
    app.set_cache_size(c.cache_size.clone().into());
    app.set_search_query(c.search_query.clone().into());
    app.set_search_loading(c.search_loading);
    app.set_search_source(c.search_source.clone().into());
    app.set_theme_light(c.settings.theme_light);
    app.set_dynamic_color(c.settings.dynamic);
    app.set_accent_index(c.settings.accent_index as i32);
    app.set_font_scale(c.settings.font_scale);
    app.set_volume(c.settings.volume);
    app.set_shuffle(c.settings.shuffle);
    app.set_repeat(c.settings.repeat);
    app.set_speed(c.settings.speed);
    app.set_pitch(c.settings.pitch);
    app.set_bass_boost(c.settings.bass);
    app.set_reverb(c.settings.reverb);
    app.set_normalizer(c.settings.normalizer);
    app.set_compressor(c.settings.compressor);
    app.set_comp_threshold(c.settings.comp_threshold);
    app.set_comp_ratio(c.settings.comp_ratio);
    app.set_comp_attack(c.settings.comp_attack);
    app.set_comp_release(c.settings.comp_release);
    app.set_comp_makeup(c.settings.comp_makeup);
    app.set_limiter(c.settings.limiter);
    app.set_limiter_ceiling(c.settings.limiter_ceiling);
    app.set_eq_profiles(ModelRc::new(VecModel::from(
        c.settings.profiles.iter().map(|p| slint::SharedString::from(p.name.as_str())).collect::<Vec<_>>(),
    )));
    if app.get_theme_list().row_count() == 0 {
        let list: Vec<ThemeItem> = themes::THEMES
            .iter()
            .map(|t| ThemeItem {
                group: t.group.into(),
                name: t.name.into(),
                accent: images::color_from_hex(t.accent),
                bg: images::color_from_hex(t.bg),
                surface: images::color_from_hex(t.surface),
                text: images::color_from_hex(t.text),
                sub: images::color_from_hex(t.sub),
                light: t.light,
            })
            .collect();
        app.set_theme_list(ModelRc::new(VecModel::from(list)));
    }
    app.set_headphone_profiles(ModelRc::new(VecModel::from(
        audio_presets::HEADPHONE_PROFILES
            .iter()
            .map(|p| slint::SharedString::from(p.name))
            .collect::<Vec<_>>(),
    )));
    app.set_headphone_index(c.settings.headphone_index);
    app.set_headphone_profile(c.settings.headphone_profile.clone().into());
    app.set_visualizer_mode(c.settings.visualizer_mode.clone().into());
    app.set_eq_tab(c.settings.eq_tab.clone().into());
    app.set_lyrics_source(state.lyrics_source.lock().unwrap().clone().into());
    app.set_gate(c.settings.gate);
    app.set_gate_threshold(c.settings.gate_threshold);
    app.set_gate_release(c.settings.gate_release);
    app.set_mbc(c.settings.mbc);
    app.set_mbc_low_threshold(c.settings.mbc_low_threshold);
    app.set_mbc_low_ratio(c.settings.mbc_low_ratio);
    app.set_mbc_mid_threshold(c.settings.mbc_mid_threshold);
    app.set_mbc_mid_ratio(c.settings.mbc_mid_ratio);
    app.set_mbc_high_threshold(c.settings.mbc_high_threshold);
    app.set_mbc_high_ratio(c.settings.mbc_high_ratio);
    app.set_spotify_client_id(c.settings.spotify_client_id.clone().into());
    app.set_spotify_client_secret(c.settings.spotify_client_secret.clone().into());
    let spotify_ok = !c.settings.spotify_access_token.is_empty()
        && c.settings.spotify_token_expires > now_millis();
    app.set_spotify_connected(spotify_ok);
    app.set_spotify_status(if spotify_ok {
        "Connected · catalog + previews".into()
    } else if !c.settings.spotify_client_id.is_empty() {
        "Credentials saved · tap Connect".into()
    } else {
        "Not connected · add a developer app".into()
    });
    app.set_visualizer_on(c.settings.visualizer_on);
    app.set_audio_quality(c.settings.audio_quality.clone().into());
    app.set_download_quality(c.settings.download_quality.clone().into());
    app.set_sleep_minutes(c.settings.sleep_minutes);
    app.set_sleep_label(
        if let Some(until) = c.sleep_until {
            let secs = ((until - now_millis()).max(0) / 1000) as u64;
            format_time(secs).into()
        } else {
            String::new().into()
        },
    );
    app.set_eq_preset(c.settings.eq_preset);
    app.set_eq_gains(ModelRc::new(VecModel::from(
        c.settings.eq_gains.to_vec(),
    )));
    app.set_eq_presets(ModelRc::new(VecModel::from(
        EQ_PRESET_NAMES
            .iter()
            .map(|s| slint::SharedString::from(*s))
            .collect::<Vec<_>>(),
    )));
    app.set_genres(ModelRc::new(VecModel::from(
        GENRES
            .iter()
            .map(|s| slint::SharedString::from(*s))
            .collect::<Vec<_>>(),
    )));
    app.set_search_history(ModelRc::new(VecModel::from(
        c.settings
            .search_history
            .iter()
            .map(|s| slint::SharedString::from(s.as_str()))
            .collect::<Vec<_>>(),
    )));
    app.set_import_sources(ModelRc::new(VecModel::from(
        ["YouTube", "SoundCloud", "Spotify"]
            .iter()
            .map(|s| slint::SharedString::from(*s))
            .collect::<Vec<_>>(),
    )));

    let recent: Vec<TrackItem> = c
        .history_tracks()
        .into_iter()
        .take(12)
        .map(|t| track_item(&c, &t))
        .collect();
    app.set_home_recent(ModelRc::new(VecModel::from(recent)));
    app.set_home_made(ModelRc::new(VecModel::from(
        c.home_made
            .iter()
            .filter_map(|id| c.tracks.get(id))
            .take(12)
            .map(|t| track_item(&c, t))
            .collect::<Vec<_>>(),
    )));
    app.set_home_trending(ModelRc::new(VecModel::from(
        c.home_trending
            .iter()
            .filter_map(|id| c.tracks.get(id))
            .take(14)
            .map(|t| track_item(&c, t))
            .collect::<Vec<_>>(),
    )));
    app.set_home_quick(ModelRc::new(VecModel::from(
        c.home_quick
            .iter()
            .filter_map(|id| c.tracks.get(id))
            .take(6)
            .map(|t| track_item(&c, t))
            .collect::<Vec<_>>(),
    )));
    app.set_search_results(ModelRc::new(VecModel::from(
        c.search_results
            .iter()
            .filter_map(|id| c.tracks.get(id))
            .map(|t| track_item(&c, t))
            .collect::<Vec<_>>(),
    )));
    app.set_library_tracks(ModelRc::new(VecModel::from(
        c.local_tracks()
            .iter()
            .map(|t| track_item(&c, t))
            .collect::<Vec<_>>(),
    )));
    app.set_liked_tracks(ModelRc::new(VecModel::from(
        c.liked_tracks()
            .iter()
            .map(|t| track_item(&c, t))
            .collect::<Vec<_>>(),
    )));
    app.set_history(ModelRc::new(VecModel::from(
        c.history_tracks()
            .iter()
            .map(|t| track_item(&c, t))
            .collect::<Vec<_>>(),
    )));
    app.set_queue(ModelRc::new(VecModel::from(
        c.queue_tracks()
            .iter()
            .map(|t| track_item(&c, t))
            .collect::<Vec<_>>(),
    )));
    app.set_detail_tracks(ModelRc::new(VecModel::from(
        c.detail_ids
            .iter()
            .filter_map(|id| c.tracks.get(id))
            .map(|t| track_item(&c, t))
            .collect::<Vec<_>>(),
    )));

    let albums: Vec<AlbumItem> = c
        .albums()
        .into_iter()
        .map(|(name, tracks)| {
            let first = tracks.first();
            AlbumItem {
                id: name.clone().into(),
                name: name.clone().into(),
                subtitle: format!(
                    "{} · {} tracks",
                    first.map(|t| t.artist.as_str()).unwrap_or("Various artists"),
                    tracks.len()
                )
                .into(),
                art: first
                    .map(|t| images::image_for_track(t))
                    .unwrap_or_default(),
                accent: first
                    .map(|t| images::color_from_hex(&t.accent))
                    .unwrap_or(theme.get_accent()),
                count: tracks.len() as i32,
            }
        })
        .collect();
    app.set_albums(ModelRc::new(VecModel::from(albums)));

    let playlists: Vec<PlaylistItem> = c
        .playlists
        .iter()
        .map(|p| PlaylistItem {
            id: p.id.clone().into(),
            name: p.name.clone().into(),
            subtitle: if p.subtitle.is_empty() {
                format!("{} tracks", p.track_ids.len()).into()
            } else {
                p.subtitle.clone().into()
            },
            art: p
                .track_ids
                .first()
                .and_then(|id| c.tracks.get(id))
                .map(|t| images::image_for_track(t))
                .unwrap_or_default(),
            accent: images::color_from_hex(&p.accent),
            count: p.track_ids.len() as i32,
        })
        .collect();
    app.set_playlists(ModelRc::new(VecModel::from(playlists)));

    let snapshot = state.player.snapshot();
    app.set_playing(snapshot.playing);
    app.set_loading(snapshot.loading);
    app.set_position(snapshot.position_ms as i32);
    app.set_position_label(format_ms(snapshot.position_ms).into());
    app.set_duration(snapshot.duration_ms as i32);
    app.set_duration_label(format_ms(snapshot.duration_ms).into());
    app.set_spectrum(ModelRc::new(VecModel::from(
        snapshot.spectrum.to_vec(),
    )));
    app.set_current(
        c.current
            .as_ref()
            .and_then(|id| c.tracks.get(id))
            .map(|t| track_item(&c, t))
            .unwrap_or_default(),
    );
    app.set_current_liked(
        c.current
            .as_ref()
            .and_then(|id| c.tracks.get(id))
            .map(|t| t.liked)
            .unwrap_or(false),
    );

    // Lyrics model: update in place to keep scroll position.
    let mut lyrics_sig = state.lyrics_ui.lock().unwrap();
    let active = active_lyric(&c, snapshot.position_ms as i64);
    let sig = (
        c.lyrics_track.clone().unwrap_or_default(),
        c.lyrics.len(),
        active.unwrap_or(usize::MAX),
    );
    let changed = lyrics_sig.as_ref() != Some(&sig);
    if changed {
        let model: Vec<LyricLine> = c
            .lyrics
            .iter()
            .enumerate()
            .map(|(i, l)| LyricLine {
                time: l.time as i32,
                text: l.text.clone().into(),
                active: Some(i) == active,
            })
            .collect();
        if let Some(i) = active {
            app.set_lyrics(ModelRc::new(VecModel::from(model)));
            *lyrics_sig = Some((sig.0.clone(), c.lyrics.len(), i));
        } else if lyrics_sig.is_none() || lyrics_sig.as_ref().map(|s| s.0.clone()) != Some(sig.0.clone()) {
            app.set_lyrics(ModelRc::new(VecModel::from(model)));
            *lyrics_sig = Some((sig.0.clone(), c.lyrics.len(), usize::MAX));
        }
    }
    drop(lyrics_sig);

    if now_millis() < c.toast_until && !c.toast_text.is_empty() {
        app.set_toast(c.toast_text.clone().into());
        app.set_toast_visible(true);
    } else {
        app.set_toast_visible(false);
    }
}

fn active_lyric(c: &Core, position_ms: i64) -> Option<usize> {
    if c.lyrics.is_empty() {
        return None;
    }
    let pos = position_ms + c.lyrics_offset;
    let mut active = None;
    for (i, l) in c.lyrics.iter().enumerate() {
        if l.time <= pos {
            active = Some(i);
        } else {
            break;
        }
    }
    active
}

fn track_item(_c: &Core, t: &Track) -> TrackItem {
    TrackItem {
        id: t.id.clone().into(),
        title: t.title.clone().into(),
        artist: t.artist.clone().into(),
        album: t.album.clone().into(),
        duration: t.duration as i32,
        duration_label: t.duration_label().into(),
        art: images::image_for_track(t),
        accent: images::color_from_hex(&t.accent),
        liked: t.liked,
        source: t.source.label().into(),
        subtitle: t.subtitle().into(),
    }
}

fn model_art(page_id: &str, tracks: &HashMap<String, Track>) -> slint::Image {
    if let Some(t) = tracks.get(page_id) {
        return images::image_for_track(t);
    }
    if let Some(t) = tracks.values().find(|t| t.album == page_id || t.artist == page_id) {
        return images::image_for_track(t);
    }
    slint::Image::default()
}

fn mix_hex(a: &str, b: &str, t: f32) -> String {
    let (ar, ag, ab) = accent_rgb(a);
    let (br, bg, bb) = accent_rgb(b);
    let t = t.clamp(0.0, 1.0);
    format!(
        "#{:02x}{:02x}{:02x}",
        (ar as f32 + (br as f32 - ar as f32) * t) as u8,
        (ag as f32 + (bg as f32 - ag as f32) * t) as u8,
        (ab as f32 + (bb as f32 - ab as f32) * t) as u8
    )
}

fn apply_theme(ui: &AppWindow, settings: &Settings) {
    let theme = ui.global::<Theme>();
    let preset = &themes::THEMES[settings.theme_index.min(themes::THEMES.len().saturating_sub(1))];
    let (bg, surface, nav) = if settings.amoled {
        ("#000000", "#0a0a0a", "#000000")
    } else {
        (preset.bg, preset.surface, preset.bg)
    };
    let text = preset.text;
    let sub = preset.sub;
    let surface2 = mix_hex(surface, text, 0.07);
    let surface3 = mix_hex(surface, text, 0.14);
    let line = mix_hex(bg, text, 0.17);
    let faint = mix_hex(sub, bg, 0.30);
    let accent = preset.accent;
    let accent2 = mix_hex(accent, text, 0.18);
    theme.set_light(preset.light);
    theme.set_amoled(settings.amoled);
    theme.set_bg(images::color_from_hex(bg));
    theme.set_bg_elevated(images::color_from_hex(surface));
    theme.set_surface(images::color_from_hex(surface));
    theme.set_surface_2(images::color_from_hex(&surface2));
    theme.set_surface_3(images::color_from_hex(&surface3));
    theme.set_line(images::color_from_hex(&line));
    theme.set_text(images::color_from_hex(text));
    theme.set_text_sub(images::color_from_hex(sub));
    theme.set_text_faint(images::color_from_hex(&faint));
    theme.set_nav(images::color_from_hex(nav));
    theme.set_accent(images::color_from_hex(accent));
    theme.set_accent_2(images::color_from_hex(&accent2));
    theme.set_danger(images::color_from_hex("#ff4d6d"));
    theme.set_success(images::color_from_hex("#1ed760"));
    theme.set_font_scale(settings.font_scale);
    theme.set_dynamic(settings.dynamic);
}

fn send_dynamics(c: &Core, player: &PlayerHandle) {
    player.send(PlayerCmd::Dynamics(Dynamics {
        compressor: c.settings.compressor,
        threshold: c.settings.comp_threshold,
        ratio: c.settings.comp_ratio,
        attack_ms: c.settings.comp_attack,
        release_ms: c.settings.comp_release,
        makeup: c.settings.comp_makeup,
        limiter: c.settings.limiter,
        ceiling: c.settings.limiter_ceiling,
        gate: c.settings.gate,
        gate_threshold: c.settings.gate_threshold,
        gate_release_ms: c.settings.gate_release,
        mbc: c.settings.mbc,
        mbc_low_threshold: c.settings.mbc_low_threshold,
        mbc_low_ratio: c.settings.mbc_low_ratio,
        mbc_mid_threshold: c.settings.mbc_mid_threshold,
        mbc_mid_ratio: c.settings.mbc_mid_ratio,
        mbc_high_threshold: c.settings.mbc_high_threshold,
        mbc_high_ratio: c.settings.mbc_high_ratio,
    }));
}

fn update_dyn(state: &Arc<AppState>, f: impl FnOnce(&mut Settings)) {
    let mut c = state.core.lock().unwrap();
    f(&mut c.settings);
    c.save_settings(paths::paths());
    send_dynamics(&c, &state.player);
}

fn ensure_spotify_token(state: &Arc<AppState>) -> Option<String> {
    let (id, secret, token, expires) = {
        let c = state.core.lock().unwrap();
        (
            c.settings.spotify_client_id.trim().to_string(),
            c.settings.spotify_client_secret.trim().to_string(),
            c.settings.spotify_access_token.clone(),
            c.settings.spotify_token_expires,
        )
    };
    if !token.is_empty() && expires > now_millis() {
        return Some(token);
    }
    if id.is_empty() || secret.is_empty() {
        return None;
    }
    match sources::spotify::client_token(&id, &secret) {
        Ok((token, exp)) => {
            let mut c = state.core.lock().unwrap();
            c.settings.spotify_access_token = token.clone();
            c.settings.spotify_token_expires = exp;
            c.save_settings(paths::paths());
            Some(token)
        }
        Err(e) => {
            log::warn!("spotify token refresh: {e}");
            None
        }
    }
}

fn apply_preset(state: &Arc<AppState>, idx: usize) {
    let mut c = state.core.lock().unwrap();
    let presets: [([f32; 10], f32, f32, f32, f32, bool); 10] = [
        ([0.0; 10], 1.0, 0.0, 0.0, 0.0, false),
        ([6.0, 5.0, 4.0, 2.0, 0.0, 0.0, -1.0, -2.0, -3.0, -4.0], 1.0, 0.0, 0.0, 0.0, false),
        ([-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 4.0, 5.0, 6.0, 7.0], 1.0, 0.0, 0.0, 0.0, false),
        ([-2.0, -1.0, 0.0, 2.0, 4.0, 4.0, 3.0, 1.0, 0.0, -1.0], 1.0, 0.0, 0.0, 0.0, false),
        ([4.0, 3.0, 2.0, 0.0, -1.0, -1.0, 1.0, 3.0, 4.0, 4.0], 1.0, 0.0, 0.0, 0.0, false),
        ([5.0, 4.0, 2.0, 0.0, -2.0, 1.0, 3.0, 4.0, 5.0, 5.0], 1.0, 0.0, 0.0, 0.0, false),
        ([3.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0], 0.82, -3.0, 2.0, 0.55, false),
        ([-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0, 3.0, 2.0], 1.28, 4.0, 2.0, 0.08, false),
        ([10.0, 9.0, 7.0, 4.0, 1.0, 0.0, -1.0, -2.0, -3.0, -4.0], 1.0, 0.0, 7.0, 0.0, false),
        ([0.0, 0.0, -1.0, 0.0, 2.0, 3.0, 2.0, 1.0, 0.0, -1.0], 1.0, 0.0, 0.0, 0.0, true),
    ];
    let (gains, speed, pitch, bass, reverb, norm) = presets[idx.min(9)];
    c.settings.eq_gains = gains;
    c.settings.speed = speed;
    c.settings.pitch = pitch;
    c.settings.bass = bass;
    c.settings.reverb = reverb;
    c.settings.normalizer = norm;
    c.settings.eq_preset = idx as i32;
    let effective_speed = speed * 2f32.powf(pitch / 12.0);
    state.player.send(PlayerCmd::Eq(gains));
    state.player.send(PlayerCmd::Speed(effective_speed));
    state.player.send(PlayerCmd::Bass(bass));
    state.player.send(PlayerCmd::Reverb(reverb));
    state.player.send(PlayerCmd::Normalizer(norm));
    c.save_settings(paths::paths());
}

const EQ_PRESET_NAMES: [&str; 10] = [
    "Flat",
    "Bass",
    "Treble",
    "Vocal",
    "Rock",
    "Electronic",
    "Slowed + reverb",
    "Nightcore",
    "Bass worship",
    "Clear sound",
];

const GENRES: [&str; 8] = [
    "Chill",
    "Focus",
    "Workout",
    "Jazz",
    "Lo-fi",
    "Pop",
    "Rock",
    "Late night",
];

fn start_timer(weak: slint::Weak<AppWindow>, state: Arc<AppState>) {
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
        let Some(ui) = weak.upgrade() else { return };
        let snap = state.player.snapshot();
        let app = ui.global::<App>();
        app.set_playing(snap.playing);
        app.set_loading(snap.loading);
        app.set_position(snap.position_ms as i32);
        app.set_position_label(format_ms(snap.position_ms).into());
        if snap.duration_ms > 0 {
            app.set_duration(snap.duration_ms as i32);
            app.set_duration_label(format_ms(snap.duration_ms).into());
        }
        if app.get_visualizer_on() {
            app.set_spectrum(ModelRc::new(VecModel::from(snap.spectrum.to_vec())));
        }

        let (ended, repeat, current, sleep_due) = {
            let mut c = state.core.lock().unwrap();
            if snap.ended && !c.last_ended_handled {
                c.last_ended_handled = true;
                (true, c.settings.repeat, c.current.clone(), false)
            } else {
                let due = c.sleep_until.map(|u| now_millis() >= u).unwrap_or(false);
                if due {
                    c.sleep_until = None;
                    c.settings.sleep_minutes = 0;
                }
                (false, c.settings.repeat, c.current.clone(), due)
            }
        };

        if sleep_due {
            state.player.send(PlayerCmd::Pause);
            state.core.lock().unwrap().toast("Sleep timer ended · playback paused");
            refresh_weak(&weak, &state);
        }

        if ended {
            if repeat == 2 {
                if let Some(id) = current {
                    play_track(&state, &weak, &id);
                }
            } else {
                next_track(&state, &weak);
            }
        }

        // Keep the active lyric highlighted without rebuilding the model.
        let (lyrics_track, active, offset_ok) = {
            let c = state.core.lock().unwrap();
            (
                c.lyrics_track.clone(),
                active_lyric(&c, snap.position_ms as i64),
                true,
            )
        };
        if offset_ok && app.get_view() == "lyrics" {
            if let (Some(track), Some(active)) = (lyrics_track, active) {
                let model = app.get_lyrics();
                if model.row_count() > active {
                    let mut changed = false;
                    let mut data = Vec::with_capacity(model.row_count());
                    for i in 0..model.row_count() {
                        if let Some(mut row) = model.row_data(i) {
                            let should = i == active;
                            if row.active != should {
                                row.active = should;
                                changed = true;
                            }
                            data.push(row);
                        }
                    }
                    if changed {
                        let _ = track;
                        app.set_lyrics(ModelRc::new(VecModel::from(data)));
                    }
                }
            }
        }

        if now_millis() >= state.core.lock().unwrap().toast_until {
            app.set_toast_visible(false);
        } else if let Some(until) = state.core.lock().unwrap().sleep_until {
            let secs = ((until - now_millis()).max(0) / 1000) as u64;
            app.set_sleep_label(format_time(secs).into());
        }
    });
    std::mem::forget(timer);
}

fn fetch_home(weak: slint::Weak<AppWindow>, state: Arc<AppState>) {
    if *state.home_loading.lock().unwrap() {
        return;
    }
    *state.home_loading.lock().unwrap() = true;
    {
        let mut c = state.core.lock().unwrap();
        c.loading_home = true;
    }
    refresh_weak(&weak, &state);

    std::thread::spawn(move || {
        let mut recent: Vec<String> = {
            let c = state.core.lock().unwrap();
            c.history_tracks().iter().take(12).map(|t| t.id.clone()).collect()
        };
        if recent.is_empty() {
            let c = state.core.lock().unwrap();
            recent = c
                .liked_tracks()
                .iter()
                .chain(c.local_tracks().iter())
                .take(12)
                .map(|t| t.id.clone())
                .collect();
        }

        let mut made_ids = Vec::new();
        let mut trending_ids = Vec::new();
        let mut any_network = false;
        let made_queries = [
            "chill mix playlist",
            "focus music",
            "late night drive music",
            "feel good songs",
        ];
        for q in made_queries {
            match sources::youtube::search(q, 8) {
                Ok(tracks) => {
                    any_network = true;
                    for mut t in tracks {
                        images::ensure_artwork(&mut t);
                        let id = t.id.clone();
                        state.core.lock().unwrap().add_track(t);
                        made_ids.push(id);
                    }
                }
                Err(e) => log::warn!("home query {q}: {e}"),
            }
        }
        for q in ["trending music 2026", "new music this week"] {
            match sources::youtube::search(q, 10) {
                Ok(tracks) => {
                    any_network = true;
                    for mut t in tracks {
                        images::ensure_artwork(&mut t);
                        let id = t.id.clone();
                        state.core.lock().unwrap().add_track(t);
                        trending_ids.push(id);
                    }
                }
                Err(e) => log::warn!("trending query {q}: {e}"),
            }
        }
        if let Ok(tracks) = sources::soundcloud::search("lofi chill", 8) {
            any_network = true;
            for mut t in tracks {
                images::ensure_artwork(&mut t);
                let id = t.id.clone();
                state.core.lock().unwrap().add_track(t);
                trending_ids.push(id);
            }
        }
        made_ids.truncate(12);
        trending_ids.truncate(14);
        let mut quick = Vec::new();
        for id in made_ids.iter().step_by(3).take(3) {
            quick.push(id.clone());
        }
        for id in trending_ids.iter().step_by(3).take(3) {
            quick.push(id.clone());
        }
        {
            let mut c = state.core.lock().unwrap();
            c.home_recent = recent;
            if !made_ids.is_empty() {
                c.home_made = made_ids;
            } else {
                c.home_made = c
                    .liked_tracks()
                    .iter()
                    .chain(c.local_tracks().iter())
                    .take(10)
                    .map(|t| t.id.clone())
                    .collect();
            }
            if !trending_ids.is_empty() {
                c.home_trending = trending_ids;
            } else {
                c.home_trending = c.local_ids.iter().take(10).cloned().collect();
            }
            if !quick.is_empty() {
                c.home_quick = quick;
            } else {
                c.home_quick = c.local_ids.iter().take(6).cloned().collect();
            }
            c.loading_home = false;
            c.offline = !any_network;
            c.save_library(paths::paths());
        }
        *state.home_loading.lock().unwrap() = false;
        refresh_weak(&weak, &state);
    });
}

fn run_search(state: Arc<AppState>, weak: slint::Weak<AppWindow>, query: String, source: String) {
    std::thread::spawn(move || {
        let mut ids = Vec::new();
        {
            let c = state.core.lock().unwrap();
            let ql = query.to_lowercase();
            for t in c.tracks.values() {
                if t.title.to_lowercase().contains(&ql) || t.artist.to_lowercase().contains(&ql) {
                    ids.push(t.id.clone());
                }
            }
        }
        let mut any_network = false;
        if source == "All" || source == "Spotify" {
            if let Some(token) = ensure_spotify_token(&state) {
                match sources::spotify::search_tracks(&token, &query, 25) {
                    Ok(tracks) => {
                        any_network = true;
                        for (i, mut t) in tracks.into_iter().enumerate() {
                            if i < 12 {
                                images::ensure_artwork(&mut t);
                            }
                            let id = t.id.clone();
                            state.core.lock().unwrap().add_track(t);
                            ids.push(id);
                        }
                    }
                    Err(e) => log::warn!("spotify search: {e}"),
                }
            }
        }
        if source == "All" || source == "YouTube" {
            match sources::youtube::search(&query, 25) {
                Ok(tracks) => {
                    any_network = true;
                    for (i, mut t) in tracks.into_iter().enumerate() {
                        if i < 14 {
                            images::ensure_artwork(&mut t);
                        }
                        let id = t.id.clone();
                        state.core.lock().unwrap().add_track(t);
                        ids.push(id);
                    }
                }
                Err(e) => log::warn!("youtube search: {e}"),
            }
        }
        if source == "All" || source == "SoundCloud" {
            match sources::soundcloud::search(&query, 15) {
                Ok(tracks) => {
                    any_network = true;
                    for mut t in tracks {
                        images::ensure_artwork(&mut t);
                        let id = t.id.clone();
                        state.core.lock().unwrap().add_track(t);
                        ids.push(id);
                    }
                }
                Err(e) => log::warn!("soundcloud search: {e}"),
            }
        }
        ids.dedup();
        {
            let mut c = state.core.lock().unwrap();
            c.search_results = ids;
            c.search_loading = false;
            c.offline = !any_network && c.search_results.is_empty();
            c.save_library(paths::paths());
        }
        refresh_weak(&weak, &state);
    });
}

fn play_track(state: &Arc<AppState>, weak: &slint::Weak<AppWindow>, id: &str) {
    let (track, quality) = {
        let mut c = state.core.lock().unwrap();
        let Some(mut track) = c.tracks.get(id).cloned() else {
            return;
        };
        if track.source == Source::Spotify && track.local_path.is_none() {
            if let Some(path) = find_local_lossless(&c, &track) {
                track.local_path = Some(path);
            }
        }
        c.current = Some(id.to_string());
        c.last_ended_handled = false;
        c.pending_seek = Some(0);
        c.lyrics.clear();
        c.lyrics_track = None;
        c.lyrics_offset = 0;
        if c.queue.iter().any(|x| x == id) {
            if let Some(i) = c.queue.iter().position(|x| x == id) {
                c.queue_index = Some(i);
            }
        } else {
            let page = page_ids(&c);
            if page.iter().any(|x| x == id) {
                c.queue = page;
                if let Some(i) = c.queue.iter().position(|x| x == id) {
                    c.queue_index = Some(i);
                }
            } else {
                c.queue = vec![id.to_string()];
                c.queue_index = Some(0);
            }
        }
        c.push_history(id);
        c.page_accent = track.accent.clone();
        c.save_library(paths::paths());
        (track, c.settings.audio_quality.clone())
    };
    if let Ok(mut s) = state.player.snapshot.lock() {
        s.loading = true;
        s.playing = false;
        s.ended = false;
        s.error = None;
        s.position_ms = 0;
        s.duration_ms = track.duration * 1000;
        s.spectrum = [0.0; 32];
    }
    android::keep_screen_on(true);
    refresh_weak(weak, state);

    let state2 = state.clone();
    let weak2 = weak.clone();
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        match prepare_audio(&track, &quality) {
            Ok((path, duration)) => {
                log::info!(
                    "playing {} from {} ({:?})",
                    track.title,
                    path.display(),
                    start.elapsed()
                );
                let dur = if duration > 0 { duration } else { track.duration };
                {
                    let mut c = state2.core.lock().unwrap();
                    if let Some(t) = c.tracks.get_mut(&track.id) {
                        if t.duration == 0 && dur > 0 {
                            t.duration = dur;
                        }
                    }
                }
                if let Ok(mut s) = state2.player.snapshot.lock() {
                    s.loading = false;
                    s.duration_ms = dur * 1000;
                }
                state2.player.send(PlayerCmd::Play {
                    path,
                    duration: dur,
                    start_ms: 0,
                });
            }
            Err(e) => {
                log::warn!("play failed: {e}");
                if let Ok(mut s) = state2.player.snapshot.lock() {
                    s.loading = false;
                    s.playing = false;
                    s.error = Some(format!("{e}"));
                }
                if let Ok(mut c) = state2.core.lock() {
                    c.toast(format!("Could not play: {e}"));
                }
            }
        }
        refresh_weak(&weak2, &state2);
        load_lyrics_for(&state2, &weak2, track.id.clone());
        android::keep_screen_on(false);
    });
}

fn prepare_audio(track: &Track, quality: &str) -> anyhow::Result<(PathBuf, u64)> {
    if let Some(p) = track.local_path.as_ref() {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok((path, track.duration));
        }
    }
    let hash = hash_id(&track.id);
    let dest = match track.source {
        Source::Youtube => paths::paths().audio.join(format!("{hash}.m4a")),
        Source::Soundcloud => paths::paths().audio.join(format!("{hash}.m4a")),
        Source::Spotify => paths::paths().audio.join(format!("{hash}.mp3")),
        _ => paths::paths().audio.join(format!("{hash}.audio")),
    };
    if dest.exists() && std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) > 24_000 {
        return Ok((dest, track.duration));
    }
    match track.source {
        Source::Youtube => {
            let Some(video_id) = sources::youtube::get_video_id(&track.id) else {
                anyhow::bail!("Invalid YouTube track");
            };
            let info = sources::youtube::stream_info(video_id, quality)?;
            sources::youtube::download_audio(&info, &dest)?;
            Ok((
                dest,
                if info.duration > 0 {
                    info.duration
                } else {
                    track.duration
                },
            ))
        }
        Source::Soundcloud => {
            let Some(track_id) = sources::soundcloud::extract_track_id(&track.id) else {
                anyhow::bail!("Invalid SoundCloud track");
            };
            let hls = sources::soundcloud::hls_url(track_id)?;
            sources::hls::download_hls(&hls, &dest)?;
            Ok((dest, track.duration))
        }
        Source::Spotify => {
            let Some(url) = track.stream_url.as_ref() else {
                anyhow::bail!("Spotify preview unavailable");
            };
            let bytes = images::download_bytes(url)?;
            if bytes.len() < 4096 {
                anyhow::bail!("Spotify preview was empty");
            }
            std::fs::write(&dest, &bytes)?;
            Ok((dest, track.duration.min(30)))
        }
        _ => anyhow::bail!("This track has no playable source"),
    }
}

fn find_local_lossless(c: &Core, track: &Track) -> Option<String> {
    fn norm(s: &str) -> String {
        s.to_lowercase()
            .chars()
            .filter(|ch| ch.is_alphanumeric())
            .collect::<String>()
    }
    let title = norm(&track.title);
    let artist = norm(&track.artist);
    c.local_ids.iter().filter_map(|id| c.tracks.get(id)).find_map(|t| {
        let path = t.local_path.as_ref()?;
        let ext = Path::new(path).extension()?.to_string_lossy().to_lowercase();
        if ext != "flac" && ext != "wav" && ext != "aiff" && ext != "alac" {
            return None;
        }
        let tt = norm(&t.title);
        let aa = norm(&t.artist);
        let same_title = !title.is_empty() && (tt == title || tt.contains(&title) || title.contains(&tt));
        let same_artist = artist.is_empty() || aa.is_empty() || aa.contains(&artist) || artist.contains(&aa);
        (same_title && same_artist).then(|| path.clone())
    })
}

fn next_track(state: &Arc<AppState>, weak: &slint::Weak<AppWindow>) {
    let next = {
        let mut c = state.core.lock().unwrap();
        if c.queue.is_empty() {
            None
        } else if c.settings.shuffle {
            use rand::Rng;
            let mut rng = rand::rng();
            let idx = rng.random_range(0..c.queue.len());
            c.queue_index = Some(idx);
            c.queue.get(idx).cloned()
        } else {
            let idx = c.queue_index.unwrap_or(0) + 1;
            if idx >= c.queue.len() {
                if c.settings.repeat == 0 {
                    c.queue_index = Some(c.queue.len().saturating_sub(1));
                    None
                } else {
                    c.queue_index = Some(0);
                    c.queue.first().cloned()
                }
            } else {
                c.queue_index = Some(idx);
                c.queue.get(idx).cloned()
            }
        }
    };
    if let Some(id) = next {
        play_track(state, weak, &id);
    } else {
        state.player.send(PlayerCmd::Stop);
        let mut c = state.core.lock().unwrap();
        c.last_ended_handled = true;
        c.toast("End of queue");
        drop(c);
        refresh_weak(weak, state);
    }
}

fn previous_track(state: &Arc<AppState>, weak: &slint::Weak<AppWindow>) {
    let prev = {
        let mut c = state.core.lock().unwrap();
        if c.queue.is_empty() {
            None
        } else {
            let idx = c.queue_index.unwrap_or(0);
            let idx = if idx == 0 { c.queue.len() - 1 } else { idx - 1 };
            c.queue_index = Some(idx);
            c.queue.get(idx).cloned()
        }
    };
    if let Some(id) = prev {
        play_track(state, weak, &id);
    }
}

fn load_lyrics_async(state: Arc<AppState>, weak: slint::Weak<AppWindow>) {
    let current = state.core.lock().unwrap().current.clone();
    if let Some(id) = current {
        load_lyrics_for(&state, &weak, id);
    }
}

fn load_lyrics_for(state: &Arc<AppState>, weak: &slint::Weak<AppWindow>, id: String) {
    let track = {
        let c = state.core.lock().unwrap();
        c.tracks.get(&id).cloned()
    };
    let Some(track) = track else { return };
    let state2 = state.clone();
    let weak2 = weak.clone();
    std::thread::spawn(move || {
        match lyrics::load_for_track_with_source(&track) {
            Ok((lines, source)) => {
                *state2.lyrics_source.lock().unwrap() = source.to_string();
                let mut c = state2.core.lock().unwrap();
                if c.current.as_deref() == Some(track.id.as_str()) {
                    c.lyrics = lines;
                    c.lyrics_track = Some(track.id.clone());
                    c.lyrics_offset = 0;
                }
                drop(c);
                refresh_weak(&weak2, &state2);
            }
            Err(e) => {
                log::info!("lyrics: {e}");
                let mut c = state2.core.lock().unwrap();
                c.toast("No synced lyrics found");
                drop(c);
                refresh_weak(&weak2, &state2);
            }
        }
    });
}

fn import_playlist(state: &Arc<AppState>, url: &str) -> anyhow::Result<(String, usize, String)> {
    let url = url.trim().to_string();
    {
        let mut c = state.core.lock().unwrap();
        c.import_progress = 10;
        c.import_status = "Resolving playlist…".into();
    }
    let (name, mut tracks, imported_from) =
        if url.contains("spotify.com") || url.starts_with("spotify:") {
            let (name, tracks) = match ensure_spotify_token(state) {
                Some(token) => sources::spotify::playlist_tracks_api(&token, &url)
                    .or_else(|e| {
                        log::warn!("Spotify API playlist failed ({e}); using public embed");
                        sources::spotify::playlist_tracks(&url)
                    })?,
                None => sources::spotify::playlist_tracks(&url)?,
            };
            (name, tracks, "Spotify")
        } else if url.contains("soundcloud.com") {
            let (name, tracks) = sources::soundcloud::playlist_tracks(&url)?;
            (name, tracks, "SoundCloud")
        } else if url.contains("youtube.com") || url.contains("youtu.be") {
            let id = sources::youtube::extract_playlist_id(&url)
                .ok_or_else(|| anyhow::anyhow!("No playlist id found in this YouTube link"))?;
            let (name, tracks) = sources::youtube::playlist_tracks(&id)?;
            (name, tracks, "YouTube")
        } else {
            anyhow::bail!("Unsupported link. Use YouTube, SoundCloud or Spotify.");
        };
    {
        let mut c = state.core.lock().unwrap();
        c.import_progress = 45;
        c.import_status = format!("Preparing {} tracks…", tracks.len());
    }
    for (i, t) in tracks.iter_mut().enumerate() {
        if i < 16 {
            images::ensure_artwork(t);
        }
    }
    let mut ids = Vec::new();
    for t in tracks {
        ids.push(state.core.lock().unwrap().add_track(t));
    }
    let mut c = state.core.lock().unwrap();
    let mut playlist = Playlist::new(name.clone());
    playlist.track_ids = ids.clone();
    playlist.subtitle = format!("{} tracks · imported from {}", ids.len(), imported_from);
    playlist.imported_from = imported_from.to_string();
    playlist.accent = crate::model::accent_for(&playlist.id);
    let pid = playlist.id.clone();
    c.playlists.insert(0, playlist);
    c.import_progress = 100;
    c.import_status = format!("Imported {} tracks", ids.len());
    c.save_library(paths::paths());
    Ok((name, ids.len(), pid))
}
