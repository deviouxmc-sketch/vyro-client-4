//! Resource pack management for one profile: list what is in `resourcepacks/`, read each pack's
//! metadata/icon, and enable/disable/reorder through the `resourcePacks` line of `options.txt`
//! (the same file Minecraft itself reads, so the choice shows up in-game).
use crate::download::{Downloader, Job};
use crate::error::{Error, Result};
use crate::modrinth::{ModFile, ModVersion, Modrinth, CDN_PREFIX};
use crate::perf;
use base64::Engine;
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct PackInfo {
    /// Name on disk, e.g. `Faithful.zip` or a folder name.
    pub file: String,
    pub is_dir: bool,
    pub size: u64,
    pub description: String,
    pub pack_format: Option<u32>,
    /// `data:image/png;base64,...` when the pack ships a reasonably small pack.png.
    pub icon: Option<String>,
    pub enabled: bool,
    /// Index in the `resourcePacks` list (later = higher priority), when enabled.
    pub position: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackList {
    pub packs: Vec<PackInfo>,
    /// Entries Minecraft or mods put in the list themselves (e.g. `fabric`); left untouched.
    pub builtin: Vec<String>,
}

pub fn packs_dir(instance: &Path) -> PathBuf {
    instance.join("resourcepacks")
}

/// A single path component, nothing that could climb out of the folder.
pub fn is_safe_pack_name(name: &str) -> bool {
    !name.is_empty() && name.len() < 200 && !name.contains(['/', '\\', '\0']) && !name.starts_with('.') && !name.contains("..")
}

pub fn pack_id(file: &str) -> String {
    format!("file/{file}")
}

/// Enabled pack ids in file order (lowest priority first). Missing/odd file = just `vanilla`.
pub fn read_enabled(instance: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(instance.join("options.txt")).unwrap_or_default();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("resourcePacks:") {
            if let Ok(list) = serde_json::from_str::<Vec<String>>(v.trim()) {
                return list;
            }
        }
    }
    vec!["vanilla".to_string()]
}

fn write_enabled(instance: &Path, ids: &[String]) -> Result<()> {
    let mut ids = ids.to_vec();
    if !ids.iter().any(|i| i == "vanilla") {
        ids.insert(0, "vanilla".into());
    }
    let json = serde_json::to_string(&ids)?;
    perf::apply_video_options(instance, &[("resourcePacks".to_string(), json)])
}

fn text_of(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a.iter().map(text_of).collect::<Vec<_>>().join(""),
        serde_json::Value::Object(o) => {
            let mut out = o.get("text").map(text_of).unwrap_or_default();
            if let Some(extra) = o.get("extra") {
                out.push_str(&text_of(extra));
            }
            out
        }
        _ => String::new(),
    }
}

fn parse_mcmeta(bytes: &[u8]) -> (String, Option<u32>) {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else { return (String::new(), None) };
    let pack = &v["pack"];
    let desc = text_of(&pack["description"]);
    // strip Minecraft colour codes (section sign + one character)
    let mut clean = String::new();
    let mut skip = false;
    for c in desc.chars() {
        if skip {
            skip = false;
        } else if c == '\u{a7}' {
            skip = true;
        } else if c != '\n' {
            clean.push(c);
        }
    }
    (clean.trim().to_string(), pack["pack_format"].as_u64().map(|n| n as u32))
}

