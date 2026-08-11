//! Read-only indexer for Grim Dawn ARZ databases and ARC localization files.

use lz4_flex::block::decompress;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap},
    env,
    fs::{self, File},
    io::{Cursor as IoCursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("data file is truncated at position {position}")]
    Truncated { position: usize },
    #[error("unrecognized ARC format")]
    InvalidArc,
    #[error("invalid string-table index: {0}")]
    InvalidStringIndex(i32),
    #[error("invalid LZ4 data: {0}")]
    Lz4(String),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid texture data: {0}")]
    InvalidTexture(String),
    #[error("image decode error: {0}")]
    Image(String),
    #[error("no usable Grim Dawn database files were found: {0}")]
    NoUsableDatabase(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogKind {
    Base,
    Prefix,
    Suffix,
    Component,
    Augment,
    Ascendant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItem {
    pub record: String,
    pub name: String,
    pub class_name: String,
    pub kind: CatalogKind,
    pub icon_path: Option<String>,
    pub level_requirement: Option<i32>,
    pub item_level: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDatabaseInfo {
    pub install_path: String,
    pub database_files: Vec<String>,
    pub localization_files: Vec<String>,
    pub resource_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GameDatabaseLocation {
    pub install_path: PathBuf,
    pub database_files: Vec<PathBuf>,
    pub localization_files: Vec<PathBuf>,
    pub resource_files: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct LoadedGameDatabase {
    pub catalog: Vec<CatalogItem>,
    pub localization: HashMap<String, String>,
    pub record_names: HashMap<String, String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GameResourceIndex {
    archives: Vec<ArcArchiveIndex>,
}

#[derive(Debug, Clone)]
struct ArcArchiveIndex {
    path: PathBuf,
    record_table_offset: u64,
    entries: HashMap<String, ArcEntry>,
}

#[derive(Debug, Clone, Copy)]
struct ArcEntry {
    entry_type: i32,
    offset: i32,
    compressed_size: i32,
    decompressed_size: i32,
    parts: i32,
    first_part: i32,
}

impl GameDatabaseLocation {
    pub fn info(&self) -> GameDatabaseInfo {
        GameDatabaseInfo {
            install_path: self.install_path.to_string_lossy().into_owned(),
            database_files: self
                .database_files
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            localization_files: self
                .localization_files
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            resource_files: self
                .resource_files
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        }
    }
}

pub fn discover_game_database() -> Option<GameDatabaseLocation> {
    discover_install_roots()
        .into_iter()
        .find_map(|root| location_at(&root))
}

pub fn load_catalog(location: &GameDatabaseLocation) -> Result<Vec<CatalogItem>, DatabaseError> {
    Ok(load_database(location)?.catalog)
}

pub fn load_database(location: &GameDatabaseLocation) -> Result<LoadedGameDatabase, DatabaseError> {
    let mut localization = HashMap::new();
    let mut warnings = Vec::new();
    for path in &location.localization_files {
        match load_localization(path) {
            Ok(entries) => localization.extend(entries),
            Err(error) => warnings.push(format!(
                "Skipped localization archive {}: {error}",
                path.display()
            )),
        }
    }

    let mut records = HashMap::<String, RecordSummary>::new();
    let mut loaded_databases = 0_usize;
    for database in &location.database_files {
        match load_arz(database, &localization) {
            Ok(loaded) => {
                loaded_databases += 1;
                for record in loaded {
                    records.insert(record.record.clone(), record);
                }
            }
            Err(error) => {
                warnings.push(format!("Skipped database {}: {error}", database.display()))
            }
        }
    }
    if loaded_databases == 0 {
        return Err(DatabaseError::NoUsableDatabase(warnings.join(" | ")));
    }

    let record_names = records
        .values()
        .filter(|record| !record.display_name.trim().is_empty())
        .map(|record| {
            (
                record.record.to_ascii_lowercase(),
                record.display_name.clone(),
            )
        })
        .collect();
    let mut catalog = records
        .into_values()
        .filter_map(RecordSummary::into_catalog_item)
        .collect::<Vec<_>>();
    catalog.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.record.cmp(&right.record))
    });
    Ok(LoadedGameDatabase {
        catalog,
        localization,
        record_names,
        warnings,
    })
}

pub fn load_resource_index(
    location: &GameDatabaseLocation,
) -> Result<GameResourceIndex, DatabaseError> {
    Ok(GameResourceIndex {
        archives: location
            .resource_files
            .iter()
            .map(|path| ArcArchiveIndex::open(path))
            .collect::<Result<_, _>>()?,
    })
}

impl GameResourceIndex {
    pub fn read(&self, resource_path: &str) -> Result<Option<Vec<u8>>, DatabaseError> {
        let candidates = resource_path_candidates(resource_path);
        for archive in self.archives.iter().rev() {
            for candidate in &candidates {
                if let Some(contents) = archive.read(candidate)? {
                    return Ok(Some(contents));
                }
            }
        }
        Ok(None)
    }

    pub fn thumbnail_png(
        &self,
        resource_path: &str,
        max_size: u32,
    ) -> Result<Option<Vec<u8>>, DatabaseError> {
        let Some(texture) = self.read(resource_path)? else {
            return Ok(None);
        };
        let dds_start = texture
            .windows(4)
            .position(|window| window == b"DDS " || window == b"DDSR")
            .ok_or_else(|| DatabaseError::InvalidTexture("DDS header is missing".into()))?;
        let mut dds_bytes = texture[dds_start..].to_vec();
        if dds_bytes.starts_with(b"DDSR") {
            dds_bytes[3] = b' ';
            patch_grim_dawn_ddsr_header(&mut dds_bytes)?;
        }
        let dds = image_dds::ddsfile::Dds::read(&mut IoCursor::new(&dds_bytes))
            .map_err(|error| DatabaseError::InvalidTexture(error.to_string()))?;
        let decoded = image_dds::image_from_dds(&dds, 0)
            .map_err(|error| DatabaseError::Image(error.to_string()))?;
        let resized = image::DynamicImage::ImageRgba8(decoded).resize(
            max_size.max(1),
            max_size.max(1),
            image::imageops::FilterType::Lanczos3,
        );
        let mut output = IoCursor::new(Vec::new());
        resized
            .write_to(&mut output, image::ImageFormat::Png)
            .map_err(|error| DatabaseError::Image(error.to_string()))?;
        Ok(Some(output.into_inner()))
    }
}

fn patch_grim_dawn_ddsr_header(bytes: &mut [u8]) -> Result<(), DatabaseError> {
    if bytes.len() < 128 {
        return Err(DatabaseError::InvalidTexture(
            "DDSR header is truncated".into(),
        ));
    }
    let bit_count = u32::from_le_bytes(bytes[88..92].try_into().expect("DDS range checked"));
    let masks_are_empty = bytes[92..108].iter().all(|byte| *byte == 0);
    if bit_count == 32 && masks_are_empty {
        // Grim Dawn's DDSR variant omits the standard BGRA channel masks.
        bytes[80..84].copy_from_slice(&0x41_u32.to_le_bytes());
        bytes[92..96].copy_from_slice(&0x00ff_0000_u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&0x0000_ff00_u32.to_le_bytes());
        bytes[100..104].copy_from_slice(&0x0000_00ff_u32.to_le_bytes());
        bytes[104..108].copy_from_slice(&0xff00_0000_u32.to_le_bytes());
    }
    Ok(())
}

impl ArcArchiveIndex {
    fn open(path: &Path) -> Result<Self, DatabaseError> {
        let mut file = File::open(path)?;
        if read_i32(&mut file)? != 0x435241 || read_i32(&mut file)? != 3 {
            return Err(DatabaseError::InvalidArc);
        }
        let file_count = read_i32(&mut file)?.max(0) as usize;
        read_i32(&mut file)?;
        let record_table_size = read_i32(&mut file)?.max(0) as u64;
        let string_table_size = read_i32(&mut file)?.max(0) as usize;
        let record_table_offset = read_i32(&mut file)?.max(0) as u64;

        file.seek(SeekFrom::Start(record_table_offset + record_table_size))?;
        let mut string_table = vec![0; string_table_size];
        file.read_exact(&mut string_table)?;
        let names = string_table
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .map(|name| normalize_resource_path(&String::from_utf8_lossy(name)))
            .collect::<Vec<_>>();
        if names.len() < file_count {
            return Err(DatabaseError::InvalidTexture(format!(
                "ARC contains {file_count} entries but only {} names",
                names.len()
            )));
        }

        file.seek(SeekFrom::Start(
            record_table_offset + record_table_size + string_table_size as u64,
        ))?;
        let mut entries = HashMap::with_capacity(file_count);
        for name in names.into_iter().take(file_count) {
            let entry_type = read_i32(&mut file)?;
            let offset = read_i32(&mut file)?;
            let compressed_size = read_i32(&mut file)?;
            let decompressed_size = read_i32(&mut file)?;
            read_i32(&mut file)?;
            read_i64(&mut file)?;
            let parts = read_i32(&mut file)?;
            let first_part = read_i32(&mut file)?;
            read_i32(&mut file)?;
            read_i32(&mut file)?;
            entries.insert(
                name,
                ArcEntry {
                    entry_type,
                    offset,
                    compressed_size,
                    decompressed_size,
                    parts,
                    first_part,
                },
            );
        }
        Ok(Self {
            path: path.to_path_buf(),
            record_table_offset,
            entries,
        })
    }

    fn read(&self, resource_path: &str) -> Result<Option<Vec<u8>>, DatabaseError> {
        let Some(entry) = self.entries.get(resource_path) else {
            return Ok(None);
        };
        let mut file = File::open(&self.path)?;
        if entry.entry_type == 1 && entry.compressed_size == entry.decompressed_size {
            return Ok(Some(read_file_slice(
                &mut file,
                entry.offset,
                entry.compressed_size,
            )?));
        }

        let mut output = Vec::with_capacity(entry.decompressed_size.max(0) as usize);
        for index in 0..entry.parts.max(0) {
            let part_header = self.record_table_offset
                + u64::try_from((entry.first_part + index).max(0)).unwrap_or(0) * 12;
            file.seek(SeekFrom::Start(part_header))?;
            let offset = read_i32(&mut file)?;
            let compressed = read_i32(&mut file)?;
            let decompressed = read_i32(&mut file)?;
            let source = read_file_slice(&mut file, offset, compressed)?;
            if compressed == decompressed {
                output.extend_from_slice(&source);
            } else {
                output.extend_from_slice(
                    &decompress(&source, decompressed.max(0) as usize)
                        .map_err(|error| DatabaseError::Lz4(error.to_string()))?,
                );
            }
        }
        Ok(Some(output))
    }
}

fn read_file_slice(file: &mut File, offset: i32, length: i32) -> Result<Vec<u8>, DatabaseError> {
    let mut output = vec![0; length.max(0) as usize];
    file.seek(SeekFrom::Start(offset.max(0) as u64))?;
    file.read_exact(&mut output)?;
    Ok(output)
}

fn read_i32(reader: &mut impl Read) -> Result<i32, DatabaseError> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_i64(reader: &mut impl Read) -> Result<i64, DatabaseError> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(i64::from_le_bytes(bytes))
}

fn normalize_resource_path(path: &str) -> String {
    path.trim_matches(['/', '\\'])
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn resource_path_candidates(path: &str) -> Vec<String> {
    let normalized = normalize_resource_path(path);
    let mut candidates = vec![normalized.clone()];
    for prefix in ["items/", "ui/"] {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            candidates.push(stripped.to_owned());
        }
    }
    candidates
}

fn discover_install_roots() -> BTreeSet<PathBuf> {
    let mut roots = BTreeSet::new();
    for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(path) = env::var_os(variable).map(PathBuf::from) {
            roots.insert(path.join("Steam/steamapps/common/Grim Dawn"));
        }
    }

    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        if let Ok(steam) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Valve\\Steam")
            && let Ok::<String, _>(path) = steam.get_value("SteamPath")
        {
            roots.insert(PathBuf::from(path).join("steamapps/common/Grim Dawn"));
        }
    }

    let libraries = roots
        .iter()
        .filter_map(|candidate| candidate.ancestors().nth(3))
        .map(|steam| steam.join("steamapps/libraryfolders.vdf"))
        .filter_map(|path| fs::read_to_string(path).ok())
        .flat_map(|contents| steam_library_paths(&contents))
        .collect::<Vec<_>>();
    roots.extend(
        libraries
            .into_iter()
            .map(|path| path.join("steamapps/common/Grim Dawn")),
    );
    roots
}

