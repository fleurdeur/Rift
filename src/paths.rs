use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub cache: PathBuf,
    pub audio: PathBuf,
    pub art: PathBuf,
    pub lyrics: PathBuf,
    pub settings_file: PathBuf,
    pub library_file: PathBuf,
}

static ROOT: OnceLock<PathBuf> = OnceLock::new();
static PATHS: OnceLock<Paths> = OnceLock::new();

pub fn set_root(root: impl Into<PathBuf>) {
    let root = root.into();
    let _ = std::fs::create_dir_all(&root);
    let _ = ROOT.set(root);
}

pub fn init_root() {
    if ROOT.get().is_some() {
        return;
    }
    let root = if cfg!(target_os = "android") {
        PathBuf::from("/data/data/io.github.rootscripts.riff/files")
    } else {
        std::env::var_os("RIFF_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("APPDATA")
                    .map(|p| PathBuf::from(p).join("RiftAndroid"))
            })
            .unwrap_or_else(|| std::env::temp_dir().join("riff-android"))
    };
    let _ = std::fs::create_dir_all(&root);
    let _ = ROOT.set(root);
}

pub fn root() -> &'static Path {
    init_root();
    ROOT.get().expect("root")
}

pub fn paths() -> &'static Paths {
    PATHS.get_or_init(|| {
        let root = root().to_path_buf();
        let cache = root.join("cache");
        let audio = cache.join("audio");
        let art = cache.join("art");
        let lyrics = cache.join("lyrics");
        for d in [&cache, &audio, &art, &lyrics] {
            let _ = std::fs::create_dir_all(d);
        }
        Paths {
            settings_file: root.join("settings.json"),
            library_file: root.join("library.json"),
            root,
            cache,
            audio,
            art,
            lyrics,
        }
    })
}

pub fn cache_size_label() -> String {
    fn dir_size(p: &Path) -> u64 {
        let mut total = 0;
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    total += dir_size(&path);
                } else if let Ok(m) = e.metadata() {
                    total += m.len();
                }
            }
        }
        total
    }
    let bytes = dir_size(&paths().cache);
    if bytes > 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{} KB", bytes / 1024)
    }
}

pub fn clear_cache() -> std::io::Result<()> {
    let paths = paths();
    for d in [&paths.audio, &paths.art, &paths.lyrics] {
        if d.exists() {
            std::fs::remove_dir_all(d)?;
        }
        std::fs::create_dir_all(d)?;
    }
    Ok(())
}

pub fn local_music_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(target_os = "android") {
        for p in [
            "/storage/emulated/0/Music",
            "/storage/emulated/0/Download",
            "/storage/emulated/0/Ringtones",
            "/storage/emulated/0/Android/media",
            "/sdcard/Music",
        ] {
            let path = PathBuf::from(p);
            if path.exists() {
                dirs.push(path);
            }
        }
    } else {
        if let Some(home) = std::env::var_os("USERPROFILE") {
            let p = PathBuf::from(home).join("Music");
            if p.exists() {
                dirs.push(p);
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            let p = PathBuf::from(home).join("Music");
            if p.exists() {
                dirs.push(p);
            }
        }
    }
    // App-specific external folder is always readable.
    if let Some(ext) = external_data_dir() {
        let p = ext.join("Music");
        let _ = std::fs::create_dir_all(&p);
        dirs.push(p);
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

#[cfg(target_os = "android")]
pub fn external_data_dir() -> Option<PathBuf> {
    crate::android::external_files_dir()
}

#[cfg(not(target_os = "android"))]
pub fn external_data_dir() -> Option<PathBuf> {
    None
}