fn icon_url(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > 160 * 1024 {
        return None;
    }
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

fn read_zip_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.by_ref().take(1024 * 1024).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn inspect(path: &Path, file: String, enabled_ids: &[String]) -> Option<PackInfo> {
    let meta = std::fs::metadata(path).ok()?;
    let is_dir = meta.is_dir();
    let (mcmeta, png, size) = if is_dir {
        let m = std::fs::read(path.join("pack.mcmeta")).ok();
        m.as_ref()?; // a folder without pack.mcmeta is not a pack
        (m, std::fs::read(path.join("pack.png")).ok(), 0)
    } else {
        if !file.to_lowercase().ends_with(".zip") {
            return None;
        }
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
        (read_zip_entry(&mut zip, "pack.mcmeta"), read_zip_entry(&mut zip, "pack.png"), meta.len())
    };
    let (description, pack_format) = mcmeta.as_deref().map(parse_mcmeta).unwrap_or_default();
    let id = pack_id(&file);
    let position = enabled_ids.iter().position(|i| *i == id);
    Some(PackInfo {
        file,
        is_dir,
        size,
        description,
        pack_format,
        icon: png.as_deref().and_then(icon_url),
        enabled: position.is_some(),
        position,
    })
}

pub fn list(instance: &Path) -> PackList {
    let enabled_ids = read_enabled(instance);
    let mut packs = Vec::new();
    if let Ok(rd) = std::fs::read_dir(packs_dir(instance)) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !is_safe_pack_name(&name) {
                continue;
            }
            if let Some(p) = inspect(&e.path(), name, &enabled_ids) {
                packs.push(p);
            }
        }
    }
    packs.sort_by(|a, b| a.file.to_lowercase().cmp(&b.file.to_lowercase()));
    let builtin = enabled_ids.iter().filter(|i| *i != "vanilla" && !i.starts_with("file/")).cloned().collect();
    PackList { packs, builtin }
}

fn require_pack(instance: &Path, file: &str) -> Result<()> {
    if !is_safe_pack_name(file) {
        return Err(Error::msg("invalid pack name"));
    }
    if !packs_dir(instance).join(file).exists() {
        return Err(Error::msg("that pack is not in the resourcepacks folder"));
    }
    Ok(())
}

/// Enable a pack at the highest priority (end of the list).
pub fn enable_pack(instance: &Path, file: &str) -> Result<()> {
    require_pack(instance, file)?;
    let mut ids = read_enabled(instance);
    let id = pack_id(file);
    if !ids.contains(&id) {
        ids.push(id);
    }
    write_enabled(instance, &ids)
}

pub fn disable_pack(instance: &Path, file: &str) -> Result<()> {
    if !is_safe_pack_name(file) {
        return Err(Error::msg("invalid pack name"));
    }
    let id = pack_id(file);
    let ids: Vec<String> = read_enabled(instance).into_iter().filter(|i| *i != id).collect();
    write_enabled(instance, &ids)
}

/// `up` = higher priority (later in the list). `vanilla` always stays first.
pub fn move_pack(instance: &Path, file: &str, up: bool) -> Result<()> {
    if !is_safe_pack_name(file) {
        return Err(Error::msg("invalid pack name"));
    }
    let mut ids = read_enabled(instance);
    let Some(i) = ids.iter().position(|x| *x == pack_id(file)) else {
        return Err(Error::msg("that pack is not enabled"));
    };
    if up {
        if i + 1 < ids.len() {
            ids.swap(i, i + 1);
        }
    } else if i >= 2 {
        ids.swap(i, i - 1);
    }
    write_enabled(instance, &ids)
}

