use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use gd_db::{CatalogItem, CatalogKind, GameDatabaseInfo, GameDatabaseLocation, GameResourceIndex};
use gd_discovery::{CharacterSummary, DiscoveredCharacter, discover};
use gd_save::{
    CharacterItem, CoreStats, FactionPatch, FactionValue, GdcDocument, ItemPatch, NewInventoryItem,
    encode_mutation, parse, read_file,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
struct AppState {
    characters: Mutex<HashMap<String, DiscoveredCharacter>>,
    catalog: Mutex<Option<CatalogCache>>,
    resources: Mutex<ResourceCache>,
    item_icons: Mutex<HashMap<String, Option<String>>>,
}

#[derive(Clone)]
struct CatalogCache {
    location: GameDatabaseLocation,
    items: Vec<CatalogItem>,
    names_by_record: HashMap<String, String>,
    icon_paths_by_record: HashMap<String, String>,
    localization: HashMap<String, String>,
    warnings: Vec<String>,
}

#[derive(Default)]
struct ResourceCache {
    index: Option<GameResourceIndex>,
    last_error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CharacterItemDocument {
    #[serde(flatten)]
    item: CharacterItem,
    display_name: String,
    component_display_name: Option<String>,
    augment_display_name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FactionDocument {
    #[serde(flatten)]
    faction: FactionValue,
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CharacterDocument {
    #[serde(flatten)]
    summary: CharacterSummary,
    core_stats: Option<CoreStats>,
    iron: Option<i32>,
    items: Vec<CharacterItemDocument>,
    inventory_bag_count: usize,
    factions: Vec<FactionDocument>,
    block_count: usize,
    write_supported: bool,
    write_blockers: Vec<i32>,
    database_warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CharacterPatch {
    character_name: String,
    character_level: i32,
    hardcore: bool,
    iron: i32,
    core_stats: Option<CoreStats>,
    #[serde(default)]
    items: Vec<ItemPatch>,
    #[serde(default)]
    new_items: Vec<NewInventoryItem>,
    #[serde(default)]
    factions: Vec<FactionPatch>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveResult {
    character: CharacterDocument,
    backup_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RestoreResult {
    character: CharacterDocument,
    restored_backup_path: String,
    safety_backup_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupMetadata {
    source_path: String,
    created_at: u128,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogSearchResult {
    database: GameDatabaseInfo,
    total: usize,
    items: Vec<CatalogItem>,
}

#[tauri::command]
fn scan_characters(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<CharacterSummary>, String> {
    let discovered = discover(&[]);
    let summaries = {
        let mut catalog = state
            .catalog
            .lock()
            .map_err(|_| "Local game database is unavailable.")?;
        let cache = match ensure_catalog_cache(&mut catalog) {
            Ok(cache) => Some(cache),
            Err(error) => {
                append_runtime_log(&app, "catalog", &error);
                None
            }
        };
        if let Some(cache) = cache {
            for warning in &cache.warnings {
                append_runtime_log(&app, "catalog-warning", warning);
            }
        }
        discovered
            .iter()
            .map(|entry| localized_summary(&entry.summary, cache))
            .collect::<Vec<_>>()
    };
    let mut allowlist = state
        .characters
        .lock()
        .map_err(|_| "Internal application state is unavailable.")?;
    *allowlist = discovered
        .into_iter()
        .map(|entry| (entry.summary.id.clone(), entry))
        .collect();
    Ok(summaries)
}

#[tauri::command]
fn load_character(id: String, state: State<'_, AppState>) -> Result<CharacterDocument, String> {
    let entry = allowed_character(&id, &state)?;
    document_for(&entry, &state)
}

#[tauri::command]
fn game_database_status() -> Result<Option<GameDatabaseInfo>, String> {
    Ok(gd_db::discover_game_database().map(|location| location.info()))
}

#[tauri::command]
fn search_item_catalog(
    query: String,
    kind: Option<CatalogKind>,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<CatalogSearchResult, String> {
    let mut cache = state
        .catalog
        .lock()
        .map_err(|_| "Local item catalog is unavailable.")?;
    let cache = ensure_catalog_cache(&mut cache)?;
    let normalized = query.trim().to_lowercase();
    let mut matches = cache
        .items
        .iter()
        .filter(|item| kind.is_none_or(|kind| item.kind == kind))
        .filter(|item| {
            normalized.is_empty()
                || item.name.to_lowercase().contains(&normalized)
                || item.record.to_lowercase().contains(&normalized)
        })
        .collect::<Vec<_>>();
    let total = matches.len();
    matches.truncate(limit.unwrap_or(100).clamp(1, 500));
    Ok(CatalogSearchResult {
        database: cache.location.info(),
        total,
        items: matches.into_iter().cloned().collect(),
    })
}

#[tauri::command]
fn load_item_icons(
    app: AppHandle,
    records: Vec<String>,
    state: State<'_, AppState>,
) -> Result<HashMap<String, Option<String>>, String> {
    let requested = records
        .into_iter()
        .map(|record| record.to_ascii_lowercase())
        .filter(|record| record.starts_with("records/items/") && record.ends_with(".dbr"))
        .collect::<BTreeSet<_>>();
    if requested.len() > 250 {
        return Err("At most 250 item icons can be loaded at once.".into());
    }

    let mut memory = state
        .item_icons
        .lock()
        .map_err(|_| "The item icon cache is unavailable.")?;
    let mut catalog = state
        .catalog
        .lock()
        .map_err(|_| "The local item catalog is unavailable.")?;
    let catalog = ensure_catalog_cache(&mut catalog)?;
    let mut resources = state
        .resources
        .lock()
        .map_err(|_| "The item resource cache is unavailable.")?;
    let resource_index = match ensure_resource_cache(&mut resources, &catalog.location) {
        Ok(index) => index,
        Err(error) => {
            append_runtime_log(&app, "item-images", &error);
            return Err(error);
        }
    };
    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(display_error)?
        .join("item-icons-v1");

    let mut result = HashMap::with_capacity(requested.len());
    for record in requested {
        let value = if let Some(cached) = memory.get(&record) {
            cached.clone()
        } else {
            // A missing or malformed texture must not prevent the remaining
            // inventory from rendering. The frontend will use its typed
            // fallback artwork for this individual record.
            let loaded = load_item_icon(catalog, resource_index, &cache_root, &record)
                .unwrap_or_else(|error| {
                    append_runtime_log(&app, "item-image", &format!("{record}: {error}"));
                    None
                });
            memory.insert(record.clone(), loaded.clone());
            loaded
        };
        result.insert(record, value);
    }
    Ok(result)
}

fn load_item_icon(
    catalog: &CatalogCache,
    resources: &GameResourceIndex,
    cache_root: &Path,
    record: &str,
) -> Result<Option<String>, String> {
    let Some(icon_path) = catalog.icon_paths_by_record.get(record) else {
        return Ok(None);
    };
    let cache_key = icon_cache_key(icon_path, &catalog.location);
    let cached_path = cache_root.join(format!("{cache_key}.png"));
    let png = if cached_path.is_file() {
        fs::read(&cached_path).map_err(display_error)?
    } else {
        let Some(png) = resources
            .thumbnail_png(icon_path, 72)
            .map_err(display_error)?
        else {
            return Ok(None);
        };
        fs::create_dir_all(cache_root).map_err(display_error)?;
        fs::write(&cached_path, &png).map_err(display_error)?;
        png
    };
    Ok(Some(format!(
        "data:image/png;base64,{}",
        BASE64.encode(png)
    )))
}

fn icon_cache_key(icon_path: &str, location: &GameDatabaseLocation) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in icon_path
        .as_bytes()
        .iter()
        .copied()
        .chain(location.resource_files.iter().flat_map(|path| {
            let metadata = path.metadata().ok();
            format!(
                "{}:{}:{}",
                path.display(),
                metadata.as_ref().map_or(0, fs::Metadata::len),
                metadata
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |duration| duration.as_secs())
            )
            .into_bytes()
        }))
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[tauri::command]
fn save_character(
    app: AppHandle,
    id: String,
    patch: CharacterPatch,
    state: State<'_, AppState>,
) -> Result<SaveResult, String> {
    if grim_dawn_running() {
        return Err("Close Grim Dawn before saving the character.".into());
    }

    let mut entry = allowed_character(&id, &state)?;
    let mut document = read_file(&entry.path).map_err(display_error)?;
    let write_blockers = document.mutation_blockers();
    if !write_blockers.is_empty() {
        return Err(format!(
            "Safety lock: this save contains encrypted blocks that Dreeg cannot safely rewrite yet: {}. No file was changed.",
            write_blockers
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    apply_patch(&mut document, patch)?;
    let encoded = encode_mutation(&document).map_err(display_error)?;

    // Reparse entirely before touching the original. This catches format,
    // length and checksum errors using the same path used on the next load.
    parse(&encoded).map_err(display_error)?;

    let backup_path = create_backup(&app, &entry)?;
    replace_atomically(&entry.path, &encoded)?;

    // Refresh metadata after the write while preserving the same allowlisted ID.
    let refreshed = refresh_entry(&entry).map_err(display_error)?;
    entry.summary = refreshed;
    {
        let mut allowlist = state
            .characters
            .lock()
            .map_err(|_| "Internal application state is unavailable.")?;
        allowlist.insert(id, entry.clone());
    }
    Ok(SaveResult {
        character: document_for(&entry, &state)?,
        backup_path: backup_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
fn restore_latest_backup(
    app: AppHandle,
    id: String,
    state: State<'_, AppState>,
) -> Result<RestoreResult, String> {
    if grim_dawn_running() {
        return Err("Close Grim Dawn before restoring a backup.".into());
    }

    let mut entry = allowed_character(&id, &state)?;
    let restored_backup_path = find_latest_backup(&app, &entry)?;
    let backup_contents = fs::read(&restored_backup_path).map_err(display_error)?;

    // Never replace a live save with a backup that no longer passes the same
    // full parser and checksum validation used for normal writes.
    parse(&backup_contents).map_err(|error| format!("The latest backup is invalid: {error}"))?;

    // Keep the operation reversible: the current live save becomes the newest
    // backup before the selected backup is restored atomically.
    let safety_backup_path = create_backup(&app, &entry)?;
    replace_atomically(&entry.path, &backup_contents)?;

    let refreshed = refresh_entry(&entry).map_err(display_error)?;
    entry.summary = refreshed;
    {
        let mut allowlist = state
            .characters
            .lock()
            .map_err(|_| "Internal application state is unavailable.")?;
        allowlist.insert(id, entry.clone());
    }

    Ok(RestoreResult {
        character: document_for(&entry, &state)?,
        restored_backup_path: restored_backup_path.to_string_lossy().into_owned(),
        safety_backup_path: safety_backup_path.to_string_lossy().into_owned(),
    })
}

fn allowed_character(id: &str, state: &State<'_, AppState>) -> Result<DiscoveredCharacter, String> {
    state
        .characters
        .lock()
        .map_err(|_| "Internal application state is unavailable.")?
        .get(id)
        .cloned()
        .ok_or_else(|| "The file is not part of this session's discovered characters.".into())
}

fn document_for(
    entry: &DiscoveredCharacter,
    state: &State<'_, AppState>,
) -> Result<CharacterDocument, String> {
    let document = read_file(&entry.path).map_err(display_error)?;
    let core_stats = document.core_stats().map_err(display_error)?;
    let iron = document.iron().map_err(display_error)?;
    let raw_items = document.items().map_err(display_error)?;
    let inventory_bag_count = document.inventory_bag_count().map_err(display_error)?;
    let raw_factions = document.factions().map_err(display_error)?;
    let mut catalog = state
        .catalog
        .lock()
        .map_err(|_| "Local game database is unavailable.")?;
    let cache = ensure_catalog_cache(&mut catalog).ok();
    let items = raw_items
        .into_iter()
        .map(|item| {
            let component_display_name = display_record_name(&item.component_record, cache);
            let augment_display_name = display_record_name(&item.augment_record, cache);
            CharacterItemDocument {
                display_name: display_item_name(&item, cache),
                component_display_name,
                augment_display_name,
                item,
            }
        })
        .collect();
    let factions = raw_factions
        .into_iter()
        .map(|faction| FactionDocument {
            name: faction_name(faction.index, cache),
            faction,
        })
        .collect();
    let write_blockers = document.mutation_blockers();
    Ok(CharacterDocument {
        summary: localized_summary(&entry.summary, cache),
        block_count: document.blocks.len(),
        core_stats,
        iron,
        items,
        inventory_bag_count,
        factions,
        write_supported: write_blockers.is_empty(),
        write_blockers,
        database_warnings: cache.map_or_else(Vec::new, |cache| cache.warnings.clone()),
    })
}

fn ensure_catalog_cache(cache: &mut Option<CatalogCache>) -> Result<&CatalogCache, String> {
    if cache.is_none() {
        let location = gd_db::discover_game_database()
            .ok_or("Grim Dawn and its database.arz files could not be found on this computer.")?;
        let database = gd_db::load_database(&location).map_err(display_error)?;
        let names_by_record = database
            .catalog
            .iter()
            .map(|item| (item.record.to_ascii_lowercase(), item.name.clone()))
            .collect();
        let icon_paths_by_record = database
            .catalog
            .iter()
            .filter_map(|item| {
                item.icon_path
                    .as_ref()
                    .map(|icon| (item.record.to_ascii_lowercase(), icon.clone()))
            })
            .collect();
        *cache = Some(CatalogCache {
            location,
            items: database.catalog,
            names_by_record,
            icon_paths_by_record,
            localization: database.localization,
            warnings: database.warnings,
        });
    }
    cache
        .as_ref()
        .ok_or_else(|| "Local game database is unavailable.".into())
}

fn ensure_resource_cache<'a>(
    cache: &'a mut ResourceCache,
    location: &GameDatabaseLocation,
) -> Result<&'a GameResourceIndex, String> {
    if cache.index.is_none() {
        match gd_db::load_resource_index(location) {
            Ok(index) => {
                cache.index = Some(index);
                cache.last_error = None;
            }
            Err(error) => {
                let message = format!("Grim Dawn item images could not be indexed: {error}");
                cache.last_error = Some(message.clone());
                return Err(message);
            }
        }
    }
    cache
        .index
        .as_ref()
        .ok_or_else(|| "The item resource cache is unavailable.".into())
}

fn append_runtime_log(app: &AppHandle, area: &str, message: &str) {
    let Ok(directory) = app.path().app_log_dir() else {
        return;
    };
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join("dreeg.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} [{area}] {message}", timestamp_millis());
    }
}

fn localized_summary(summary: &CharacterSummary, cache: Option<&CatalogCache>) -> CharacterSummary {
    let mut localized = summary.clone();
    if let Some(name) = cache.and_then(|cache| cache.localization.get(&summary.class_name)) {
        localized.class_name = localized_gender_name(name, summary.male);
    } else if summary.class_name.is_empty() {
        localized.class_name = "No class".into();
    }
    localized
}

fn localized_gender_name(value: &str, male: bool) -> String {
    let Some(without_marker) = value.strip_prefix("[ms]") else {
        return value.to_owned();
    };
    let Some((male_name, female_name)) = without_marker.split_once("[fs]") else {
        return value.to_owned();
    };
    if male {
        male_name.to_owned()
    } else {
        female_name.to_owned()
    }
}

fn display_item_name(item: &CharacterItem, cache: Option<&CatalogCache>) -> String {
    if item.base_record.is_empty() {
        return "Empty slot".into();
    }
    let Some(cache) = cache else {
        return "Unknown item".into();
    };
    let lookup = |record: &str| {
        cache
            .names_by_record
            .get(&record.to_ascii_lowercase())
            .cloned()
    };
    let Some(base) = lookup(&item.base_record) else {
        return "Unknown item".into();
    };
    [
        lookup(&item.prefix_record),
        Some(base),
        lookup(&item.suffix_record),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.trim().is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

fn display_record_name(record: &str, cache: Option<&CatalogCache>) -> Option<String> {
    if record.is_empty() {
        return None;
    }
    cache
        .and_then(|cache| cache.names_by_record.get(&record.to_ascii_lowercase()))
        .cloned()
}

fn faction_name(index: usize, cache: Option<&CatalogCache>) -> String {
    match index {
        0 => "Player".into(),
        1 => "Devil's Crossing".into(),
        2 => "Aetherials".into(),
        3 => "Chthonians".into(),
        4 => "Cronley's Gang".into(),
        5 => "Neutral".into(),
        _ => {
            let tag = format!("tagFactionUser{}", index - 6);
            cache
                .and_then(|cache| cache.localization.get(&tag))
                .cloned()
                .unwrap_or_else(|| format!("Faction {index}"))
        }
    }
}

fn apply_patch(document: &mut GdcDocument, patch: CharacterPatch) -> Result<(), String> {
    if patch.character_name != document.header.character_name {
        return Err(
            "Character renaming is not enabled because it also requires moving the save folder."
                .into(),
        );
    }
    ensure_existing_items_are_read_only(patch.items.len())?;
    if !(1..=100).contains(&patch.character_level) {
        return Err("Level must be between 1 and 100.".into());
    }
    if !(0..=2_000_000_000).contains(&patch.iron) {
        return Err("Iron must be between 0 and 2,000,000,000.".into());
    }
    document.header.character_level = patch.character_level;
    document.header.hardcore = patch.hardcore;
    document.set_iron(patch.iron).map_err(display_error)?;

    if let Some(mut stats) = patch.core_stats {
        stats.level_in_bio = patch.character_level;
        validate_core_stats(&stats)?;
        document.set_core_stats(&stats).map_err(display_error)?;
    }
    for item in &patch.new_items {
        document.add_inventory_item(item).map_err(display_error)?;
    }
    for faction in &patch.factions {
        document
            .apply_faction_patch(faction)
            .map_err(display_error)?;
    }
    Ok(())
}

fn ensure_existing_items_are_read_only(item_patch_count: usize) -> Result<(), String> {
    if item_patch_count == 0 {
        return Ok(());
    }
    Err(
        "Existing inventory and equipped items are read-only. Add a new inventory item instead."
            .into(),
    )
}

fn validate_core_stats(stats: &CoreStats) -> Result<(), String> {
    for (label, value) in [
        ("internal level", stats.level_in_bio),
        ("experience", stats.experience),
        ("attribute points", stats.attribute_points),
        ("skill points", stats.skill_points),
        ("devotion points", stats.devotion_points),
        ("unlocked devotion", stats.total_devotion_points_unlocked),
    ] {
        if value < 0 {
            return Err(format!("{label} cannot be negative."));
        }
    }
    if !(1..=100).contains(&stats.level_in_bio) {
        return Err("Internal level must be between 1 and 100.".into());
    }
    for (label, value) in [
        ("physique", stats.physique),
        ("cunning", stats.cunning),
        ("spirit", stats.spirit),
        ("health", stats.health),
        ("energy", stats.energy),
    ] {
        if !value.is_finite() || !(0.0..=10_000_000.0).contains(&value) {
            return Err(format!("Invalid value for {label}."));
        }
    }
    Ok(())
}

fn create_backup(app: &AppHandle, entry: &DiscoveredCharacter) -> Result<PathBuf, String> {
    let backup_root = app
        .path()
        .app_local_data_dir()
        .map_err(display_error)?
        .join("backups");
    fs::create_dir_all(&backup_root).map_err(display_error)?;

    let mut stamp = timestamp_millis();
    let backup_dir = loop {
        let candidate = backup_root.join(format!("{stamp}-{}", entry.summary.id));
        match fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => stamp += 1,
            Err(error) => return Err(display_error(error)),
        }
    };
    let backup_path = backup_dir.join("player.gdc");
    fs::copy(&entry.path, &backup_path).map_err(display_error)?;

    let metadata = serde_json::json!({
        "character": entry.summary.name,
        "sourcePath": entry.summary.path,
        "createdAt": stamp,
    });
    fs::write(
        backup_dir.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata).map_err(display_error)?,
    )
    .map_err(display_error)?;
    Ok(backup_path)
}

fn find_latest_backup(app: &AppHandle, entry: &DiscoveredCharacter) -> Result<PathBuf, String> {
    let backup_root = app
        .path()
        .app_local_data_dir()
        .map_err(display_error)?
        .join("backups");
    let directories = fs::read_dir(&backup_root).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            "No backup is available for this character yet.".to_string()
        }
        _ => display_error(error),
    })?;
    let expected_suffix = format!("-{}", entry.summary.id);
    let mut latest: Option<(u128, PathBuf)> = None;

    for directory in directories
        .flatten()
        .filter(|candidate| candidate.path().is_dir())
    {
        if !directory
            .file_name()
            .to_string_lossy()
            .ends_with(&expected_suffix)
        {
            continue;
        }
        let directory_path = directory.path();
        let Ok(metadata_contents) = fs::read(directory_path.join("metadata.json")) else {
            continue;
        };
        let Ok(metadata) = serde_json::from_slice::<BackupMetadata>(&metadata_contents) else {
            continue;
        };
        if !metadata
            .source_path
            .eq_ignore_ascii_case(&entry.summary.path)
        {
            continue;
        }
        let backup_path = directory_path.join("player.gdc");
        if !backup_path.is_file() {
            continue;
        }
        if latest
            .as_ref()
            .is_none_or(|(created_at, _)| metadata.created_at > *created_at)
        {
            latest = Some((metadata.created_at, backup_path));
        }
    }

    latest
        .map(|(_, path)| path)
        .ok_or_else(|| "No backup is available for this character yet.".into())
}

fn replace_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("The save file does not have a valid parent folder.")?;
    let stamp = timestamp_millis();
    let temp_path = parent.join(format!("player.gdc.dreeg-{stamp}.tmp"));
    let swap_path = parent.join(format!("player.gdc.dreeg-{stamp}.swap"));

    let mut temp = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(display_error)?;
    temp.write_all(contents).map_err(display_error)?;
    temp.sync_all().map_err(display_error)?;
    drop(temp);

    let verification = fs::read(&temp_path).map_err(display_error)?;
    if let Err(error) = parse(&verification) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("The temporary save failed validation: {error}"));
    }

    fs::rename(path, &swap_path).map_err(display_error)?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let rollback = fs::rename(&swap_path, path);
        return match rollback {
            Ok(()) => Err(format!(
                "Failed to replace the save; the original was restored: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "Failed to replace and restore the save. The original remains at {}. Errors: {error}; {rollback_error}",
                swap_path.display()
            )),
        };
    }
    // The replacement already succeeded. A stale swap is recoverable and
    // must not be reported as though the save operation itself had failed.
    let _ = fs::remove_file(&swap_path);
    Ok(())
}

fn refresh_entry(entry: &DiscoveredCharacter) -> Result<CharacterSummary, gd_save::GdcError> {
    let document = read_file(&entry.path)?;
    let modified_at = entry
        .path
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64);
    Ok(CharacterSummary {
        id: entry.summary.id.clone(),
        path: entry.summary.path.clone(),
        name: document.header.character_name,
        class_name: document.header.class_name.clone(),
        class_tag: document.header.class_name,
        level: document.header.character_level,
        male: document.header.male,
        hardcore: document.header.hardcore,
        expansion_character: document.header.expansion_character,
        source: entry.summary.source,
        modified_at,
        data_version: document.data_version,
    })
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(windows)]
fn grim_dawn_running() -> bool {
    let Ok(output) = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq Grim Dawn.exe", "/NH"])
        .output()
    else {
        // Failure to inspect processes should fail closed for a write operation.
        return true;
    };
    String::from_utf8_lossy(&output.stdout)
        .to_ascii_lowercase()
        .contains("grim dawn.exe")
}

#[cfg(not(windows))]
fn grim_dawn_running() -> bool {
    false
}

fn display_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            scan_characters,
            load_character,
            game_database_status,
            search_item_catalog,
            load_item_icons,
            save_character,
            restore_latest_backup
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Dreeg");
}

#[cfg(test)]
mod tests {
    use super::{
        ResourceCache, ensure_existing_items_are_read_only, ensure_resource_cache,
        localized_gender_name, timestamp_millis,
    };
    use gd_db::GameDatabaseLocation;
    use std::{fs, path::PathBuf};

    #[test]
    fn rejects_existing_item_patches_but_accepts_insert_only_saves() {
        assert!(ensure_existing_items_are_read_only(0).is_ok());
        assert!(ensure_existing_items_are_read_only(1).is_err());
    }

    #[test]
    fn resolves_gendered_class_localization() {
        assert_eq!(
            localized_gender_name("[ms]Sorcerer[fs]Sorceress", true),
            "Sorcerer"
        );
        assert_eq!(
            localized_gender_name("[ms]Sorcerer[fs]Sorceress", false),
            "Sorceress"
        );
        assert_eq!(localized_gender_name("Warlord", true), "Warlord");
    }

    #[test]
    fn reports_a_broken_resource_archive_without_affecting_the_catalog() {
        let archive = std::env::temp_dir().join(format!(
            "dreeg-invalid-resource-{}-{}.arc",
            std::process::id(),
            timestamp_millis()
        ));
        fs::write(&archive, b"not an ARC archive").expect("write invalid ARC fixture");
        let location = GameDatabaseLocation {
            install_path: PathBuf::new(),
            database_files: Vec::new(),
            localization_files: Vec::new(),
            resource_files: vec![archive.clone()],
        };
        let mut cache = ResourceCache::default();

        let error = match ensure_resource_cache(&mut cache, &location) {
            Ok(_) => panic!("invalid resource archive should fail"),
            Err(error) => error,
        };

        assert!(error.contains("item images could not be indexed"));
        assert!(cache.index.is_none());
        assert_eq!(cache.last_error.as_deref(), Some(error.as_str()));
        fs::remove_file(archive).expect("remove invalid ARC fixture");
    }
}