fn steam_library_paths(contents: &str) -> Vec<PathBuf> {
    contents
        .lines()
        .filter_map(|line| {
            let mut quoted = line.split('"').skip(1).step_by(2);
            let key = quoted.next()?;
            let value = quoted.next()?;
            (key == "path").then(|| PathBuf::from(value.replace("\\\\", "\\")))
        })
        .collect()
}

fn location_at(root: &Path) -> Option<GameDatabaseLocation> {
    let candidates = [
        root.join("database/database.arz"),
        root.join("gdx1/database/GDX1.arz"),
        root.join("gdx2/database/GDX2.arz"),
        root.join("gdx3/database/GDX3.arz"),
    ];
    let database_files = candidates
        .into_iter()
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    if database_files.is_empty() {
        return None;
    }
    let localization_candidates = [
        root.join("resources/Text_EN.arc"),
        root.join("gdx1/resources/Text_EN.arc"),
        root.join("gdx2/resources/Text_EN.arc"),
        root.join("gdx3/resources/Text_EN.arc"),
    ];
    let resource_candidates = [
        root.join("resources/Items.arc"),
        root.join("resources/UI.arc"),
        root.join("gdx1/resources/Items.arc"),
        root.join("gdx1/resources/UI.arc"),
        root.join("gdx2/resources/Items.arc"),
        root.join("gdx2/resources/UI.arc"),
        root.join("gdx3/resources/Items.arc"),
        root.join("gdx3/resources/UI.arc"),
    ];
    Some(GameDatabaseLocation {
        install_path: root.to_path_buf(),
        database_files,
        localization_files: localization_candidates
            .into_iter()
            .filter(|path| path.is_file())
            .collect(),
        resource_files: resource_candidates
            .into_iter()
            .filter(|path| path.is_file())
            .collect(),
    })
}