pub fn delete_pack(instance: &Path, file: &str) -> Result<()> {
    require_pack(instance, file)?;
    disable_pack(instance, file)?;
    let p = packs_dir(instance).join(file);
    if p.is_dir() {
        std::fs::remove_dir_all(p)?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

/// Pick the version to install (newest release, else newest anything) and its .zip file.
pub fn choose_pack_file(versions: &[ModVersion]) -> Result<(ModVersion, ModFile)> {
    let v = versions
        .iter()
        .find(|v| v.version_type == "release")
        .or_else(|| versions.first())
        .ok_or_else(|| Error::msg("no version of this pack exists for this Minecraft version"))?;
    let zips = v.files.iter().filter(|f| f.filename.to_lowercase().ends_with(".zip"));
    let file = zips.clone().find(|f| f.primary).or_else(|| zips.clone().next()).ok_or_else(|| Error::msg("this version has no .zip file"))?;
    if !is_safe_pack_name(&file.filename) {
        return Err(Error::msg(format!("refusing unsafe pack file name '{}'", file.filename)));
    }
    if !file.url.starts_with(CDN_PREFIX) {
        return Err(Error::msg("refusing to download a pack from outside Modrinth's CDN"));
    }
    Ok((v.clone(), file.clone()))
}

/// Download a resource pack from Modrinth into the profile and (optionally) enable it. Returns the file name.
pub async fn install_from_modrinth(instance: &Path, dl: &Downloader, mr: &Modrinth, project: &str, mc_version: &str, enable: bool) -> Result<String> {
    let versions = mr.versions_for_mc(project, mc_version).await?;
    let (_, file) = choose_pack_file(&versions)?;
    let dest = packs_dir(instance).join(&file.filename);
    std::fs::create_dir_all(packs_dir(instance))?;
    if dest.exists() {
        if crate::download::file_sha1(&dest).map(|h| h == file.hashes.sha1).unwrap_or(false) {
            if enable {
                enable_pack(instance, &file.filename)?;
            }
            return Ok(file.filename);
        }
        return Err(Error::msg(format!("a different file named '{}' already exists in the resourcepacks folder", file.filename)));
    }
    dl.fetch(&Job { url: file.url.clone(), dest: dest.clone(), sha1: Some(file.hashes.sha1.clone()), size: None }).await?;
    if enable {
        enable_pack(instance, &file.filename)?;
    }
    Ok(file.filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, mcmeta: &str, png: &[u8]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::FileOptions::default();
        z.start_file("pack.mcmeta", o).unwrap();
        z.write_all(mcmeta.as_bytes()).unwrap();
        if !png.is_empty() {
            z.start_file("pack.png", o).unwrap();
            z.write_all(png).unwrap();
        }
        z.finish().unwrap();
    }

    fn setup() -> tempfile::TempDir {
        let td = tempfile::tempdir().unwrap();
        let d = packs_dir(td.path());
        std::fs::create_dir_all(&d).unwrap();
        make_zip(&d.join("Alpha.zip"), r#"{"pack":{"pack_format":34,"description":"\u00a76Gold \u00a7rPack"}}"#, b"PNGDATA");
        make_zip(&d.join("Beta.zip"), r#"{"pack":{"pack_format":42,"description":[{"text":"Hello "},{"text":"there"}]}}"#, b"");
        std::fs::create_dir_all(d.join("FolderPack")).unwrap();
        std::fs::write(d.join("FolderPack/pack.mcmeta"), r#"{"pack":{"pack_format":1,"description":{"text":"From a folder"}}}"#).unwrap();
        std::fs::write(d.join("notes.txt"), "not a pack").unwrap();
        std::fs::create_dir_all(d.join("EmptyDir")).unwrap();
        td
    }

    #[test]
    fn lists_zip_and_folder_packs_with_metadata() {
        let td = setup();
        let l = list(td.path());
        let names: Vec<_> = l.packs.iter().map(|p| p.file.as_str()).collect();
        assert_eq!(names, vec!["Alpha.zip", "Beta.zip", "FolderPack"]); // notes.txt and EmptyDir ignored
        let a = &l.packs[0];
        assert_eq!(a.description, "Gold Pack"); // colour codes stripped
        assert_eq!(a.pack_format, Some(34));
        assert!(a.icon.as_ref().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(l.packs[1].description, "Hello there");
        assert!(l.packs[1].icon.is_none());
        assert_eq!(l.packs[2].description, "From a folder");
        assert!(l.packs[2].is_dir);
    }

    #[test]
    fn enable_disable_and_reorder_keep_builtin_entries() {
        let td = setup();
        std::fs::write(td.path().join("options.txt"), "lang:en_us\nresourcePacks:[\"vanilla\",\"fabric\"]\nfov:70\n").unwrap();
        enable_pack(td.path(), "Alpha.zip").unwrap();
        enable_pack(td.path(), "Beta.zip").unwrap();
        assert_eq!(read_enabled(td.path()), vec!["vanilla", "fabric", "file/Alpha.zip", "file/Beta.zip"]);
        let l = list(td.path());
        assert!(l.packs.iter().find(|p| p.file == "Alpha.zip").unwrap().enabled);
        assert_eq!(l.builtin, vec!["fabric".to_string()]);

        move_pack(td.path(), "Alpha.zip", true).unwrap(); // Alpha above Beta
        assert_eq!(read_enabled(td.path()), vec!["vanilla", "fabric", "file/Beta.zip", "file/Alpha.zip"]);
        move_pack(td.path(), "Beta.zip", false).unwrap(); // down, but vanilla stays first
        move_pack(td.path(), "Beta.zip", false).unwrap();
        move_pack(td.path(), "Beta.zip", false).unwrap();
        assert_eq!(read_enabled(td.path())[0], "vanilla");

        disable_pack(td.path(), "Alpha.zip").unwrap();
        assert!(!read_enabled(td.path()).contains(&"file/Alpha.zip".to_string()));
        // unrelated options survive
        let o = std::fs::read_to_string(td.path().join("options.txt")).unwrap();
        assert!(o.contains("lang:en_us") && o.contains("fov:70"));
    }

    #[test]
    fn missing_options_defaults_to_vanilla_and_creates_file() {
        let td = setup();
        assert_eq!(read_enabled(td.path()), vec!["vanilla"]);
        enable_pack(td.path(), "FolderPack").unwrap();
        assert_eq!(read_enabled(td.path()), vec!["vanilla", "file/FolderPack"]);
    }

    #[test]
    fn delete_removes_file_and_entry_and_rejects_traversal() {
        let td = setup();
        enable_pack(td.path(), "Alpha.zip").unwrap();
        delete_pack(td.path(), "Alpha.zip").unwrap();
        assert!(!packs_dir(td.path()).join("Alpha.zip").exists());
        assert!(!read_enabled(td.path()).contains(&"file/Alpha.zip".to_string()));
        delete_pack(td.path(), "FolderPack").unwrap();
        assert!(!packs_dir(td.path()).join("FolderPack").exists());
        for bad in ["../x.zip", "a/b.zip", ".hidden", ""] {
            assert!(delete_pack(td.path(), bad).is_err(), "{bad}");
            assert!(enable_pack(td.path(), bad).is_err(), "{bad}");
        }
        assert!(enable_pack(td.path(), "Missing.zip").is_err());
    }

    fn version(kind: &str, url: &str, name: &str) -> ModVersion {
        use crate::modrinth::Hashes;
        ModVersion {
            id: "v".into(), project_id: "p".into(), name: "".into(), version_number: "1".into(), version_type: kind.into(),
            game_versions: vec![], loaders: vec![], dependencies: vec![],
            files: vec![ModFile { url: url.into(), filename: name.into(), primary: true, size: 1, hashes: Hashes { sha1: "x".into(), sha512: String::new() } }],
        }
    }

    #[test]
    fn chooses_release_zip_from_modrinth_cdn_only() {
        let good = version("release", &format!("{CDN_PREFIX}data/p/Pack.zip"), "Pack.zip");
        let beta = version("beta", &format!("{CDN_PREFIX}data/p/Beta.zip"), "Beta.zip");
        assert_eq!(choose_pack_file(&[beta.clone(), good.clone()]).unwrap().1.filename, "Pack.zip");
        assert_eq!(choose_pack_file(&[beta]).unwrap().1.filename, "Beta.zip"); // falls back to non-release
        assert!(choose_pack_file(&[]).is_err());
        assert!(choose_pack_file(&[version("release", "https://evil.example/Pack.zip", "Pack.zip")]).is_err());
        assert!(choose_pack_file(&[version("release", &format!("{CDN_PREFIX}x"), "../Pack.zip")]).is_err());
        assert!(choose_pack_file(&[version("release", &format!("{CDN_PREFIX}x"), "Pack.jar")]).is_err());
    }
}
