//! Crash report discovery, parsing and plain-language diagnosis.
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Hint {
    pub title: String,
    pub advice: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CrashSummary {
    pub description: String,
    pub time: String,
    pub exception: String,
    /// First lines of the stack trace.
    pub head: String,
    pub suspected: Vec<String>,
    pub hints: Vec<Hint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportMeta {
    pub name: String,
    pub modified_ms: u64,
}

/// Only plain `crash-*.txt` names, so a name coming from the UI can never point outside the folder.
pub fn is_safe_report_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 120
        && !name.contains(['/', '\\', '\0'])
        && !name.contains("..")
        && name.starts_with("crash-")
        && name.ends_with(".txt")
}

fn reports_dir(instance: &Path) -> PathBuf {
    instance.join("crash-reports")
}

pub fn list_reports(instance: &Path) -> Vec<ReportMeta> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(reports_dir(instance)) else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !is_safe_report_name(&name) {
            continue;
        }
        let modified_ms = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        out.push(ReportMeta { name, modified_ms });
    }
    out.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    out
}

/// Newest report written at or after `since_ms` (with a little slack for clock rounding).
pub fn newest_since(instance: &Path, since_ms: u64) -> Option<ReportMeta> {
    list_reports(instance).into_iter().find(|r| r.modified_ms + 2000 >= since_ms)
}

pub fn read_report(instance: &Path, name: &str) -> Option<String> {
    if !is_safe_report_name(name) {
        return None;
    }
    let bytes = std::fs::read(reports_dir(instance).join(name)).ok()?;
    let cut = bytes.len().min(2 * 1024 * 1024);
    Some(String::from_utf8_lossy(&bytes[..cut]).to_string())
}

/// Last `n` lines of the newest game log (`logs/latest.log`).
pub fn log_tail(instance: &Path, n: usize) -> String {
    let path = instance.join("logs").join("latest.log");
    let Ok(bytes) = std::fs::read(&path) else {
        return String::new();
    };
    let start = bytes.len().saturating_sub(512 * 1024);
    let text = String::from_utf8_lossy(&bytes[start..]).to_string();
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(n);
    lines[from..].join("\n")
}

fn first_token_after(text: &str, marker: &str) -> Option<String> {
    let i = text.find(marker)? + marker.len();
    let tok: String = text[i..].chars().take_while(|c| !c.is_whitespace() && *c != ',' && *c != ':').collect();
    if tok.is_empty() { None } else { Some(tok) }
}

pub fn diagnose(text: &str) -> Vec<Hint> {
    let mut hints = Vec::new();
    let has = |s: &str| text.contains(s);
    let mut push = |t: &str, a: String| hints.push(Hint { title: t.to_string(), advice: a });

    if has("java.lang.OutOfMemoryError") {
        push("The game ran out of memory", "Give this profile more RAM (Performance page, or Profiles → Edit) and remove very heavy mods or high render distance.".into());
    }
    if has("UnsupportedClassVersionError") || has("class file version") {
        push("Wrong Java version", "A mod or Minecraft needs a newer (or older) Java than the one used. Leave the Java setting on automatic so VYRO picks the right one.".into());
    }
    if has("Incompatible mod set") || has("Mod resolution failed") || has("requires version") || has("which is missing") {
        let lines: Vec<String> = text
            .lines()
            .filter(|l| l.to_lowercase().contains("requires") && l.contains("mod '"))
            .map(|l| l.trim().trim_start_matches('-').trim().to_string())
            .take(4)
            .collect();
        let detail = if lines.is_empty() { String::new() } else { format!(" {}", lines.join(" | ")) };
        push("A mod is missing a dependency or is the wrong version", format!("Install the missing mod (often Fabric API) or update/remove the mod that complains.{detail}"));
    }
    if has("Mixin apply for mod") || has("MixinApplyError") || has("InvalidInjectionException") || has("InjectionError") {
        let who = first_token_after(text, "Mixin apply for mod ").map(|m| format!(" Mod involved: {m}.")).unwrap_or_default();
        push("A mod could not hook into Minecraft", format!("This usually means a mod version does not match this Minecraft version, or two mods conflict. Update the mod or remove it.{who}"));
    }
    if has("NoSuchMethodError") || has("NoSuchFieldError") || has("NoClassDefFoundError") || has("ClassNotFoundException") {
        push("A mod does not match this Minecraft/loader version", "A mod is calling code that doesn't exist here. Check each mod supports this exact Minecraft version, and update or remove the odd one out.".into());
    }
    if has("GLFW error") || has("Failed to create window") || has("OpenGL") && has("not supported") || has("Pixel format not accelerated") {
        push("Graphics problem", "The game could not start its window or renderer. Update your graphics driver, and try turning off shader/graphics mods.".into());
    }
    hints
}

