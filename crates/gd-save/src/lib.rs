//! Grim Dawn `player.gdc` container reader and writer.
//!
//! The binary behavior is independently expressed in Rust using the public
//! gd-edit implementation as a compatibility reference. Adapted portions of
//! this crate are distributed under EPL-1.0; see `docs/ATTRIBUTION.md`.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

mod items;
mod progression;
pub use items::{CharacterItem, ItemContainer, ItemPatch, NewInventoryItem};
pub use progression::{CharacterSkill, FactionPatch, FactionValue};

const SEED_MASK: u32 = 0x5555_5555;
const GDC_MAGIC: i32 = i32::from_le_bytes(*b"GDCX");
const CHARACTER_BLOCK_ID: i32 = 1;
const CORE_BLOCK_ID: i32 = 2;
const CORE_BLOCK_SIZE: usize = 48;
const IRON_OFFSET: usize = 8;

#[derive(Debug, Error)]
pub enum GdcError {
    #[error("truncated file at position {position}; {needed} bytes were required")]
    Truncated { position: usize, needed: usize },
    #[error("the file does not contain the GDCX signature")]
    InvalidMagic,
    #[error("unsupported GDC data version: {0}")]
    UnsupportedDataVersion(i32),
    #[error("invalid checksum in {section}: expected {expected:#010x}, found {actual:#010x}")]
    InvalidChecksum {
        section: String,
        expected: u32,
        actual: u32,
    },
    #[error("invalid length in block {block_id}: {length}")]
    InvalidBlockLength { block_id: i32, length: i32 },
    #[error("invalid {encoding} text")]
    InvalidText { encoding: &'static str },
    #[error("the attribute block is shorter than expected")]
    InvalidCoreBlock,
    #[error("invalid value: {0}")]
    InvalidValue(String),
    #[error(
        "writing this save is disabled because these encrypted blocks are not fully mapped: {block_ids:?}"
    )]
    UnsafeMutation { block_ids: Vec<i32> },
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterHeader {
    pub character_name: String,
    pub male: bool,
    pub class_name: String,
    pub character_level: i32,
    pub hardcore: bool,
    pub expansion_character: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreStats {
    pub level_in_bio: i32,
    pub experience: i32,
    pub attribute_points: i32,
    pub skill_points: i32,
    pub devotion_points: i32,
    pub total_devotion_points_unlocked: i32,
    pub physique: f32,
    pub cunning: f32,
    pub spirit: f32,
    pub health: f32,
    pub energy: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GdcBlock {
    pub id: i32,
    /// Fully decrypted block payload, including its internal version field.
    pub payload: Vec<u8>,
    markers: Vec<RawMarker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawMarker {
    offset: usize,
    kind: RawMarkerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawMarkerKind {
    CipherI32,
    Length,
    Checksum,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GdcDocument {
    pub seed: u32,
    pub file_version: i32,
    pub header: CharacterHeader,
    expansion_raw: u8,
    pub data_version: i32,
    pub mystery: [u8; 16],
    pub blocks: Vec<GdcBlock>,
}

impl GdcDocument {
    /// Returns encrypted blocks whose typed field layout is not fully known.
    ///
    /// Opaque payloads can be preserved during an exact, unchanged round-trip,
    /// but cannot safely be re-encrypted after an earlier field changes. The
    /// cipher state evolves differently for typed values and raw bytes.
    pub fn mutation_blockers(&self) -> Vec<i32> {
        self.blocks
            .iter()
            .filter(|block| !block.payload.is_empty() && block.markers.is_empty())
            .map(|block| block.id)
            .collect()
    }

    pub fn mutation_supported(&self) -> bool {
        self.mutation_blockers().is_empty()
    }

    pub fn iron(&self) -> Result<Option<i32>, GdcError> {
        self.blocks
            .iter()
            .find(|block| block.id == CHARACTER_BLOCK_ID)
            .map(|block| plain_i32_at(&block.payload, IRON_OFFSET))
            .transpose()
    }

    pub fn set_iron(&mut self, iron: i32) -> Result<(), GdcError> {
        let block = self
            .blocks
            .iter_mut()
            .find(|block| block.id == CHARACTER_BLOCK_ID)
            .ok_or_else(|| GdcError::InvalidValue("character state block is missing".into()))?;
        let target = block
            .payload
            .get_mut(IRON_OFFSET..IRON_OFFSET + 4)
            .ok_or_else(|| GdcError::InvalidValue("character state block is truncated".into()))?;
        target.copy_from_slice(&iron.to_le_bytes());
        Ok(())
    }

    pub fn core_stats(&self) -> Result<Option<CoreStats>, GdcError> {
        self.blocks
            .iter()
            .find(|block| block.id == CORE_BLOCK_ID)
            .map(|block| CoreStats::from_payload(&block.payload))
            .transpose()
    }

    pub fn set_core_stats(&mut self, stats: &CoreStats) -> Result<(), GdcError> {
        let block = self
            .blocks
            .iter_mut()
            .find(|block| block.id == CORE_BLOCK_ID)
            .ok_or(GdcError::InvalidCoreBlock)?;
        stats.write_payload(&mut block.payload)
    }
}

impl CoreStats {
    fn from_payload(payload: &[u8]) -> Result<Self, GdcError> {
        if payload.len() < CORE_BLOCK_SIZE {
            return Err(GdcError::InvalidCoreBlock);
        }
        let mut cursor = PlainCursor::new(payload);
        let _block_version = cursor.i32()?;
        Ok(Self {
            level_in_bio: cursor.i32()?,
            experience: cursor.i32()?,
            attribute_points: cursor.i32()?,
            skill_points: cursor.i32()?,
            devotion_points: cursor.i32()?,
            total_devotion_points_unlocked: cursor.i32()?,
            physique: cursor.f32()?,
            cunning: cursor.f32()?,
            spirit: cursor.f32()?,
            health: cursor.f32()?,
            energy: cursor.f32()?,
        })
    }

    fn write_payload(&self, payload: &mut [u8]) -> Result<(), GdcError> {
        if payload.len() < CORE_BLOCK_SIZE {
            return Err(GdcError::InvalidCoreBlock);
        }
        let mut offset = 4;
        for value in [
            self.level_in_bio,
            self.experience,
            self.attribute_points,
            self.skill_points,
            self.devotion_points,
            self.total_devotion_points_unlocked,
        ] {
            payload[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            offset += 4;
        }
        for value in [
            self.physique,
            self.cunning,
            self.spirit,
            self.health,
            self.energy,
        ] {
            payload[offset..offset + 4].copy_from_slice(&value.to_bits().to_le_bytes());
            offset += 4;
        }
        Ok(())
    }
}

pub fn read_file(path: impl AsRef<Path>) -> Result<GdcDocument, GdcError> {
    parse(&std::fs::read(path)?)
}

pub fn read_summary(path: impl AsRef<Path>) -> Result<(CharacterHeader, i32), GdcError> {
    let document = read_file(path)?;
    Ok((document.header, document.data_version))
}

pub fn parse(bytes: &[u8]) -> Result<GdcDocument, GdcError> {
    let mut position = 0;
    let stored_seed = raw_u32(bytes, &mut position)?;
    let seed = stored_seed ^ SEED_MASK;
    let mut cipher = Cipher::new(seed);

    let magic = cipher.read_i32(bytes, &mut position)?;
    if magic != GDC_MAGIC {
        return Err(GdcError::InvalidMagic);
    }
    let file_version = cipher.read_i32(bytes, &mut position)?;
    let character_name = cipher.read_utf16(bytes, &mut position)?;
    let male = cipher.read_bool(bytes, &mut position)?;
    let class_name = cipher.read_ascii(bytes, &mut position)?;
    let character_level = cipher.read_i32(bytes, &mut position)?;
    let hardcore = cipher.read_bool(bytes, &mut position)?;
    let expansion_raw = cipher.read_byte(bytes, &mut position)?;
    let header = CharacterHeader {
        character_name,
        male,
        class_name,
        character_level,
        hardcore,
        expansion_character: expansion_raw != 0,
    };
    verify_checksum(bytes, &mut position, cipher.state, "header")?;

    let data_version = cipher.read_i32(bytes, &mut position)?;
    if !matches!(data_version, 6..=8) {
        return Err(GdcError::UnsupportedDataVersion(data_version));
    }
    let mystery_bytes = cipher.read_bytes(bytes, &mut position, 16)?;
    let mut mystery = [0_u8; 16];
    mystery.copy_from_slice(&mystery_bytes);

    let mut blocks = Vec::new();
    while position < bytes.len() {
        let id = cipher.read_i32(bytes, &mut position)?;
        let encrypted_length = raw_u32(bytes, &mut position)?;
        let length = (encrypted_length ^ cipher.state) as i32;
        if length < 0 || length as usize > bytes.len().saturating_sub(position) {
            return Err(GdcError::InvalidBlockLength {
                block_id: id,
                length,
            });
        }
        let (payload, markers) = if id == CHARACTER_BLOCK_ID {
            read_character_payload(bytes, &mut position, length as usize, &mut cipher)?
        } else if id == CORE_BLOCK_ID {
            read_core_payload(bytes, &mut position, length as usize, &mut cipher)?
        } else if id == 8 {
            // Fangs of Asterkarn introduced a newer skill payload. Parse the
            // layouts we understand on a cloned cipher and fall back to an
            // opaque byte-for-byte payload when the layout is newer. This
            // keeps the entire save readable and exactly round-trippable.
            let mut trial_cipher = cipher.clone();
            let mut trial_position = position;
            match read_skills_payload(
                bytes,
                &mut trial_position,
                length as usize,
                &mut trial_cipher,
            ) {
                Ok(result) => {
                    cipher = trial_cipher;
                    position = trial_position;
                    result
                }
                Err(_) => (
                    cipher.read_bytes(bytes, &mut position, length as usize)?,
                    Vec::new(),
                ),
            }
        } else if id == 13 {
            read_factions_payload(bytes, &mut position, length as usize, &mut cipher)?
        } else if matches!(id, 3 | 4) {
            read_nested_payload(bytes, &mut position, length as usize, &mut cipher, id)?
        } else if matches!(id, 5 | 6 | 7 | 10 | 12 | 14 | 15 | 16 | 17) {
            // These blocks are not edited by Dreeg, but their scalar layout
            // must still be known so an earlier mutation can safely advance
            // the chained cipher. Try the verified 1.2.1.6/1.3.0 layouts and
            // retain an opaque fallback for future versions.
            let mut trial_cipher = cipher.clone();
            let mut trial_position = position;
            match read_preserved_typed_payload(
                bytes,
                &mut trial_position,
                length as usize,
                &mut trial_cipher,
                id,
            ) {
                Ok(result) => {
                    cipher = trial_cipher;
                    position = trial_position;
                    result
                }
                Err(_) => (
                    cipher.read_bytes(bytes, &mut position, length as usize)?,
                    Vec::new(),
                ),
            }
        } else {
            (
                cipher.read_bytes(bytes, &mut position, length as usize)?,
                Vec::new(),
            )
        };
        verify_checksum(bytes, &mut position, cipher.state, &format!("block {id}"))?;
        blocks.push(GdcBlock {
            id,
            payload,
            markers,
        });
    }

    Ok(GdcDocument {
        seed,
        file_version,
        header,
        expansion_raw,
        data_version,
        mystery,
        blocks,
    })
}

fn read_character_payload(
    input: &[u8],
    position: &mut usize,
    length: usize,
    cipher: &mut Cipher,
) -> Result<(Vec<u8>, Vec<RawMarker>), GdcError> {
    let end = position
        .checked_add(length)
        .filter(|end| *end <= input.len())
        .ok_or(GdcError::InvalidBlockLength {
            block_id: CHARACTER_BLOCK_ID,
            length: length as i32,
        })?;
    let mut reader = CharacterPayloadReader {
        input,
        position: *position,
        end,
        cipher,
        payload: Vec::with_capacity(length),
        markers: Vec::new(),
    };

    let version = reader.i32()?;
    reader.byte()?; // in main quest
    reader.byte()?; // has been in game
    reader.byte()?; // last difficulty
    reader.byte()?; // greatest difficulty completed
    reader.i32()?; // iron
    reader.byte()?; // greatest survival difficulty completed
    reader.i32()?; // tributes
    reader.byte()?; // UI compass state
    if (2..=4).contains(&version) {
        reader.i32()?; // legacy always-show-loot setting
    }
    reader.byte()?; // show skill help
    reader.byte()?; // alternate weapon set active
    reader.byte()?; // alternate weapon set enabled
    reader.ascii()?; // player texture
    if version >= 5 {
        let filter_count = reader.i32()?;
        if filter_count < 0 || filter_count as usize > reader.end.saturating_sub(reader.position) {
            return Err(GdcError::InvalidValue(format!(
                "invalid loot-filter count in character block: {filter_count}"
            )));
        }
        for _ in 0..filter_count {
            reader.byte()?;
        }
    }
    if reader.position != reader.end {
        return Err(GdcError::InvalidValue(format!(
            "character block v{version} left {} untyped bytes",
            reader.end.saturating_sub(reader.position)
        )));
    }
    *position = reader.position;
    Ok((reader.payload, reader.markers))
}

struct CharacterPayloadReader<'input, 'cipher> {
    input: &'input [u8],
    position: usize,
    end: usize,
    cipher: &'cipher mut Cipher,
    payload: Vec<u8>,
    markers: Vec<RawMarker>,
}

impl CharacterPayloadReader<'_, '_> {
    fn byte(&mut self) -> Result<u8, GdcError> {
        if self.position >= self.end {
            return Err(GdcError::InvalidValue(
                "character state block is truncated".into(),
            ));
        }
        let value = self.cipher.read_byte(self.input, &mut self.position)?;
        self.payload.push(value);
        Ok(value)
    }

    fn i32(&mut self) -> Result<i32, GdcError> {
        if self.end.saturating_sub(self.position) < 4 {
            return Err(GdcError::InvalidValue(
                "character state block is truncated".into(),
            ));
        }
        self.markers.push(RawMarker {
            offset: self.payload.len(),
            kind: RawMarkerKind::CipherI32,
        });
        let value = self.cipher.read_i32(self.input, &mut self.position)?;
        self.payload.extend_from_slice(&value.to_le_bytes());
        Ok(value)
    }

    fn ascii(&mut self) -> Result<(), GdcError> {
        let length = self.i32()?;
        if length < 0 || length as usize > self.end.saturating_sub(self.position) {
            return Err(GdcError::InvalidValue(
                "invalid text length in character state block".into(),
            ));
        }
        for _ in 0..length {
            self.byte()?;
        }
        Ok(())
    }
}

fn plain_i32_at(payload: &[u8], offset: usize) -> Result<i32, GdcError> {
    let value = payload
        .get(offset..offset + 4)
        .ok_or_else(|| GdcError::InvalidValue("character state block is truncated".into()))?;
    Ok(i32::from_le_bytes(
        value.try_into().expect("slice length checked"),
    ))
}

fn read_skills_payload(
    input: &[u8],
    position: &mut usize,
    length: usize,
    cipher: &mut Cipher,
) -> Result<(Vec<u8>, Vec<RawMarker>), GdcError> {
    let mut reader = TypedPayloadReader::new(input, *position, length, cipher, 8)?;
    let version = reader.i32()?;
    reader.version = version;
    if !matches!(version, 5 | 6 | 8) {
        return Err(GdcError::InvalidValue(format!(
            "unsupported skills block version: {version}"
        )));
    }
    let skill_count = reader.count(65_536, "skills")?;
    for _ in 0..skill_count {
        reader.ascii()?;
        reader.i32()?;
        reader.byte()?;
        if version >= 8 {
            reader.byte()?; // skill locked
        }
        reader.i32()?;
        reader.i32()?;
        reader.i32()?;
        reader.byte()?;
        reader.byte()?;
        reader.ascii()?;
        reader.ascii()?;
    }
    reader.i32()?; // masteries allowed
    reader.i32()?; // reclaimed skill points
    reader.i32()?; // reclaimed devotion points
    let item_skill_count = reader.count(65_536, "item skills")?;
    for _ in 0..item_skill_count {
        reader.ascii()?;
        reader.ascii()?;
        reader.ascii()?;
        reader.i32()?; // equipment location
        reader.ascii()?;
    }
    if version >= 6 {
        let sub_skill_count = reader.count(65_536, "subskills")?;
        for _ in 0..sub_skill_count {
            reader.ascii()?;
            reader.ascii()?;
            reader.ascii()?;
            reader.ascii()?;
        }
    }
    reader.finish_exact()?;
    *position = reader.position;
    Ok((reader.payload, reader.markers))
}

fn read_factions_payload(
    input: &[u8],
    position: &mut usize,
    length: usize,
    cipher: &mut Cipher,
) -> Result<(Vec<u8>, Vec<RawMarker>), GdcError> {
    let mut reader = TypedPayloadReader::new(input, *position, length, cipher, 13)?;
    let version = reader.i32()?;
    reader.version = version;
    if version != 5 {
        return Err(GdcError::InvalidValue(format!(
            "unsupported factions block version: {version}"
        )));
    }
    reader.i32()?; // player faction
    let faction_count = reader.count(16_384, "factions")?;
    for _ in 0..faction_count {
        reader.byte()?;
        reader.byte()?;
        reader.i32()?; // reputation value as f32 bits
        reader.i32()?; // positive boost as f32 bits
        reader.i32()?; // negative boost as f32 bits
    }
    reader.finish_exact()?;
    *position = reader.position;
    Ok((reader.payload, reader.markers))
}

struct TypedPayloadReader<'input, 'cipher> {
    input: &'input [u8],
    position: usize,
    end: usize,
    cipher: &'cipher mut Cipher,
    payload: Vec<u8>,
    markers: Vec<RawMarker>,
    block_id: i32,
    version: i32,
}

impl<'input, 'cipher> TypedPayloadReader<'input, 'cipher> {
    fn new(
        input: &'input [u8],
        position: usize,
        length: usize,
        cipher: &'cipher mut Cipher,
        block_id: i32,
    ) -> Result<Self, GdcError> {
        let end = position
            .checked_add(length)
            .filter(|end| *end <= input.len())
            .ok_or(GdcError::InvalidBlockLength {
                block_id,
                length: length as i32,
            })?;
        Ok(Self {
            input,
            position,
            end,
            cipher,
            payload: Vec::with_capacity(length),
            markers: Vec::new(),
            block_id,
            version: 0,
        })
    }

    fn byte(&mut self) -> Result<u8, GdcError> {
        if self.position >= self.end {
            return Err(GdcError::InvalidValue(format!(
                "block {} is truncated",
                self.block_id
            )));
        }
        let value = self.cipher.read_byte(self.input, &mut self.position)?;
        self.payload.push(value);
        Ok(value)
    }

    fn bytes(&mut self, count: usize) -> Result<(), GdcError> {
        for _ in 0..count {
            self.byte()?;
        }
        Ok(())
    }

    fn i32(&mut self) -> Result<i32, GdcError> {
        if self.end.saturating_sub(self.position) < 4 {
            return Err(GdcError::InvalidValue(format!(
                "block {} is truncated",
                self.block_id
            )));
        }
        self.markers.push(RawMarker {
            offset: self.payload.len(),
            kind: RawMarkerKind::CipherI32,
        });
        let value = self.cipher.read_i32(self.input, &mut self.position)?;
        self.payload.extend_from_slice(&value.to_le_bytes());
        Ok(value)
    }

    fn count(&mut self, maximum: i32, label: &str) -> Result<usize, GdcError> {
        let count = self.i32()?;
        if !(0..=maximum).contains(&count) {
            return Err(GdcError::InvalidValue(format!(
                "invalid {label} count in block {}: {count}",
                self.block_id
            )));
        }
        Ok(count as usize)
    }

    fn ascii(&mut self) -> Result<(), GdcError> {
        let length = self.i32()?;
        if length < 0 || length as usize > self.end.saturating_sub(self.position) {
            return Err(GdcError::InvalidValue(format!(
                "invalid text length in block {} v{} at decoded offset {} (length {length}, {} bytes remain)",
                self.block_id,
                self.version,
                self.payload.len().saturating_sub(4),
                self.end.saturating_sub(self.position),
            )));
        }
        self.bytes(length as usize)
    }

    fn utf16(&mut self) -> Result<(), GdcError> {
        let length = self.i32()?;
        let byte_length = usize::try_from(length)
            .ok()
            .and_then(|length| length.checked_mul(2));
        if byte_length.is_none_or(|length| length > self.end.saturating_sub(self.position)) {
            return Err(GdcError::InvalidValue(format!(
                "invalid UTF-16 text length in block {} v{}: {length}",
                self.block_id, self.version
            )));
        }
        self.bytes(byte_length.expect("validated above"))
    }

    fn uuid(&mut self) -> Result<(), GdcError> {
        self.bytes(16)
    }

    fn uuid_slice(&mut self, label: &str) -> Result<(), GdcError> {
        let count = self.count(1_000_000, label)?;
        for _ in 0..count {
            self.uuid()?;
        }
        Ok(())
    }

    fn ascii_slice(&mut self, label: &str) -> Result<(), GdcError> {
        let count = self.count(1_000_000, label)?;
        for _ in 0..count {
            self.ascii()?;
        }
        Ok(())
    }

    fn hot_slot(&mut self) -> Result<(), GdcError> {
        match self.i32()? {
            0 => {
                self.ascii()?;
                self.byte()?;
                self.ascii()?;
                self.i32()?;
            }
            4 => {
                self.ascii()?;
                self.ascii()?;
                self.ascii()?;
                self.utf16()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn skill_set(&mut self) -> Result<(), GdcError> {
        self.ascii()?;
        self.ascii()?;
        self.byte()?;
        Ok(())
    }

    fn mob_digest(&mut self) -> Result<(), GdcError> {
        self.ascii()?;
        self.i32()?;
        self.i32()?;
        self.ascii()?;
        self.ascii()?;
        Ok(())
    }

    fn finish_exact(&self) -> Result<(), GdcError> {
        if self.position != self.end {
            return Err(GdcError::InvalidValue(format!(
                "block {} v{} left {} untyped bytes",
                self.block_id,
                self.version,
                self.end.saturating_sub(self.position)
            )));
        }
        Ok(())
    }
}

fn read_preserved_typed_payload(
    input: &[u8],
    position: &mut usize,
    length: usize,
    cipher: &mut Cipher,
    block_id: i32,
) -> Result<(Vec<u8>, Vec<RawMarker>), GdcError> {
    let mut reader = TypedPayloadReader::new(input, *position, length, cipher, block_id)?;
    let version = reader.i32()?;
    reader.version = version;

    match block_id {
        5 if version == 1 => {
            for _ in 0..3 {
                reader.uuid_slice("spawn points")?;
            }
            for _ in 0..3 {
                reader.uuid()?;
            }
        }
        6 | 7 if version == 1 => {
            for _ in 0..3 {
                reader.uuid_slice("map points")?;
            }
        }
        17 if version == 2 => {
            for _ in 0..6 {
                reader.uuid_slice("shrine points")?;
            }
        }
        10 if version == 2 => {
            for _ in 0..3 {
                reader.ascii_slice("tokens")?;
            }
        }
        12 if version == 1 => reader.ascii_slice("lore notes")?,
        15 if version == 1 => {
            let tutorial_count = reader.count(1_000_000, "tutorials")?;
            for _ in 0..tutorial_count {
                reader.i32()?;
            }
        }
        14 if matches!(version, 5..=7) => {
            reader.byte()?;
            reader.i32()?;
            reader.byte()?;
            for _ in 0..5 {
                reader.skill_set()?;
            }
            match version {
                5 => {
                    for _ in 0..46 {
                        reader.hot_slot()?;
                    }
                }
                6 => {
                    for _ in 0..47 {
                        reader.hot_slot()?;
                    }
                }
                7 => {
                    let array_count = reader.count(1_024, "hotbar arrays")?;
                    let slot_count = reader.count(1_024, "hotbar slots")?;
                    for _ in 0..array_count {
                        reader.i32()?; // array index
                        for _ in 0..slot_count {
                            reader.hot_slot()?;
                        }
                    }
                }
                _ => unreachable!(),
            }
            reader.i32()?; // camera distance as f32 bits
        }
        16 if matches!(version, 11 | 12) => {
            for _ in 0..12 {
                reader.i32()?;
            }
            for _ in 0..3 {
                reader.mob_digest()?;
            }
            for _ in 0..19 {
                reader.i32()?;
            }
            let skill_count = reader.count(65_536, "summary skills")?;
            for _ in 0..skill_count {
                reader.ascii()?;
                reader.i32()?;
            }
            reader.i32()?; // shattered souls
            reader.i32()?; // shattered essence
            reader.byte()?; // difficulty skip
            if version >= 12 {
                reader.i32()?; // ascendant champion kills
                reader.i32()?; // hidden chests opened
            }
            reader.i32()?;
            reader.i32()?;
        }
        _ => {
            return Err(GdcError::InvalidValue(format!(
                "unsupported preserved block {block_id} version {version}"
            )));
        }
    }

    reader.finish_exact()?;
    *position = reader.position;
    Ok((reader.payload, reader.markers))
}

fn read_core_payload(
    input: &[u8],
    position: &mut usize,
    length: usize,
    cipher: &mut Cipher,
) -> Result<(Vec<u8>, Vec<RawMarker>), GdcError> {
    if length != CORE_BLOCK_SIZE || input.len().saturating_sub(*position) < length {
        return Err(GdcError::InvalidCoreBlock);
    }
    let mut payload = Vec::with_capacity(length);
    let mut markers = Vec::with_capacity(CORE_BLOCK_SIZE / 4);
    for _ in 0..CORE_BLOCK_SIZE / 4 {
        markers.push(RawMarker {
            offset: payload.len(),
            kind: RawMarkerKind::CipherI32,
        });
        payload.extend_from_slice(&cipher.read_i32(input, position)?.to_le_bytes());
    }
    payload.extend_from_slice(&cipher.read_bytes(input, position, length - CORE_BLOCK_SIZE)?);
    Ok((payload, markers))
}

pub fn encode(document: &GdcDocument) -> Result<Vec<u8>, GdcError> {
    validate_document(document)?;
    let mut output = Vec::with_capacity(64 * 1024);
    output.extend_from_slice(&(document.seed ^ SEED_MASK).to_le_bytes());
    let mut cipher = Cipher::new(document.seed);

    cipher.write_i32(GDC_MAGIC, &mut output);
    cipher.write_i32(document.file_version, &mut output);
    cipher.write_utf16(&document.header.character_name, &mut output)?;
    cipher.write_bool(document.header.male, &mut output);
    cipher.write_ascii(&document.header.class_name, &mut output)?;
    cipher.write_i32(document.header.character_level, &mut output);
    cipher.write_bool(document.header.hardcore, &mut output);
    let expansion_raw = if document.header.expansion_character {
        document.expansion_raw.max(1)
    } else {
        0
    };
    cipher.write_byte(expansion_raw, &mut output);
    output.extend_from_slice(&cipher.state.to_le_bytes());

    cipher.write_i32(document.data_version, &mut output);
    cipher.write_bytes(&document.mystery, &mut output);

    for block in &document.blocks {
        cipher.write_i32(block.id, &mut output);
        let length = u32::try_from(block.payload.len())
            .map_err(|_| GdcError::InvalidValue("block is too large".into()))?;
        output.extend_from_slice(&(length ^ cipher.state).to_le_bytes());
        write_block_payload(block, &mut cipher, &mut output)?;
        output.extend_from_slice(&cipher.state.to_le_bytes());
    }
    Ok(output)
}

/// Encodes a document only when every encrypted block has a verified typed
/// layout. Use this entry point for edited saves; `encode` remains available
/// for exact round-trip preservation and codec diagnostics.
pub fn encode_mutation(document: &GdcDocument) -> Result<Vec<u8>, GdcError> {
    let block_ids = document.mutation_blockers();
    if !block_ids.is_empty() {
        return Err(GdcError::UnsafeMutation { block_ids });
    }
    encode(document)
}

fn write_block_payload(
    block: &GdcBlock,
    cipher: &mut Cipher,
    output: &mut Vec<u8>,
) -> Result<(), GdcError> {
    let mut cursor = 0;
    for marker in &block.markers {
        if marker.offset < cursor || marker.offset + 4 > block.payload.len() {
            return Err(GdcError::InvalidValue(format!(
                "invalid internal marker in block {}",
                block.id
            )));
        }
        cipher.write_bytes(&block.payload[cursor..marker.offset], output);
        match marker.kind {
            RawMarkerKind::CipherI32 => {
                let value = i32::from_le_bytes(
                    block.payload[marker.offset..marker.offset + 4]
                        .try_into()
                        .expect("slice length checked"),
                );
                cipher.write_i32(value, output);
            }
            RawMarkerKind::Length => {
                let logical = u32::from_le_bytes(
                    block.payload[marker.offset..marker.offset + 4]
                        .try_into()
                        .expect("slice length checked"),
                );
                output.extend_from_slice(&(logical ^ cipher.state).to_le_bytes());
            }
            RawMarkerKind::Checksum => output.extend_from_slice(&cipher.state.to_le_bytes()),
        }
        cursor = marker.offset + 4;
    }
    cipher.write_bytes(&block.payload[cursor..], output);
    Ok(())
}

fn read_nested_payload(
    input: &[u8],
    position: &mut usize,
    length: usize,
    cipher: &mut Cipher,
    block_id: i32,
) -> Result<(Vec<u8>, Vec<RawMarker>), GdcError> {
    let end = position
        .checked_add(length)
        .filter(|end| *end <= input.len())
        .ok_or(GdcError::InvalidBlockLength {
            block_id,
            length: length as i32,
        })?;
    let mut reader = NestedPayloadReader {
        input,
        position: *position,
        end,
        cipher,
        payload: Vec::with_capacity(length),
        markers: Vec::new(),
        outer_block: block_id,
        context: String::new(),
        block_version: 0,
    };
    match block_id {
        3 => reader.block_three()?,
        4 => reader.block_four()?,
        _ => unreachable!("only blocks with known nested framing are routed here"),
    }
    if reader.position != end {
        return Err(GdcError::InvalidValue(format!(
            "block {block_id} ended at position {}, but it should end at {end}",
            reader.position
        )));
    }
    *position = reader.position;
    Ok((reader.payload, reader.markers))
}

struct NestedPayloadReader<'input, 'cipher> {
    input: &'input [u8],
    position: usize,
    end: usize,
    cipher: &'cipher mut Cipher,
    payload: Vec<u8>,
    markers: Vec<RawMarker>,
    outer_block: i32,
    context: String,
    block_version: i32,
}

impl NestedPayloadReader<'_, '_> {
    fn block_three(&mut self) -> Result<(), GdcError> {
        self.block_version = self.i32()?;
        let has_data = self.bool()?;
        if !has_data {
            return Ok(());
        }
        let sack_count = self.count("inventory bags", 128)?;
        self.i32()?; // focused sack
        self.i32()?; // selected sack
        for sack_index in 0..sack_count {
            self.inventory_sack(sack_index)?;
        }
        self.bool()?; // use alternate weapon set
        for item_index in 0..12 {
            self.context = format!("equipment {item_index}");
            self.item(ItemTail::Equipment)?;
        }
        self.bool()?; // first alternate set enabled
        for item_index in 0..2 {
            self.context = format!("arma alternativa 1/{item_index}");
            self.item(ItemTail::Equipment)?;
        }
        self.bool()?; // second alternate set enabled
        for item_index in 0..2 {
            self.context = format!("arma alternativa 2/{item_index}");
            self.item(ItemTail::Equipment)?;
        }
        Ok(())
    }

    fn block_four(&mut self) -> Result<(), GdcError> {
        self.block_version = self.i32()?;
        let stash_count = self.count("stash tabs", 128)?;
        for stash_index in 0..stash_count {
            self.stash(stash_index)?;
        }
        Ok(())
    }

    fn inventory_sack(&mut self, sack_index: usize) -> Result<(), GdcError> {
        self.nested_block(|reader| {
            reader.bool()?; // unused
            let count = reader.count("bag items", 16_384)?;
            for item_index in 0..count {
                reader.context = format!("bag {sack_index}, item {item_index}");
                reader.item(ItemTail::Coordinates)?;
            }
            Ok(())
        })
    }

    fn stash(&mut self, stash_index: usize) -> Result<(), GdcError> {
        self.nested_block(|reader| {
            reader.i32()?; // width
            reader.i32()?; // height
            let count = reader.count("stash items", 65_536)?;
            for item_index in 0..count {
                reader.context = format!("stash {stash_index}, item {item_index}");
                reader.item(ItemTail::Coordinates)?;
            }
            if reader.block_version >= 11 {
                reader.i32()?; // border style
                reader.i32()?; // border color
                reader.i32()?; // symbol
                reader.i32()?; // symbol color
                reader.utf16()?; // custom tab label
            }
            Ok(())
        })
    }

    fn nested_block(
        &mut self,
        body: impl FnOnce(&mut Self) -> Result<(), GdcError>,
    ) -> Result<(), GdcError> {
        let nested_id = self.i32()?;
        if nested_id != 0 {
            return Err(GdcError::InvalidValue(format!(
                "unexpected nested block {nested_id} inside block {}",
                self.outer_block
            )));
        }
        let length = self.raw_length()? as usize;
        let nested_end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.end)
            .ok_or(GdcError::InvalidBlockLength {
                block_id: nested_id,
                length: length as i32,
            })?;
        body(self)?;
        if self.position != nested_end {
            return Err(GdcError::InvalidValue(format!(
                "nested block consumed {} bytes; expected {length}",
                self.position
                    .saturating_sub(nested_end.saturating_sub(length))
            )));
        }
        self.raw_checksum("nested block")
    }

    fn item(&mut self, tail: ItemTail) -> Result<(), GdcError> {
        let item_context = self.context.clone();
        self.context = format!("{item_context}, base");
        self.ascii()?;
        for index in 1..3 {
            self.context = format!("{item_context}, string {index}");
            self.ascii()?;
        }
        self.context = format!("{item_context}, string 3");
        self.ascii()?;
        self.context = format!("{item_context}, string 4");
        self.ascii()?;
        self.i32()?; // item seed
        self.context = format!("{item_context}, relic");
        self.ascii()?;
        self.context = format!("{item_context}, relic bonus");
        self.ascii()?;
        self.i32()?; // relic seed
        self.context = format!("{item_context}, augment");
        self.ascii()?;
        self.i32()?; // unknown
        self.i32()?; // augment seed
        if self.block_version >= 11 {
            self.context = format!("{item_context}, ascendant affix");
            self.ascii()?;
            self.context = format!("{item_context}, two-handed ascendant affix");
            self.ascii()?;
        }
        self.i32()?; // relic completion level
        self.i32()?; // stack count
        if self.block_version >= 11 {
            self.i32()?; // seed reroll count
            self.i32()?; // ascendant reroll count
        }
        match tail {
            ItemTail::Coordinates => {
                self.i32()?; // x coordinate
                self.i32()?; // y coordinate
            }
            ItemTail::Equipment => {
                self.bool()?;
            }
        }
        self.context = item_context;
        Ok(())
    }

    fn count(&mut self, label: &str, maximum: i32) -> Result<usize, GdcError> {
        let value = self.i32()?;
        if !(0..=maximum).contains(&value) {
            return Err(GdcError::InvalidValue(format!(
                "invalid {label} count: {value}"
            )));
        }
        Ok(value as usize)
    }

    fn ascii(&mut self) -> Result<usize, GdcError> {
        let length = self.i32()?;
        if length < 0 || length as usize > self.end.saturating_sub(self.position) {
            return Err(GdcError::InvalidValue(format!(
                "invalid text length ({length}) in block {} v{}, position {}, context {}",
                self.outer_block, self.block_version, self.position, self.context
            )));
        }
        for _ in 0..length {
            self.byte()?;
        }
        Ok(length as usize)
    }

    fn utf16(&mut self) -> Result<usize, GdcError> {
        let length = self.i32()?;
        let byte_length = usize::try_from(length)
            .ok()
            .and_then(|length| length.checked_mul(2));
        if byte_length.is_none_or(|length| length > self.end.saturating_sub(self.position)) {
            return Err(GdcError::InvalidValue(format!(
                "invalid UTF-16 length ({length}) in block {} v{}, position {}, context {}",
                self.outer_block, self.block_version, self.position, self.context
            )));
        }
        let byte_length = byte_length.expect("validated above");
        for _ in 0..byte_length {
            self.byte()?;
        }
        Ok(byte_length / 2)
    }

    fn bool(&mut self) -> Result<bool, GdcError> {
        Ok(self.byte()? == 1)
    }

    fn byte(&mut self) -> Result<u8, GdcError> {
        if self.position >= self.end {
            return Err(GdcError::Truncated {
                position: self.position,
                needed: 1,
            });
        }
        let value = self.cipher.read_byte(self.input, &mut self.position)?;
        self.payload.push(value);
        Ok(value)
    }

    fn i32(&mut self) -> Result<i32, GdcError> {
        if self.end.saturating_sub(self.position) < 4 {
            return Err(GdcError::Truncated {
                position: self.position,
                needed: 4,
            });
        }
        let value = self.cipher.read_i32(self.input, &mut self.position)?;
        self.markers.push(RawMarker {
            offset: self.payload.len(),
            kind: RawMarkerKind::CipherI32,
        });
        self.payload.extend_from_slice(&value.to_le_bytes());
        Ok(value)
    }

    fn raw_length(&mut self) -> Result<u32, GdcError> {
        if self.end.saturating_sub(self.position) < 4 {
            return Err(GdcError::Truncated {
                position: self.position,
                needed: 4,
            });
        }
        let encrypted = raw_u32(self.input, &mut self.position)?;
        let logical = encrypted ^ self.cipher.state;
        self.markers.push(RawMarker {
            offset: self.payload.len(),
            kind: RawMarkerKind::Length,
        });
        self.payload.extend_from_slice(&logical.to_le_bytes());
        Ok(logical)
    }

    fn raw_checksum(&mut self, section: &str) -> Result<(), GdcError> {
        let actual = raw_u32(self.input, &mut self.position)?;
        if actual != self.cipher.state {
            return Err(GdcError::InvalidChecksum {
                section: section.into(),
                expected: self.cipher.state,
                actual,
            });
        }
        self.markers.push(RawMarker {
            offset: self.payload.len(),
            kind: RawMarkerKind::Checksum,
        });
        self.payload.extend_from_slice(&actual.to_le_bytes());
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum ItemTail {
    Coordinates,
    Equipment,
}

fn validate_document(document: &GdcDocument) -> Result<(), GdcError> {
    if document.header.character_name.trim().is_empty() {
        return Err(GdcError::InvalidValue(
            "the character name cannot be empty".into(),
        ));
    }
    if document.header.character_name.encode_utf16().count() > 32 {
        return Err(GdcError::InvalidValue(
            "the character name can contain at most 32 characters".into(),
        ));
    }
    if !(1..=100).contains(&document.header.character_level) {
        return Err(GdcError::InvalidValue(
            "the level must be between 1 and 100".into(),
        ));
    }
    if !matches!(document.data_version, 6..=8) {
        return Err(GdcError::UnsupportedDataVersion(document.data_version));
    }
    Ok(())
}

fn verify_checksum(
    bytes: &[u8],
    position: &mut usize,
    expected: u32,
    section: &str,
) -> Result<(), GdcError> {
    let actual = raw_u32(bytes, position)?;
    if actual != expected {
        return Err(GdcError::InvalidChecksum {
            section: section.to_owned(),
            expected,
            actual,
        });
    }
    Ok(())
}

fn raw_u32(bytes: &[u8], position: &mut usize) -> Result<u32, GdcError> {
    let end = position.saturating_add(4);
    let chunk = bytes.get(*position..end).ok_or(GdcError::Truncated {
        position: *position,
        needed: 4,
    })?;
    *position = end;
    Ok(u32::from_le_bytes(
        chunk.try_into().expect("slice length checked"),
    ))
}

#[derive(Clone)]
struct Cipher {
    state: u32,
    table: [u32; 256],
}

impl Cipher {
    fn new(seed: u32) -> Self {
        let mut value = seed;
        let mut table = [0_u32; 256];
        for entry in &mut table {
            value = value.rotate_right(1).wrapping_mul(39_916_801);
            *entry = value;
        }
        Self { state: seed, table }
    }

    fn advance(&mut self, encrypted: u8) {
        self.state ^= self.table[encrypted as usize];
    }

    fn read_byte(&mut self, input: &[u8], position: &mut usize) -> Result<u8, GdcError> {
        let encrypted = *input.get(*position).ok_or(GdcError::Truncated {
            position: *position,
            needed: 1,
        })?;
        *position += 1;
        let plain = encrypted ^ self.state as u8;
        self.advance(encrypted);
        Ok(plain)
    }

    fn read_bool(&mut self, input: &[u8], position: &mut usize) -> Result<bool, GdcError> {
        Ok(self.read_byte(input, position)? == 1)
    }

    fn read_i32(&mut self, input: &[u8], position: &mut usize) -> Result<i32, GdcError> {
        let mut encrypted = [0_u8; 4];
        for byte in &mut encrypted {
            *byte = *input.get(*position).ok_or(GdcError::Truncated {
                position: *position,
                needed: 4,
            })?;
            *position += 1;
        }
        let plain = u32::from_le_bytes(encrypted) ^ self.state;
        for byte in encrypted {
            self.advance(byte);
        }
        Ok(plain as i32)
    }

    fn read_bytes(
        &mut self,
        input: &[u8],
        position: &mut usize,
        length: usize,
    ) -> Result<Vec<u8>, GdcError> {
        if input.len().saturating_sub(*position) < length {
            return Err(GdcError::Truncated {
                position: *position,
                needed: length,
            });
        }
        let mut result = Vec::with_capacity(length);
        for _ in 0..length {
            result.push(self.read_byte(input, position)?);
        }
        Ok(result)
    }

    fn read_ascii(&mut self, input: &[u8], position: &mut usize) -> Result<String, GdcError> {
        let length = self.read_i32(input, position)?;
        if length < 0 {
            return Err(GdcError::InvalidText { encoding: "ASCII" });
        }
        let bytes = self.read_bytes(input, position, length as usize)?;
        if !bytes.is_ascii() {
            return Err(GdcError::InvalidText { encoding: "ASCII" });
        }
        String::from_utf8(bytes).map_err(|_| GdcError::InvalidText { encoding: "ASCII" })
    }

    fn read_utf16(&mut self, input: &[u8], position: &mut usize) -> Result<String, GdcError> {
        let length = self.read_i32(input, position)?;
        if length < 0 {
            return Err(GdcError::InvalidText {
                encoding: "UTF-16LE",
            });
        }
        let bytes = self.read_bytes(input, position, length as usize * 2)?;
        let words = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&words).map_err(|_| GdcError::InvalidText {
            encoding: "UTF-16LE",
        })
    }

    fn write_byte(&mut self, value: u8, output: &mut Vec<u8>) {
        let encrypted = value ^ self.state as u8;
        output.push(encrypted);
        self.advance(encrypted);
    }

    fn write_bool(&mut self, value: bool, output: &mut Vec<u8>) {
        self.write_byte(u8::from(value), output);
    }

    fn write_i32(&mut self, value: i32, output: &mut Vec<u8>) {
        let encrypted = (value as u32 ^ self.state).to_le_bytes();
        output.extend_from_slice(&encrypted);
        for byte in encrypted {
            self.advance(byte);
        }
    }

    fn write_bytes(&mut self, values: &[u8], output: &mut Vec<u8>) {
        for &value in values {
            self.write_byte(value, output);
        }
    }

    fn write_ascii(&mut self, value: &str, output: &mut Vec<u8>) -> Result<(), GdcError> {
        if !value.is_ascii() {
            return Err(GdcError::InvalidText { encoding: "ASCII" });
        }
        self.write_i32(value.len() as i32, output);
        self.write_bytes(value.as_bytes(), output);
        Ok(())
    }

    fn write_utf16(&mut self, value: &str, output: &mut Vec<u8>) -> Result<(), GdcError> {
        let words = value.encode_utf16().collect::<Vec<_>>();
        self.write_i32(words.len() as i32, output);
        for word in words {
            self.write_bytes(&word.to_le_bytes(), output);
        }
        Ok(())
    }
}

struct PlainCursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> PlainCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn i32(&mut self) -> Result<i32, GdcError> {
        let end = self.position + 4;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or(GdcError::InvalidCoreBlock)?;
        self.position = end;
        Ok(i32::from_le_bytes(
            value.try_into().expect("slice length checked"),
        ))
    }

    fn f32(&mut self) -> Result<f32, GdcError> {
        Ok(f32::from_bits(self.i32()? as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core_payload() -> Vec<u8> {
        let mut values = Vec::new();
        values.extend_from_slice(&1_i32.to_le_bytes());
        for value in [42_i32, 123_456, 9, 12, 4, 17] {
            values.extend_from_slice(&value.to_le_bytes());
        }
        for value in [400.5_f32, 318.0, 255.25, 2_400.0, 1_100.0] {
            values.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        values
    }

    fn character_payload() -> Vec<u8> {
        let mut values = Vec::new();
        values.extend_from_slice(&1_i32.to_le_bytes());
        values.extend_from_slice(&[0, 1, 2, 1]);
        values.extend_from_slice(&75_000_i32.to_le_bytes());
        values.push(0);
        values.extend_from_slice(&14_i32.to_le_bytes());
        values.extend_from_slice(&[1, 1, 0, 1]);
        values.extend_from_slice(&0_i32.to_le_bytes());
        values
    }

    fn fixture() -> GdcDocument {
        GdcDocument {
            seed: 0x1234_abcd,
            file_version: 2,
            header: CharacterHeader {
                character_name: "Korvaak".into(),
                male: true,
                class_name: "tagSkillClassName0101".into(),
                character_level: 42,
                hardcore: false,
                expansion_character: true,
            },
            expansion_raw: 1,
            data_version: 8,
            mystery: [0x5a; 16],
            blocks: vec![
                GdcBlock {
                    id: 1,
                    payload: character_payload(),
                    markers: [0, 8, 13, 21]
                        .into_iter()
                        .map(|offset| RawMarker {
                            offset,
                            kind: RawMarkerKind::CipherI32,
                        })
                        .collect(),
                },
                GdcBlock {
                    id: 2,
                    payload: core_payload(),
                    markers: (0..CORE_BLOCK_SIZE)
                        .step_by(4)
                        .map(|offset| RawMarker {
                            offset,
                            kind: RawMarkerKind::CipherI32,
                        })
                        .collect(),
                },
                GdcBlock {
                    id: 99,
                    payload: vec![1, 2, 3, 4, 5],
                    markers: vec![],
                },
            ],
        }
    }

    #[test]
    fn exact_round_trip_preserves_unknown_blocks() {
        let encoded = encode(&fixture()).unwrap();
        let parsed = parse(&encoded).unwrap();
        assert_eq!(parsed, fixture());
        assert_eq!(encode(&parsed).unwrap(), encoded);
    }

    #[test]
    fn refuses_to_reencode_a_mutated_document_with_opaque_blocks() {
        let mut document = fixture();
        let mut stats = document.core_stats().unwrap().unwrap();
        stats.experience += 1;
        document.set_core_stats(&stats).unwrap();

        assert_eq!(document.mutation_blockers(), vec![99]);
        assert!(matches!(
            encode_mutation(&document),
            Err(GdcError::UnsafeMutation { block_ids }) if block_ids == vec![99]
        ));
    }

    #[test]
    fn reencodes_a_mutation_when_preserved_block_types_are_known() {
        let mut document = fixture();
        let mut spawn_payload = Vec::new();
        for value in [1_i32, 0, 0, 0] {
            spawn_payload.extend_from_slice(&value.to_le_bytes());
        }
        spawn_payload.extend_from_slice(&[0_u8; 48]);
        document.blocks[2] = GdcBlock {
            id: 5,
            payload: spawn_payload,
            markers: [0, 4, 8, 12]
                .into_iter()
                .map(|offset| RawMarker {
                    offset,
                    kind: RawMarkerKind::CipherI32,
                })
                .collect(),
        };
        let encoded = encode(&document).unwrap();
        let mut reparsed = parse(&encoded).unwrap();
        let mut stats = reparsed.core_stats().unwrap().unwrap();
        stats.experience += 1;
        reparsed.set_core_stats(&stats).unwrap();

        assert!(reparsed.mutation_supported());
        let mutated = encode_mutation(&reparsed).unwrap();
        let verified = parse(&mutated).unwrap();
        assert_eq!(
            verified.core_stats().unwrap().unwrap().experience,
            stats.experience
        );
        assert!(verified.mutation_supported());
    }

    #[test]
    fn reads_and_updates_core_stats_without_touching_trailing_data() {
        let mut fixture = fixture();
        fixture.blocks[1].payload.extend_from_slice(&[9, 8, 7]);
        let mut stats = fixture.core_stats().unwrap().unwrap();
        stats.physique = 999.0;
        fixture.set_core_stats(&stats).unwrap();
        assert_eq!(&fixture.blocks[1].payload[CORE_BLOCK_SIZE..], &[9, 8, 7]);
        assert_eq!(fixture.core_stats().unwrap().unwrap().physique, 999.0);
    }

    #[test]
    fn reads_and_updates_iron() {
        let mut fixture = fixture();
        assert_eq!(fixture.iron().unwrap(), Some(75_000));
        fixture.set_iron(999_999).unwrap();
        let reparsed = parse(&encode(&fixture).unwrap()).unwrap();
        assert_eq!(reparsed.iron().unwrap(), Some(999_999));
    }

    #[test]
    fn rejects_corrupted_checksum() {
        let mut encoded = encode(&fixture()).unwrap();
        let last = encoded.len() - 1;
        encoded[last] ^= 0xff;
        assert!(matches!(
            parse(&encoded),
            Err(GdcError::InvalidChecksum { .. })
        ));
    }
}