#[derive(Default)]
struct RecordSummary {
    record: String,
    class_name: String,
    display_name: String,
    quality_name: String,
    affix_name: String,
    icon_path: String,
    level_requirement: Option<i32>,
    item_level: Option<i32>,
}

impl RecordSummary {
    fn into_catalog_item(self) -> Option<CatalogItem> {
        let record_lower = self.record.to_ascii_lowercase();
        let class_lower = self.class_name.to_ascii_lowercase();
        if !record_lower.starts_with("records/items/") {
            return None;
        }
        let kind = if record_lower.contains("/lootaffixes/ascended/") {
            CatalogKind::Ascendant
        } else if record_lower.contains("/prefix/") {
            CatalogKind::Prefix
        } else if record_lower.contains("/suffix/") {
            CatalogKind::Suffix
        } else if class_lower.contains("ascend") {
            CatalogKind::Ascendant
        } else if class_lower == "itemenchantment" {
            CatalogKind::Augment
        } else if class_lower == "itemrelic"
            && (record_lower.contains("/materia/") || record_lower.contains("/crafting/materials/"))
        {
            CatalogKind::Component
        } else if class_lower.contains("fixeditem") || class_lower.contains("table") {
            return None;
        } else if ["item", "armor", "weapon", "oneshot", "formula", "relic"]
            .iter()
            .any(|candidate| class_lower.contains(candidate))
        {
            CatalogKind::Base
        } else {
            return None;
        };
        let core_name = if matches!(
            kind,
            CatalogKind::Prefix | CatalogKind::Suffix | CatalogKind::Ascendant
        ) && !self.affix_name.is_empty()
        {
            self.affix_name
        } else {
            self.display_name
        };
        let name = [self.quality_name, core_name]
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .replace("^k", "")
            .trim()
            .to_owned();
        if name.is_empty() {
            return None;
        }
        Some(CatalogItem {
            record: self.record,
            name,
            class_name: self.class_name,
            kind,
            icon_path: (!self.icon_path.is_empty()).then_some(self.icon_path),
            level_requirement: self.level_requirement,
            item_level: self.item_level,
        })
    }
}

