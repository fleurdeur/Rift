#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

pub mod android;
pub mod app;
pub mod audio_presets;
pub mod images;
pub mod library;
pub mod lyrics;
pub mod model;
pub mod paths;
pub mod player;
pub mod sources;
pub mod themes;

slint::include_modules!();

pub fn run() -> anyhow::Result<()> {
    app::run()
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("Rift"),
    );
    if let Some(path) = app.internal_data_path() {
        paths::set_root(path);
    }
    if let Err(e) = slint::android::init(app) {
        log::error!("Slint init failed: {e}");
        return;
    }
    if let Err(e) = app::run() {
        log::error!("Rift failed: {e:#}");
    }
}
