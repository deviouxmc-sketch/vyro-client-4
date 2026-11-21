//! Hardware-aware performance presets. These are *starting points*: RAM and JVM/video settings
//! that are sensible for a class of machine. VYRO never promises an FPS gain; the in-game mod
//! records real sessions so players can compare their own numbers (see `PerfSession`).
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Low,
    Balanced,
    High,
}

#[derive(Debug, Clone, Copy)]
pub struct Hardware {
    pub total_mem_mb: u64,
    pub cpu_threads: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub preset: Preset,
    pub ram_mb: u32,
    pub jvm_args: Vec<String>,
    /// options.txt key/value pairs
    pub video: Vec<(String, String)>,
    pub notes: Vec<String>,
}

pub fn suggest_preset(hw: &Hardware) -> Preset {
    if hw.total_mem_mb <= 8 * 1024 || hw.cpu_threads <= 4 {
        Preset::Low
    } else if hw.total_mem_mb >= 24 * 1024 && hw.cpu_threads >= 12 {
        Preset::High
    } else {
        Preset::Balanced
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

pub fn recommend(hw: &Hardware, preset: Preset, modded: bool) -> Recommendation {
    let base: u32 = match (preset, modded) {
        (Preset::Low, false) => 2048,
        (Preset::Low, true) => 3072,
        (Preset::Balanced, false) => 4096,
        (Preset::Balanced, true) => 5120,
        (Preset::High, false) => 6144,
        (Preset::High, true) => 8192,
    };
    // never hand the game more than 60% of physical RAM: the OS and browser need the rest
    let cap = (((hw.total_mem_mb as f64) * 0.6) / 512.0).floor() as u32 * 512;
    let ram_mb = base.min(cap).max(1024);

    let mut notes = Vec::new();
    if cap < base {
        notes.push(format!("RAM is capped at 60% of your {:.1} GB so the system keeps enough memory.", hw.total_mem_mb as f64 / 1024.0));
    }
    if hw.total_mem_mb < 6 * 1024 {
        notes.push("This computer has very little RAM; close other programs while playing.".into());
    }
    notes.push("These are starting points. Play a session, then compare the numbers in the table below.".into());

    let (jvm_args, video) = match preset {
        Preset::Low => (
            args(&["-XX:+UseG1GC", "-XX:+ParallelRefProcEnabled", "-XX:+DisableExplicitGC", "-XX:MaxGCPauseMillis=100", "-XX:G1NewSizePercent=20", "-XX:G1ReservePercent=20"]),
            pairs(&[("renderDistance", "6"), ("simulationDistance", "5"), ("maxFps", "120"), ("particles", "2"), ("entityShadows", "false"), ("biomeBlendRadius", "0"), ("mipmapLevels", "0")]),
        ),
        Preset::Balanced => (
            args(&["-XX:+UseG1GC", "-XX:+ParallelRefProcEnabled", "-XX:+DisableExplicitGC", "-XX:MaxGCPauseMillis=60", "-XX:G1NewSizePercent=30", "-XX:G1ReservePercent=20"]),
            pairs(&[("renderDistance", "10"), ("simulationDistance", "8"), ("maxFps", "144"), ("particles", "1"), ("entityShadows", "true"), ("biomeBlendRadius", "2"), ("mipmapLevels", "2")]),
        ),
        Preset::High => (
            args(&["-XX:+UseG1GC", "-XX:+ParallelRefProcEnabled", "-XX:+DisableExplicitGC", "-XX:MaxGCPauseMillis=40", "-XX:G1NewSizePercent=30", "-XX:G1HeapRegionSize=16M", "-XX:G1ReservePercent=20"]),
            pairs(&[("renderDistance", "16"), ("simulationDistance", "12"), ("maxFps", "260"), ("particles", "0"), ("entityShadows", "true"), ("biomeBlendRadius", "5"), ("mipmapLevels", "4")]),
        ),
    };
    Recommendation { preset, ram_mb, jvm_args, video, notes }
}

/// Merge key:value pairs into `<instance>/options.txt`, keeping every other line untouched.
pub fn apply_video_options(instance_dir: &Path, video: &[(String, String)]) -> Result<()> {
    std::fs::create_dir_all(instance_dir)?;
    let path = instance_dir.join("options.txt");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = existing.lines().map(String::from).collect();
    for (k, v) in video {
        let prefix = format!("{k}:");
        match lines.iter_mut().find(|l| l.starts_with(&prefix)) {
            Some(l) => *l = format!("{k}:{v}"),
            None => lines.push(format!("{k}:{v}")),
        }
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, lines.join("\n") + "\n")?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

// ------------------------------------------------------------------ recorded play sessions

/// One play session as written by the in-game mod to `<instance>/config/vyro/perf.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PerfSession {
    pub started_at: u64,
    pub duration_sec: u64,
    pub avg_fps: f64,
    pub low1_fps: f64,
    pub max_heap_mb: u64,
    pub mods: u32,
    pub mc_version: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PerfFile {
    sessions: Vec<PerfSession>,
}

/// Tolerant reader (missing/corrupt file = no sessions), newest first.
pub fn read_sessions(path: &Path) -> Vec<PerfSession> {
    let Ok(bytes) = std::fs::read(path) else { return vec![] };
    let Ok(f) = serde_json::from_slice::<PerfFile>(&bytes) else { return vec![] };
    let mut s = f.sessions;
    s.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(mem_gb: u64, threads: usize) -> Hardware {
        Hardware { total_mem_mb: mem_gb * 1024, cpu_threads: threads }
    }

    #[test]
    fn suggests_by_hardware() {
        assert_eq!(suggest_preset(&hw(8, 8)), Preset::Low);
        assert_eq!(suggest_preset(&hw(16, 4)), Preset::Low);
        assert_eq!(suggest_preset(&hw(16, 8)), Preset::Balanced);
        assert_eq!(suggest_preset(&hw(32, 16)), Preset::High);
        assert_eq!(suggest_preset(&hw(32, 8)), Preset::Balanced);
    }

    #[test]
    fn ram_is_capped_to_sixty_percent_and_rounded() {
        let r = recommend(&hw(8, 8), Preset::High, true);
        assert_eq!(r.ram_mb, 4608); // 60% of 8192 = 4915 -> 4608
        assert!(r.notes.iter().any(|n| n.contains("capped")));
        let r = recommend(&hw(32, 16), Preset::High, true);
        assert_eq!(r.ram_mb, 8192);
        assert!(!r.notes.iter().any(|n| n.contains("capped")));
        let tiny = recommend(&hw(2, 2), Preset::Low, false);
        assert_eq!(tiny.ram_mb, 1024); // never below 1 GB
        assert!(tiny.notes.iter().any(|n| n.contains("very little RAM")));
    }

    #[test]
    fn modded_gets_more_ram() {
        let h = hw(32, 16);
        assert!(recommend(&h, Preset::Balanced, true).ram_mb > recommend(&h, Preset::Balanced, false).ram_mb);
    }

    #[test]
    fn presets_scale_video_settings() {
        let h = hw(16, 8);
        let get = |p: Preset, key: &str| -> u32 {
            recommend(&h, p, false).video.iter().find(|(k, _)| k == key).unwrap().1.parse().unwrap()
        };
        assert!(get(Preset::Low, "renderDistance") < get(Preset::Balanced, "renderDistance"));
        assert!(get(Preset::Balanced, "renderDistance") < get(Preset::High, "renderDistance"));
        assert!(recommend(&h, Preset::Low, false).jvm_args.contains(&"-XX:+UseG1GC".to_string()));
    }

    #[test]
    fn options_txt_merge_keeps_other_lines() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("options.txt"), "version:4000\nrenderDistance:12\nlang:en_us\n").unwrap();
        apply_video_options(td.path(), &[("renderDistance".into(), "6".into()), ("maxFps".into(), "120".into())]).unwrap();
        let out = std::fs::read_to_string(td.path().join("options.txt")).unwrap();
        assert_eq!(out, "version:4000\nrenderDistance:6\nlang:en_us\nmaxFps:120\n");
        // missing file is created
        let td2 = tempfile::tempdir().unwrap();
        apply_video_options(&td2.path().join("new"), &[("maxFps".into(), "60".into())]).unwrap();
        assert_eq!(std::fs::read_to_string(td2.path().join("new/options.txt")).unwrap(), "maxFps:60\n");
    }

    #[test]
    fn reads_sessions_tolerantly() {
        let td = tempfile::tempdir().unwrap();
        let f = td.path().join("perf.json");
        assert!(read_sessions(&f).is_empty());
        std::fs::write(&f, "not json").unwrap();
        assert!(read_sessions(&f).is_empty());
        std::fs::write(&f, r#"{"sessions":[{"startedAt":100,"durationSec":600,"avgFps":143.5,"low1Fps":88.0,"maxHeapMb":4096,"mods":12,"mcVersion":"1.21.11"},{"startedAt":200,"avgFps":90}]}"#).unwrap();
        let s = read_sessions(&f);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].started_at, 200); // newest first
        assert_eq!(s[0].duration_sec, 0); // missing fields default
        assert_eq!(s[1].mods, 12);
    }
}