fn load_arz(
    path: &Path,
    localization: &HashMap<String, String>,
) -> Result<Vec<RecordSummary>, DatabaseError> {
    let bytes = fs::read(path)?;
    let mut cursor = Cursor::new(&bytes);
    cursor.i16()?;
    cursor.i16()?;
    let record_table_start = cursor.i32()? as usize;
    cursor.i32()?;
    let record_count = cursor.i32()?;
    let string_table_start = cursor.i32()? as usize;
    cursor.i32()?;

    cursor.position = string_table_start;
    let string_count = cursor.i32()?;
    let mut strings = Vec::with_capacity(string_count.max(0) as usize);
    for _ in 0..string_count {
        let length = cursor.i32()?;
        strings.push(cursor.text(length)?);
    }

    cursor.position = record_table_start;
    let mut headers = Vec::with_capacity(record_count.max(0) as usize);
    for _ in 0..record_count {
        let filename = lookup(&strings, cursor.i32()?)?.to_owned();
        let type_length = cursor.i32()?;
        cursor.skip(type_length.max(0) as usize)?;
        let offset = cursor.i32()?;
        let compressed_size = cursor.i32()?;
        let decompressed_size = cursor.i32()?;
        cursor.i32()?;
        cursor.i32()?;
        headers.push((filename, offset, compressed_size, decompressed_size));
    }

    let mut records = Vec::with_capacity(headers.len());
    for (record, offset, compressed_size, decompressed_size) in headers {
        let start = 24_usize
            .checked_add(offset.max(0) as usize)
            .ok_or(DatabaseError::Truncated { position: 24 })?;
        let end = start
            .checked_add(compressed_size.max(0) as usize)
            .ok_or(DatabaseError::Truncated { position: start })?;
        let compressed = bytes
            .get(start..end)
            .ok_or(DatabaseError::Truncated { position: start })?;
        let decompressed = decompress(compressed, decompressed_size.max(0) as usize)
            .map_err(|error| DatabaseError::Lz4(error.to_string()))?;
        let summary = parse_record(&record, &decompressed, &strings, localization)?;
        records.push(summary);
    }
    Ok(records)
}