pub fn summarize(text: &str) -> CrashSummary {
    let mut s = CrashSummary::default();
    let lines: Vec<&str> = text.lines().collect();
    let mut desc_idx = None;
    for (i, l) in lines.iter().enumerate() {
        if let Some(r) = l.strip_prefix("Description: ") {
            s.description = r.trim().to_string();
            desc_idx = Some(i);
        } else if let Some(r) = l.strip_prefix("Time: ") {
            s.time = r.trim().to_string();
        }
        if s.description.is_empty() == false && !s.time.is_empty() {
            break;
        }
    }
    if let Some(i) = desc_idx {
        if let Some(j) = lines.iter().skip(i + 1).position(|l| !l.trim().is_empty() && !l.trim_start().starts_with("at ")) {
            let start = i + 1 + j;
            s.exception = lines[start].trim().to_string();
            s.head = lines[start..].iter().take(14).cloned().collect::<Vec<_>>().join("\n");
        }
    }
    if let Some(i) = lines.iter().position(|l| l.trim_start().starts_with("Suspected Mod")) {
        s.suspected = lines
            .iter()
            .skip(i + 1)
            .take_while(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .take(8)
            .collect();
    }
    s.hints = diagnose(text);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const OOM: &str = "---- Minecraft Crash Report ----\n// Don't do that.\n\nTime: 2026-11-09 15:36:03\nDescription: Rendering screen\n\njava.lang.OutOfMemoryError: Java heap space\n\tat net.minecraft.A.b(A.java:1)\n\tat net.minecraft.C.d(C.java:2)\n\nA detailed walkthrough...\n";

    const MIXIN: &str = "---- Minecraft Crash Report ----\n\nTime: 2026-11-09 16:00:00\nDescription: Initializing game\n\norg.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: An unexpected critical error was encountered\nCaused by: Mixin apply for mod coolmod failed coolmod.mixins.json:FooMixin from mod coolmod -> net.minecraft.A: InvalidInjectionException\n";

    const DEPS: &str = "Time: 2026-11-09 16:00:00\nDescription: Mod resolution failed\n\nnet.fabricmc.loader.impl.FormattedException: Incompatible mod set!\n\t- Mod 'Sodium' (sodium) 0.6.0 requires version 0.141 or later of mod 'Fabric API' (fabric-api), which is missing!\n";

    #[test]
    fn summarizes_exception_and_time() {
        let s = summarize(OOM);
        assert_eq!(s.description, "Rendering screen");
        assert_eq!(s.time, "2026-11-09 15:36:03");
        assert_eq!(s.exception, "java.lang.OutOfMemoryError: Java heap space");
        assert!(s.head.contains("at net.minecraft.A.b"));
        assert_eq!(s.hints.len(), 1);
        assert!(s.hints[0].title.contains("ran out of memory"));
    }

    #[test]
    fn names_the_mod_in_mixin_failures() {
        let s = summarize(MIXIN);
        assert!(s.hints.iter().any(|h| h.title.contains("hook into Minecraft") && h.advice.contains("coolmod")));
    }

    #[test]
    fn explains_missing_dependencies() {
        let s = summarize(DEPS);
        let h = s.hints.iter().find(|h| h.title.contains("dependency")).unwrap();
        assert!(h.advice.contains("Sodium"));
        assert!(h.advice.contains("Fabric API"));
    }

    #[test]
    fn unknown_crash_has_no_hints_but_still_parses() {
        let s = summarize("Time: x\nDescription: Boom\n\njava.lang.RuntimeException: weird\n\tat a.b(C.java:1)\n");
        assert!(s.hints.is_empty());
        assert_eq!(s.exception, "java.lang.RuntimeException: weird");
    }

    #[test]
    fn report_names_are_validated() {
        assert!(is_safe_report_name("crash-2026-11-09_15.36.03-client.txt"));
        for bad in ["", "../crash-1.txt", "crash-1/../x.txt", "crash-1.sh", "log.txt", "crash-\\a.txt"] {
            assert!(!is_safe_report_name(bad), "{bad}");
        }
    }

    #[test]
    fn lists_reads_and_tails() {
        let td = tempfile::tempdir().unwrap();
        let rd = td.path().join("crash-reports");
        std::fs::create_dir_all(&rd).unwrap();
        std::fs::write(rd.join("crash-a-client.txt"), OOM).unwrap();
        std::fs::write(rd.join("notes.txt"), "ignore me").unwrap();
        let l = list_reports(td.path());
        assert_eq!(l.len(), 1);
        assert!(read_report(td.path(), &l[0].name).unwrap().contains("OutOfMemoryError"));
        assert!(read_report(td.path(), "../crash-a-client.txt").is_none());
        assert!(newest_since(td.path(), 0).is_some());
        assert!(newest_since(td.path(), u64::MAX / 2).is_none());

        std::fs::create_dir_all(td.path().join("logs")).unwrap();
        let log: String = (1..=500).map(|i| format!("line {i}\n")).collect();
        std::fs::write(td.path().join("logs/latest.log"), log).unwrap();
        let t = log_tail(td.path(), 3);
        assert_eq!(t, "line 498\nline 499\nline 500");
        assert_eq!(log_tail(&td.path().join("missing"), 5), "");
    }
}
