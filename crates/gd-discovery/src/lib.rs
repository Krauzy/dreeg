use gd_save::read_summary;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveSource {
    Local,
    SteamCloud,
    Custom,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterSummary {
    pub id: String,
    pub path: String,
    pub name: String,
    pub class_name: String,
    pub class_tag: String,
    pub level: i32,
    pub male: bool,
    pub hardcore: bool,
    pub expansion_character: bool,
    pub source: SaveSource,
    pub modified_at: Option<u64>,
    pub data_version: i32,
}

#[derive(Debug, Clone)]
pub struct DiscoveredCharacter {
    pub summary: CharacterSummary,
    pub path: PathBuf,
}

pub fn default_save_roots() -> Vec<(PathBuf, SaveSource)> {
    let mut roots = BTreeSet::new();
    let mut add_documents = |documents: PathBuf| {
        roots.insert((
            documents.join("My Games/Grim Dawn/save/main"),
            SaveSource::Local,
        ));
        roots.insert((
            documents.join("My Games/Grim Dawn/save/user"),
            SaveSource::Local,
        ));
    };

    if let Some(profile) = env::var_os("USERPROFILE").map(PathBuf::from) {
        add_documents(profile.join("Documents"));
    }
    for variable in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Some(one_drive) = env::var_os(variable).map(PathBuf::from) {
            add_documents(one_drive.join("Documents"));
        }
    }

    let mut steam_candidates = BTreeSet::new();
    if let Some(program_files) = env::var_os("ProgramFiles(x86)").map(PathBuf::from) {
        steam_candidates.insert(program_files.join("Steam"));
    }
    if let Some(program_files) = env::var_os("ProgramFiles").map(PathBuf::from) {
        steam_candidates.insert(program_files.join("Steam"));
    }
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        if let Ok(steam) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Valve\\Steam")
            && let Ok::<String, _>(path) = steam.get_value("SteamPath")
        {
            steam_candidates.insert(PathBuf::from(path));
        }
    }
    for steam in steam_candidates {
        let userdata = steam.join("userdata");
        let Ok(accounts) = fs::read_dir(userdata) else {
            continue;
        };
        for account in accounts.flatten().filter(|entry| entry.path().is_dir()) {
            roots.insert((
                account.path().join("219990/remote/save/main"),
                SaveSource::SteamCloud,
            ));
            roots.insert((
                account.path().join("219990/remote/save/user"),
                SaveSource::SteamCloud,
            ));
        }
    }
    roots.into_iter().collect()
}

pub fn discover(extra_roots: &[PathBuf]) -> Vec<DiscoveredCharacter> {
    let mut roots = default_save_roots();
    roots.extend(
        extra_roots
            .iter()
            .cloned()
            .map(|path| (path, SaveSource::Custom)),
    );

    let mut found = BTreeSet::new();
    let mut characters = Vec::new();
    for (root, source) in roots {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
            let path = entry.path().join("player.gdc");
            if !path.is_file() {
                continue;
            }
            let canonical = fs::canonicalize(&path).unwrap_or(path);
            let identity = canonical.to_string_lossy().to_lowercase();
            if !found.insert(identity) {
                continue;
            }
            let Ok((header, data_version)) = read_summary(&canonical) else {
                continue;
            };
            let modified_at = canonical
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as u64);
            characters.push(DiscoveredCharacter {
                summary: CharacterSummary {
                    id: stable_path_id(&canonical),
                    path: canonical.to_string_lossy().into_owned(),
                    name: header.character_name,
                    class_tag: header.class_name.clone(),
                    class_name: header.class_name,
                    level: header.character_level,
                    male: header.male,
                    hardcore: header.hardcore,
                    expansion_character: header.expansion_character,
                    source,
                    modified_at,
                    data_version,
                },
                path: canonical,
            });
        }
    }
    characters.sort_by(|left, right| {
        left.summary
            .name
            .to_lowercase()
            .cmp(&right.summary.name.to_lowercase())
            .then_with(|| left.summary.path.cmp(&right.summary.path))
    });
    characters
}

pub fn stable_path_id(path: &Path) -> String {
    // Stable FNV-1a is sufficient here: IDs only address paths already allowlisted
    // by the backend and are never treated as an authorization secret.
    let normalized = path.to_string_lossy().to_lowercase();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in normalized.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_ids_are_case_insensitive_on_windows() {
        assert_eq!(
            stable_path_id(Path::new(r"C:\Saves\Hero\player.gdc")),
            stable_path_id(Path::new(r"c:\saves\hero\PLAYER.GDC")),
        );
    }
}