fn parse_record(
    record: &str,
    bytes: &[u8],
    strings: &[String],
    localization: &HashMap<String, String>,
) -> Result<RecordSummary, DatabaseError> {
    let mut cursor = Cursor::new(bytes);
    let mut result = RecordSummary {
        record: record.to_owned(),
        ..Default::default()
    };
    while cursor.position < bytes.len() {
        let value_type = cursor.i16()?;
        let count = cursor.i16()?.max(0) as usize;
        let field = lookup(strings, cursor.i32()?)?;
        for index in 0..count {
            let raw = cursor.i32()?;
            if index != 0 {
                continue;
            }
            let text = if value_type == 2 {
                let value = lookup(strings, raw)?;
                localization.get(value).map(String::as_str).unwrap_or(value)
            } else {
                ""
            };
            if value_type == 2
                && result.icon_path.is_empty()
                && !text.is_empty()
                && field.to_ascii_lowercase().contains("bitmap")
            {
                result.icon_path = text.to_owned();
            }
            match field {
                "Class" if value_type == 2 => result.class_name = text.to_owned(),
                "itemNameTag" | "skillDisplayName" | "skillName" | "description"
                    if value_type == 2 && result.display_name.is_empty() =>
                {
                    result.display_name = text.to_owned();
                }
                "itemQualityTag" | "itemStyleTag"
                    if value_type == 2 && result.quality_name.is_empty() =>
                {
                    result.quality_name = text.to_owned();
                }
                "lootRandomizerName" if value_type == 2 => result.affix_name = text.to_owned(),
                "levelRequirement" if value_type != 2 => result.level_requirement = Some(raw),
                "itemLevel" if value_type != 2 => result.item_level = Some(raw),
                _ => {}
            }
        }
    }
    Ok(result)
}

fn load_localization(path: &Path) -> Result<HashMap<String, String>, DatabaseError> {
    let bytes = fs::read(path)?;
    let mut cursor = Cursor::new(&bytes);
    if cursor.i32()? != 0x435241 || cursor.i32()? != 3 {
        return Err(DatabaseError::InvalidArc);
    }
    let file_entries = cursor.i32()?;
    cursor.i32()?;
    let record_table_size = cursor.i32()?;
    let string_table_size = cursor.i32()?;
    let record_table_offset = cursor.i32()?;
    cursor.position = (record_table_offset + record_table_size + string_table_size) as usize;
    let mut headers = Vec::with_capacity(file_entries.max(0) as usize);
    for _ in 0..file_entries {
        let entry_type = cursor.i32()?;
        let offset = cursor.i32()?;
        let compressed_size = cursor.i32()?;
        let decompressed_size = cursor.i32()?;
        cursor.i32()?;
        cursor.i64()?;
        let file_parts = cursor.i32()?;
        let first_part_index = cursor.i32()?;
        cursor.i32()?;
        cursor.i32()?;
        headers.push((
            entry_type,
            offset,
            compressed_size,
            decompressed_size,
            file_parts,
            first_part_index,
        ));
    }
    let mut table = HashMap::new();
    for (entry_type, offset, compressed_size, decompressed_size, parts, first_part) in headers {
        let contents = if entry_type == 1 && compressed_size == decompressed_size {
            slice(&bytes, offset, compressed_size)?.to_vec()
        } else {
            let mut output = Vec::with_capacity(decompressed_size.max(0) as usize);
            for index in 0..parts.max(0) {
                let header_offset =
                    record_table_offset as usize + (first_part + index).max(0) as usize * 12;
                let mut part = Cursor::at(&bytes, header_offset);
                let part_offset = part.i32()?;
                let part_compressed = part.i32()?;
                let part_decompressed = part.i32()?;
                let source = slice(&bytes, part_offset, part_compressed)?;
                if part_compressed == part_decompressed {
                    output.extend_from_slice(source);
                } else {
                    output.extend_from_slice(
                        &decompress(source, part_decompressed.max(0) as usize)
                            .map_err(|error| DatabaseError::Lz4(error.to_string()))?,
                    );
                }
            }
            output
        };
        let text = String::from_utf8_lossy(&contents).replace('\u{feff}', "");
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                table.insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }
    }
    Ok(table)
}

