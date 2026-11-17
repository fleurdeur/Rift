# Rift

**A native Android music player written in Rust + Slint.**

Rift is the Android-native continuation of the Riff music player. It plays local
lossless files, streams from YouTube and SoundCloud, imports public playlists,
and connects to Spotify through its official Web API. The whole UI is rendered
by Slint — no Electron, no WebView, no JavaScript runtime.

## Screens

- **Home** — daily mix, recent plays, recommendations, quick genres
- **Search** — Spotify, YouTube and SoundCloud search with a colour-coded browse grid
- **Library** — liked songs, local files, playlists, albums, artists, downloads
- **Player** — artwork, synced lyrics, queue, visualizer, sleep timer
- **Audio editor** — 10-band graphic EQ, compressor, brickwall limiter, bass,
  reverb, speed/pitch, normalizer, scene presets and saved profiles
- **Settings** — themes, accent colours, text size, playback quality, Spotify,
  local scanning, cache and import

## Audio

- Local FLAC, WAV, AIFF, ALAC, MP3, OGG, Opus, M4A and AAC via Rodio/Symphonia.
- 10-band peaking EQ, low-shelf bass, reverb, peak normalizer.
- Feed-forward compressor with threshold, ratio, attack, release and makeup gain.
- Transparent brickwall limiter with adjustable ceiling.
- Saved profiles store the complete signal chain, not just EQ gains.
- Lossless playback is available for local files. Spotify's official API does
  **not** expose full-length or lossless streams to third-party apps, so Spotify
  tracks play as 30-second previews unless Rift finds a matching FLAC/WAV in your
  local library. Rift does not bypass DRM or download from unauthorised sources.

## Spotify

Rift uses the supported Spotify Web API with a **Client Credentials** app:

1. Create an app at <https://developer.spotify.com/dashboard>.
2. Copy its Client ID and Client Secret into **Settings → Spotify**.
3. Tap **Connect**. Rift stores a short-lived token and refreshes it on demand.

This gives catalogue search and public-playlist import. No Spotify account
password or cookie is collected. Playback remains limited to previews and your
own local files, as required by Spotify's developer policy.

## Build

Requirements: Rust stable, Android SDK 35, Android NDK 26.1+, `cargo-ndk`,
JDK 17+.

```bash
export JAVA_HOME="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk}"
export ANDROID_HOME="$HOME/Android/Sdk"
export ANDROID_NDK_ROOT="$ANDROID_HOME/ndk/26.1.10909125"
export ANDROID_NDK="$ANDROID_NDK_ROOT"
export ANDROID_JAR="$ANDROID_HOME/platforms/android-35/android.jar"

cargo ndk -t arm64-v8a -P 26 -o jniLibs build --release
./scripts/package-android.sh
```

The packaging script uses `aapt2`, `zipalign` and `apksigner` from build-tools
35.0.0 and produces `target/Rift.apk`. Use your own release keystore for
distribution; the sample script falls back to the local debug key.

Desktop preview (development only):

```bash
cargo run --features desktop
```

## Privacy

No telemetry, no analytics, no accounts required. API credentials and session
tokens stay on the device. Build outputs are stripped and path-scrubbed before
packaging.

## Credits

Rift is a native reimplementation built on the MIT-licensed
[Riff](https://github.com/rootscripts/riff) project and inspired by the Spotify
experience of [Spotui](https://github.com/Spotui/Spotui). The audio feature set
is informed by [Equalizer314](https://github.com/bearinmindcat/Equalizer314)
(GPL-3.0); no Equalizer314 source code is included here.

Built with [Slint](https://slint.dev/), [Rodio](https://github.com/RustAudio/rodio),
[Symphonia](https://github.com/pdeljanov/Symphonia) and
[Lofty](https://github.com/Serial-ATA/lofty-rs).

## License

MIT. See [LICENSE](./LICENSE).
