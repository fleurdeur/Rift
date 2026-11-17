use rodio::{Decoder, Player, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct PlayerHandle {
    tx: Sender<Cmd>,
    pub snapshot: Arc<Mutex<Snapshot>>,
}

pub enum Cmd {
    Play {
        path: PathBuf,
        duration: u64,
        start_ms: u64,
    },
    Pause,
    Resume,
    Toggle,
    Stop,
    Seek(u64),
    Volume(f32),
    Speed(f32),
    Eq([f32; 10]),
    Bass(f32),
    Reverb(f32),
    Normalizer(bool),
    Dynamics(Dynamics),
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub playing: bool,
    pub loading: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub ended: bool,
    pub error: Option<String>,
    pub spectrum: [f32; 32],
    pub silent: bool,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            playing: false,
            loading: false,
            position_ms: 0,
            duration_ms: 0,
            ended: false,
            error: None,
            spectrum: [0.0; 32],
            silent: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Dynamics {
    pub compressor: bool,
    pub threshold: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup: f32,
    pub limiter: bool,
    pub ceiling: f32,
    pub gate: bool,
    pub gate_threshold: f32,
    pub gate_release_ms: f32,
    pub mbc: bool,
    pub mbc_low_threshold: f32,
    pub mbc_low_ratio: f32,
    pub mbc_mid_threshold: f32,
    pub mbc_mid_ratio: f32,
    pub mbc_high_threshold: f32,
    pub mbc_high_ratio: f32,
}

impl Default for Dynamics {
    fn default() -> Self {
        Self {
            compressor: false,
            threshold: -18.0,
            ratio: 3.0,
            attack_ms: 12.0,
            release_ms: 120.0,
            makeup: 2.0,
            limiter: true,
            ceiling: -1.0,
            gate: false,
            gate_threshold: -55.0,
            gate_release_ms: 160.0,
            mbc: false,
            mbc_low_threshold: -20.0,
            mbc_low_ratio: 2.5,
            mbc_mid_threshold: -18.0,
            mbc_mid_ratio: 2.0,
            mbc_high_threshold: -16.0,
            mbc_high_ratio: 2.0,
        }
    }
}

#[derive(Clone)]
struct FilterSettings {
    gains: [f32; 10],
    bass: f32,
    reverb: f32,
    normalizer: bool,
    volume: f32,
    speed: f32,
    dynamics: Dynamics,
}

pub fn spawn() -> PlayerHandle {
    let (tx, rx) = mpsc::channel();
    let snapshot = Arc::new(Mutex::new(Snapshot::default()));
    let snap = snapshot.clone();
    std::thread::Builder::new()
        .name("riff-audio".into())
        .spawn(move || audio_thread(rx, snap))
        .ok();
    PlayerHandle { tx, snapshot }
}

impl PlayerHandle {
    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }
}

fn audio_thread(rx: Receiver<Cmd>, snap: Arc<Mutex<Snapshot>>) {
    let output = rodio::DeviceSinkBuilder::open_default_sink();
    let (handle, player, silent) = match output {
        Ok(h) => {
            let p = Player::connect_new(&h.mixer());
            (Some(h), Some(p), false)
        }
        Err(e) => {
            log::warn!("audio output unavailable: {e}");
            (None, None, true)
        }
    };
    let _ = handle;

    if silent {
        if let Ok(mut s) = snap.lock() {
            s.silent = true;
            s.error = Some("No audio output – visual playback only".into());
        }
    }

    let filters = Arc::new(Mutex::new(FilterSettings {
        gains: [0.0; 10],
        bass: 0.0,
        reverb: 0.0,
        normalizer: false,
        volume: 0.8,
        speed: 1.0,
        dynamics: Dynamics::default(),
    }));
    let spectrum = Arc::new(Mutex::new([0.0f32; 32]));

    let mut current: Option<(PathBuf, u64, u64)> = None; // path, duration, start
    let mut started = Instant::now();
    let mut paused_at: Option<Instant> = None;
    let mut paused_total = Duration::ZERO;
    let mut ended_sent = false;
    let mut sim_paused = false;

    loop {
        let timeout = Duration::from_millis(80);
        match rx.recv_timeout(timeout) {
            Ok(cmd) => {
                match cmd {
                    Cmd::Play {
                        path,
                        duration,
                        start_ms,
                    } => {
                        ended_sent = false;
                        sim_paused = false;
                        paused_total = Duration::ZERO;
                        paused_at = None;
                        started = Instant::now();
                        current = Some((path.clone(), duration, start_ms));
                        if let Ok(mut s) = snap.lock() {
                            s.loading = false;
                            s.playing = true;
                            s.ended = false;
                            s.error = None;
                            s.position_ms = start_ms;
                            s.duration_ms = duration * 1000;
                            s.spectrum = [0.0; 32];
                        }
                        if let Some(p) = player.as_ref() {
                            p.stop();
                            p.clear();
                            p.set_volume(filters.lock().map(|f| f.volume).unwrap_or(0.8));
                            p.set_speed(filters.lock().map(|f| f.speed).unwrap_or(1.0));
                            match open_source(&path, &filters, &spectrum, duration, start_ms) {
                                Ok(source) => {
                                    p.append(source);
                                    p.play();
                                }
                                Err(e) => {
                                    if let Ok(mut s) = snap.lock() {
                                        s.error = Some(format!("Could not play: {e}"));
                                        s.playing = false;
                                        s.loading = false;
                                    }
                                }
                            }
                        }
                    }
                    Cmd::Pause => {
                        if let Some(p) = player.as_ref() {
                            p.pause();
                        }
                        if paused_at.is_none() {
                            paused_at = Some(Instant::now());
                        }
                        sim_paused = true;
                        if let Ok(mut s) = snap.lock() {
                            s.playing = false;
                        }
                    }
                    Cmd::Resume => {
                        if let Some(p) = player.as_ref() {
                            p.play();
                        }
                        if let Some(t) = paused_at.take() {
                            paused_total += t.elapsed();
                        }
                        sim_paused = false;
                        if let Ok(mut s) = snap.lock() {
                            s.playing = true;
                        }
                    }
                    Cmd::Toggle => {
                        if player.as_ref().map(|p| p.is_paused()).unwrap_or(sim_paused) {
                            if let Some(p) = player.as_ref() {
                                p.play();
                            }
                            if let Some(t) = paused_at.take() {
                                paused_total += t.elapsed();
                            }
                            sim_paused = false;
                            if let Ok(mut s) = snap.lock() {
                                s.playing = true;
                            }
                        } else {
                            if let Some(p) = player.as_ref() {
                                p.pause();
                            }
                            if paused_at.is_none() {
                                paused_at = Some(Instant::now());
                            }
                            sim_paused = true;
                            if let Ok(mut s) = snap.lock() {
                                s.playing = false;
                            }
                        }
                    }
                    Cmd::Stop => {
                        if let Some(p) = player.as_ref() {
                            p.stop();
                            p.clear();
                        }
                        current = None;
                        if let Ok(mut s) = snap.lock() {
                            s.playing = false;
                            s.position_ms = 0;
                            s.spectrum = [0.0; 32];
                        }
                    }
                    Cmd::Seek(pos) => {
                        if let Some((path, duration, _)) = current.clone() {
                            ended_sent = false;
                            paused_total = Duration::ZERO;
                            started = Instant::now();
                            if let Some(p) = player.as_ref() {
                                p.stop();
                                p.clear();
                                match open_source(&path, &filters, &spectrum, duration, pos) {
                                    Ok(source) => {
                                        p.append(source);
                                        p.play();
                                    }
                                    Err(e) => {
                                        log::warn!("seek failed: {e}");
                                    }
                                }
                            }
                            current = Some((path, duration, pos));
                            if let Ok(mut s) = snap.lock() {
                                s.position_ms = pos * 1000;
                                s.ended = false;
                                if !sim_paused {
                                    s.playing = true;
                                }
                            }
                        }
                    }
                    Cmd::Volume(v) => {
                        if let Ok(mut f) = filters.lock() {
                            f.volume = v;
                        }
                        if let Some(p) = player.as_ref() {
                            p.set_volume(v);
                        }
                    }
                    Cmd::Speed(s) => {
                        if let Ok(mut f) = filters.lock() {
                            f.speed = s;
                        }
                        if let Some(p) = player.as_ref() {
                            p.set_speed(s);
                        }
                    }
                    Cmd::Eq(g) => {
                        if let Ok(mut f) = filters.lock() {
                            f.gains = g;
                        }
                    }
                    Cmd::Bass(b) => {
                        if let Ok(mut f) = filters.lock() {
                            f.bass = b;
                        }
                    }
                    Cmd::Reverb(r) => {
                        if let Ok(mut f) = filters.lock() {
                            f.reverb = r;
                        }
                    }
                    Cmd::Normalizer(n) => {
                        if let Ok(mut f) = filters.lock() {
                            f.normalizer = n;
                        }
                    }
                    Cmd::Dynamics(d) => {
                        if let Ok(mut f) = filters.lock() {
                            f.dynamics = d;
                        }
                    }
                    Cmd::Shutdown => break,
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Update the snapshot
        if let Some((_, duration, start_ms)) = current.as_ref() {
            let mut pos = if let Some(p) = player.as_ref() {
                start_ms + p.get_pos().as_millis() as u64
            } else {
                let elapsed = started.elapsed().saturating_sub(paused_total);
                start_ms + elapsed.as_millis() as u64
            };
            if sim_paused {
                pos = if let Ok(s) = snap.lock() {
                    s.position_ms
                } else {
                    pos
                };
            }
            let dur_ms = *duration * 1000;
            if dur_ms > 0 && pos >= dur_ms + 250 {
                pos = dur_ms;
            }
            let done = player
                .as_ref()
                .map(|p| p.empty() && !p.is_paused())
                .unwrap_or(false)
                || (dur_ms > 0 && pos >= dur_ms);
            let spectrum_now = *spectrum.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(mut s) = snap.lock() {
                s.position_ms = pos;
                s.duration_ms = dur_ms;
                s.spectrum = spectrum_now;
                if done && !ended_sent {
                    s.ended = true;
                    s.playing = false;
                    ended_sent = true;
                }
            }
        } else {
            let spectrum_now = *spectrum.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(mut s) = snap.lock() {
                s.spectrum = spectrum_now;
            }
        }
    }
}

fn open_source(
    path: &std::path::Path,
    filters: &Arc<Mutex<FilterSettings>>,
    spectrum: &Arc<Mutex<[f32; 32]>>,
    _duration: u64,
    start_ms: u64,
) -> anyhow::Result<EqSource> {
    let file = File::open(path)?;
    let decoder = Decoder::try_from(BufReader::new(file))?;
    let source = if start_ms > 0 {
        EqSource::new(decoder.skip_duration(Duration::from_millis(start_ms)), filters.clone(), spectrum.clone())
    } else {
        EqSource::new(decoder, filters.clone(), spectrum.clone())
    };
    Ok(source)
}

#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn peaking(freq: f32, q: f32, gain_db: f32, sr: f32) -> Self {
        if gain_db.abs() < 0.05 {
            return Self::identity();
        }
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / sr;
        let alpha = w0.sin() / (2.0 * q);
        let cos = w0.cos();
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos;
        let a2 = 1.0 - alpha / a;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn low_shelf(freq: f32, gain_db: f32, sr: f32) -> Self {
        if gain_db.abs() < 0.05 {
            return Self::identity();
        }
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / sr;
        let cos = w0.cos();
        let sin = w0.sin();
        let alpha = sin / 2.0 * (2.0f32).sqrt();
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        let a0 = (a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha;
        let b0 = a * ((a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha);
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos);
        let a2 = (a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

pub struct EqSource {
    inner: Box<dyn Source<Item = f32> + Send>,
    filters: Arc<Mutex<FilterSettings>>,
    spectrum: Arc<Mutex<[f32; 32]>>,
    bands: Vec<Vec<Biquad>>,
    bass: Vec<Biquad>,
    channels: usize,
    sample_rate: f32,
    frame: usize,
    chan: usize,
    reverb_buf: Vec<Vec<f32>>,
    reverb_pos: Vec<usize>,
    reverb_len: usize,
    peak: f32,
    fft_buf: Vec<f32>,
    fft_pos: usize,
    smooth: [f32; 32],
    goertzel_coeffs: [f32; 32],
    comp_env: f32,
    comp_gain: f32,
    lim_env: f32,
    gate_env: f32,
    gate_gain: f32,
    low_lp: f32,
    high_lp: f32,
    mbc_env: [f32; 3],
    mbc_gain: [f32; 3],
    dyn_state: Dynamics,
}

impl EqSource {
    fn new<S>(inner: S, filters: Arc<Mutex<FilterSettings>>, spectrum: Arc<Mutex<[f32; 32]>>) -> Self
    where
        S: Source<Item = f32> + Send + 'static,
    {
        let channels = inner.channels().get() as usize;
        let sample_rate = inner.sample_rate().get() as f32;
        let gains = filters.lock().map(|f| f.gains).unwrap_or([0.0; 10]);
        let bass_gain = filters.lock().map(|f| f.bass).unwrap_or(0.0);
        let bands = (0..channels)
            .map(|_| make_bands(&gains, sample_rate))
            .collect();
        let bass = (0..channels)
            .map(|_| Biquad::low_shelf(110.0, bass_gain, sample_rate))
            .collect();
        let reverb_len = (sample_rate * 0.085) as usize;
        let reverb_buf = (0..channels)
            .map(|_| vec![0.0f32; reverb_len.max(1)])
            .collect();
        let mut goertzel_coeffs = [0.0f32; 32];
        for i in 0..32 {
            let t = i as f32 / 31.0;
            let freq = 55.0 * (220.0f32).powf(t); // 55 Hz .. 12.1 kHz
            goertzel_coeffs[i] = 2.0 * std::f32::consts::PI * freq / sample_rate;
        }
        Self {
            inner: Box::new(inner),
            filters,
            spectrum,
            bands,
            bass,
            channels,
            sample_rate,
            frame: 0,
            chan: 0,
            reverb_buf,
            reverb_pos: vec![0; channels],
            reverb_len: reverb_len.max(1),
            peak: 0.001,
            fft_buf: vec![0.0; 1024],
            fft_pos: 0,
            smooth: [0.0; 32],
            goertzel_coeffs,
            comp_env: 0.0,
            comp_gain: 1.0,
            lim_env: 0.0,
            gate_env: 0.0,
            gate_gain: 1.0,
            low_lp: 0.0,
            high_lp: 0.0,
            mbc_env: [0.0; 3],
            mbc_gain: [1.0; 3],
            dyn_state: Dynamics::default(),
        }
    }

    fn update_bands(&mut self) {
        let Ok(f) = self.filters.lock() else { return };
        let gains = f.gains;
        let bass = f.bass;
        if gains != self.current_gains() {
            for ch in 0..self.channels {
                self.bands[ch] = make_bands(&gains, self.sample_rate);
                self.bass[ch] = Biquad::low_shelf(110.0, bass, self.sample_rate);
            }
        } else if (bass - self.current_bass()).abs() > 0.05 {
            for ch in 0..self.channels {
                self.bass[ch] = Biquad::low_shelf(110.0, bass, self.sample_rate);
            }
        }
    }

    fn current_gains(&self) -> [f32; 10] {
        self.filters.lock().map(|f| f.gains).unwrap_or([0.0; 10])
    }
    fn current_bass(&self) -> f32 {
        self.filters.lock().map(|f| f.bass).unwrap_or(0.0)
    }

    fn push_spectrum(&mut self, sample: f32) {
        if self.channels > 1 {
            // use the first channel only
            if self.frame % self.channels != 0 {
                return;
            }
        }
        self.fft_buf[self.fft_pos] = sample;
        self.fft_pos += 1;
        if self.fft_pos < self.fft_buf.len() {
            return;
        }
        self.fft_pos = 0;
        let mut out = [0.0f32; 32];
        for (i, coeff) in self.goertzel_coeffs.iter().enumerate() {
            let mut s1 = 0.0f32;
            let mut s2 = 0.0f32;
            for &x in &self.fft_buf {
                let s0 = coeff * s1 - s2 + x;
                s2 = s1;
                s1 = s0;
            }
            let power = (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0);
            let mag = (power.sqrt() / 256.0).min(1.0);
            out[i] = mag;
        }
        for i in 0..32 {
            self.smooth[i] = (self.smooth[i] * 0.55 + out[i] * 0.9).min(1.0);
        }
        if let Ok(mut spec) = self.spectrum.lock() {
            *spec = self.smooth;
        }
    }
}

fn make_bands(gains: &[f32; 10], sr: f32) -> Vec<Biquad> {
    let freqs = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
    freqs
        .iter()
        .zip(gains.iter())
        .map(|(f, g)| Biquad::peaking(f32::min(*f, sr * 0.45), 1.25, *g, sr))
        .collect()
}

impl Iterator for EqSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        if self.frame % 512 == 0 {
            self.update_bands();
        }
        let ch = self.chan % self.channels.max(1);
        let mut x = sample;
        // Bass shelf first
        if let Some(b) = self.bass.get_mut(ch) {
            x = b.process(x);
        }
        if let Some(chain) = self.bands.get_mut(ch) {
            for b in chain.iter_mut() {
                x = b.process(x);
            }
        }
        let (reverb_amt, normalizer) = self
            .filters
            .lock()
            .map(|f| (f.reverb, f.normalizer))
            .unwrap_or((0.0, false));
        if reverb_amt > 0.001 {
            let buf = &mut self.reverb_buf[ch];
            let pos = self.reverb_pos[ch];
            let delayed = buf[pos];
            buf[pos] = x + delayed * 0.42;
            self.reverb_pos[ch] = (pos + 1) % self.reverb_len;
            x = x * (1.0 - reverb_amt * 0.35) + delayed * reverb_amt * 0.5;
        }
        // Compressor + limiter (Equalizer314-style dynamics chain).
        let dyn_settings = self
            .filters
            .lock()
            .map(|f| f.dynamics)
            .unwrap_or_default();
        if dyn_settings.gate {
            let env_abs = x.abs();
            let atk = (-1.0 / (0.008 * self.sample_rate)).exp();
            let rel = (-1.0 / (dyn_settings.gate_release_ms.max(20.0) * 0.001 * self.sample_rate)).exp();
            let coef = if env_abs > self.gate_env { atk } else { rel };
            self.gate_env = coef * self.gate_env + (1.0 - coef) * env_abs;
            let thr = 10f32.powf(dyn_settings.gate_threshold.min(0.0) / 20.0);
            let target = if self.gate_env < thr { 0.0 } else { 1.0 };
            let gcoef = if target < self.gate_gain { atk } else { rel };
            self.gate_gain = gcoef * self.gate_gain + (1.0 - gcoef) * target;
            x *= self.gate_gain;
        } else {
            self.gate_gain = 1.0;
        }
        if dyn_settings.mbc {
            // Three-band Linkwitz-Riley-ish split with per-band compression.
            let a_low = 1.0 - (-2.0 * std::f32::consts::PI * 200.0 / self.sample_rate).exp();
            let a_high = 1.0 - (-2.0 * std::f32::consts::PI * 2000.0 / self.sample_rate).exp();
            self.low_lp += a_low * (x - self.low_lp);
            self.high_lp += a_high * (x - self.high_lp);
            let low = self.low_lp;
            let mid = self.high_lp - self.low_lp;
            let high = x - self.high_lp;
            let bands = [low, mid, high];
            let params = [
                (dyn_settings.mbc_low_threshold, dyn_settings.mbc_low_ratio),
                (dyn_settings.mbc_mid_threshold, dyn_settings.mbc_mid_ratio),
                (dyn_settings.mbc_high_threshold, dyn_settings.mbc_high_ratio),
            ];
            let mut sum = 0.0;
            for i in 0..3 {
                let b = bands[i];
                let (thr, ratio) = params[i];
                let env_abs = b.abs();
                let atk = (-1.0 / (0.012 * self.sample_rate)).exp();
                let rel = (-1.0 / (0.12 * self.sample_rate)).exp();
                let coef = if env_abs > self.mbc_env[i] { atk } else { rel };
                self.mbc_env[i] = coef * self.mbc_env[i] + (1.0 - coef) * env_abs;
                let env_db = 20.0 * self.mbc_env[i].max(1e-6).log10();
                let over = env_db - thr;
                let target_db = if over > 0.0 { -over * (1.0 - 1.0 / ratio.max(1.0)) } else { 0.0 };
                let target = 10f32.powf(target_db / 20.0);
                let gcoef = if target < self.mbc_gain[i] { atk } else { rel };
                self.mbc_gain[i] = gcoef * self.mbc_gain[i] + (1.0 - gcoef) * target;
                sum += b * self.mbc_gain[i];
            }
            x = sum;
        }
        if dyn_settings.compressor {
            let env_abs = x.abs();
            let attack = (-1.0 / (dyn_settings.attack_ms.max(0.5) * 0.001 * self.sample_rate)).exp();
            let release = (-1.0 / (dyn_settings.release_ms.max(5.0) * 0.001 * self.sample_rate)).exp();
            let coef = if env_abs > self.comp_env { attack } else { release };
            self.comp_env = coef * self.comp_env + (1.0 - coef) * env_abs;
            let env_db = 20.0 * self.comp_env.max(1e-6).log10();
            let over = env_db - dyn_settings.threshold;
            let target_db = if over > 0.0 {
                -over * (1.0 - 1.0 / dyn_settings.ratio.max(1.0))
            } else {
                0.0
            };
            let target_gain = 10f32.powf(target_db / 20.0);
            let gcoef = if target_gain < self.comp_gain { attack } else { release };
            self.comp_gain = gcoef * self.comp_gain + (1.0 - gcoef) * target_gain;
            x *= self.comp_gain * 10f32.powf(dyn_settings.makeup / 20.0);
        } else {
            self.comp_env = 0.0;
            self.comp_gain = 1.0;
        }
        if dyn_settings.limiter {
            let ceil = 10f32.powf(dyn_settings.ceiling.min(0.0) / 20.0);
            let peak = x.abs();
            if peak > self.lim_env {
                self.lim_env = peak;
            } else {
                let rel = (-1.0 / (0.08 * self.sample_rate)).exp();
                self.lim_env = rel * self.lim_env + (1.0 - rel) * peak;
            }
            if self.lim_env > ceil && self.lim_env > 1e-6 {
                x *= ceil / self.lim_env;
            }
        } else {
            self.lim_env = 0.0;
        }
        self.dyn_state = dyn_settings;
        let mut gain = 0.82;
        if normalizer {
            let abs = x.abs();
            if abs > self.peak {
                self.peak = abs;
            } else {
                self.peak = (self.peak * 0.9995).max(0.02);
            }
            gain *= (0.85 / self.peak).clamp(0.6, 2.4);
        }
        x = (x * gain).clamp(-1.0, 1.0);
        self.push_spectrum(x);
        self.chan = (self.chan + 1) % self.channels.max(1);
        self.frame += 1;
        Some(x)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl Source for EqSource {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)
    }
}