fn slice(bytes: &[u8], offset: i32, length: i32) -> Result<&[u8], DatabaseError> {
    let start = offset.max(0) as usize;
    let end = start
        .checked_add(length.max(0) as usize)
        .ok_or(DatabaseError::Truncated { position: start })?;
    bytes
        .get(start..end)
        .ok_or(DatabaseError::Truncated { position: start })
}

fn lookup(strings: &[String], index: i32) -> Result<&str, DatabaseError> {
    strings
        .get(index.max(0) as usize)
        .map(String::as_str)
        .ok_or(DatabaseError::InvalidStringIndex(index))
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn at(bytes: &'a [u8], position: usize) -> Self {
        Self { bytes, position }
    }

    fn read<const N: usize>(&mut self) -> Result<[u8; N], DatabaseError> {
        let end = self.position + N;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(DatabaseError::Truncated {
                position: self.position,
            })?;
        self.position = end;
        Ok(value.try_into().expect("slice length checked"))
    }

    fn i16(&mut self) -> Result<i16, DatabaseError> {
        Ok(i16::from_le_bytes(self.read()?))
    }

    fn i32(&mut self) -> Result<i32, DatabaseError> {
        Ok(i32::from_le_bytes(self.read()?))
    }

    fn i64(&mut self) -> Result<i64, DatabaseError> {
        Ok(i64::from_le_bytes(self.read()?))
    }

    fn skip(&mut self, length: usize) -> Result<(), DatabaseError> {
        let end = self.position + length;
        if end > self.bytes.len() {
            return Err(DatabaseError::Truncated {
                position: self.position,
            });
        }
        self.position = end;
        Ok(())
    }

    fn text(&mut self, length: i32) -> Result<String, DatabaseError> {
        let start = self.position;
        self.skip(length.max(0) as usize)?;
        Ok(String::from_utf8_lossy(&self.bytes[start..self.position]).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_candidates_cover_items_and_ui_archive_roots() {
        assert_eq!(
            resource_path_candidates("Items\\GearWeapons\\icon.tex"),
            ["items/gearweapons/icon.tex", "gearweapons/icon.tex"]
        );
        assert_eq!(
            resource_path_candidates("ui/icons/icon.tex"),
            ["ui/icons/icon.tex", "icons/icon.tex"]
        );
    }

    #[test]
    fn grim_dawn_ddsr_header_receives_standard_bgra_masks() {
        let mut header = [0_u8; 128];
        header[..4].copy_from_slice(b"DDS ");
        header[88..92].copy_from_slice(&32_u32.to_le_bytes());

        patch_grim_dawn_ddsr_header(&mut header).expect("valid DDSR header");

        assert_eq!(u32::from_le_bytes(header[80..84].try_into().unwrap()), 0x41);
        assert_eq!(
            u32::from_le_bytes(header[92..96].try_into().unwrap()),
            0x00ff_0000
        );
        assert_eq!(
            u32::from_le_bytes(header[96..100].try_into().unwrap()),
            0x0000_ff00
        );
        assert_eq!(
            u32::from_le_bytes(header[100..104].try_into().unwrap()),
            0x0000_00ff
        );
        assert_eq!(
            u32::from_le_bytes(header[104..108].try_into().unwrap()),
            0xff00_0000
        );
    }

    #[test]
    fn truncated_ddsr_header_is_rejected() {
        let error = patch_grim_dawn_ddsr_header(&mut [0_u8; 127]).unwrap_err();
        assert!(matches!(error, DatabaseError::InvalidTexture(_)));
    }
}
