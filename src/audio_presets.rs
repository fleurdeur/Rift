//! Built-in tone presets. Headphone corrections are compact, community-style
//! approximations for the 10-band graphic EQ; they are not measurement-grade
//! AutoEQ profiles and can be replaced by importing a saved Rift profile.

pub struct HeadphoneProfile {
    pub name: &'static str,
    pub sub: &'static str,
    pub gains: [f32; 10],
    pub bass: f32,
}

const fn hp(name: &'static str, sub: &'static str, gains: [f32; 10], bass: f32) -> HeadphoneProfile {
    HeadphoneProfile { name, sub, gains, bass }
}

pub const HEADPHONE_PROFILES: &[HeadphoneProfile] = &[
    hp("Flat / reference", "No correction", [0.0; 10], 0.0),
    hp("Harman target", "Balanced with sub-bass", [-1.5, -1.0, 0.0, 1.0, 2.0, 2.0, 1.0, 0.0, -1.0, -2.0], 1.0),
    hp("Studio monitor", "Neutral, dry", [-1.0, -0.5, 0.0, 0.5, 1.0, 1.0, 0.5, 0.0, -0.5, -1.0], 0.0),
    hp("Basshead", "Deep low end", [7.0, 5.5, 3.5, 1.0, -0.5, -1.0, -0.5, 0.5, 1.5, 2.0], 3.0),
    hp("Vocal clarity", "Forward mids", [-2.0, -1.5, 0.0, 2.0, 3.5, 3.0, 1.5, 0.0, -1.0, -2.0], 0.0),
    hp("Podcast / speech", "Cut rumble", [-5.0, -3.0, -1.0, 1.5, 2.5, 2.0, 0.5, -1.0, -2.0, -3.0], 0.0),
    hp("AirPods Pro", "Harman-ish in-ear", [-2.0, -1.0, 0.5, 1.5, 2.0, 1.5, 0.5, -0.5, -1.5, -2.5], 1.0),
    hp("AirPods Max", "Open, smooth", [-2.5, -1.5, 0.0, 1.0, 1.5, 1.0, 0.0, -0.5, -1.5, -2.5], 0.5),
    hp("Sony WH-1000XM5", "Warm noise-cancelling", [1.5, 2.0, 2.0, 1.0, 0.0, -0.5, -1.0, -1.0, -0.5, 0.0], 1.0),
    hp("Sony WH-1000XM4", "Warm, bass-forward", [2.5, 3.0, 2.5, 1.0, -0.5, -1.0, -1.5, -1.0, -0.5, 0.0], 1.5),
    hp("Bose QC45", "Smooth, easy", [0.5, 1.0, 1.5, 1.0, 0.5, 0.0, -0.5, -1.0, -0.5, 0.0], 0.5),
    hp("Sennheiser HD 600", "Mid-focused open", [-2.0, -1.5, -0.5, 1.0, 2.0, 2.0, 1.0, 0.0, -1.0, -2.0], 0.0),
    hp("Sennheiser HD 650", "Warm reference", [-1.0, -0.5, 0.0, 1.0, 1.5, 1.5, 0.5, 0.0, -1.0, -2.0], 0.0),
    hp("Beyerdynamic DT 770", "Bright closed-back", [1.0, 1.5, 1.0, 0.0, -1.0, -1.0, 0.0, 2.0, 3.0, 2.0], 1.0),
    hp("Beyerdynamic DT 990", "Airy, treble lift", [0.5, 1.0, 1.0, 0.0, -1.0, -0.5, 0.5, 2.5, 3.0, 1.5], 0.5),
    hp("ATH-M50x", "Punchy monitor", [2.0, 2.5, 2.0, 0.5, -1.0, -1.5, -0.5, 1.0, 2.0, 1.0], 1.5),
    hp("Moondrop Aria", "Neutral in-ear", [-1.0, 0.0, 1.0, 1.5, 1.0, 0.5, 0.0, -1.0, -1.5, -2.0], 0.5),
    hp("Truthear Hexa", "Clean in-ear", [-1.0, -0.5, 0.5, 1.0, 1.0, 0.5, 0.0, -0.5, -1.5, -2.5], 0.0),
    hp("7Hz Salnotes Zero", "Harman in-ear", [-1.0, -0.5, 0.5, 1.5, 2.0, 1.5, 0.5, -0.5, -1.5, -2.5], 1.0),
    hp("KZ ZSN Pro", "V-shaped in-ear", [2.0, 2.5, 2.0, 0.5, -1.0, -1.0, 0.0, 2.0, 3.0, 2.0], 1.5),
    hp("Galaxy Buds2 Pro", "Smooth in-ear", [0.5, 1.0, 1.5, 1.0, 0.5, 0.0, 0.0, -0.5, -1.0, -1.5], 0.5),
    hp("Nothing Ear", "Bright in-ear", [1.0, 1.5, 1.0, 0.0, -0.5, 0.0, 0.5, 1.5, 2.0, 1.0], 0.5),
    hp("Pixel Buds Pro", "Balanced in-ear", [0.0, 0.5, 1.0, 1.0, 0.5, 0.0, -0.5, 0.0, 0.5, 0.0], 0.5),
    hp("Beats Studio3", "Bass-heavy", [5.0, 4.0, 2.0, 0.0, -1.5, -1.0, 0.0, 1.0, 1.5, 0.5], 2.0),
    hp("Shure SE215", "Warm in-ear", [2.0, 2.0, 1.0, 0.5, 0.0, -0.5, -0.5, 0.5, 1.0, 0.5], 1.0),
];
