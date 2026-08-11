use super::{GdcDocument, GdcError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterSkill {
    pub record: String,
    pub level: i32,
    pub enabled: bool,
    pub devotion_level: i32,
    pub devotion_experience: i32,
    pub sublevel: i32,
    pub skill_active: bool,
    pub skill_transition: bool,
    pub autocast_skill_record: String,
    pub autocast_controller_record: String,
    pub devotion: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactionValue {
    pub index: usize,
    pub changed: bool,
    pub unlocked: bool,
    pub value: f32,
    pub positive_boost: f32,
    pub negative_boost: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactionPatch {
    pub index: usize,
    pub value: f32,
}

impl GdcDocument {
    pub fn skills(&self) -> Result<Option<Vec<CharacterSkill>>, GdcError> {
        let Some(block) = self.blocks.iter().find(|block| block.id == 8) else {
            return Ok(Some(Vec::new()));
        };
        if block.markers.is_empty() {
            // Newer Fangs saves changed the typed layout, but record strings
            // remain byte-ciphered. Expose only verified saved skill records
            // and deliberately leave their numeric levels unknown.
            return Ok(Some(scan_skill_records(&block.payload)));
        }
        let mut cursor = Cursor::new(&block.payload);
        let version = cursor.i32()?;
        if !matches!(version, 5 | 6 | 8) {
            return Ok(None);
        }
        let count = cursor.count(65_536)?;
        let mut skills = Vec::with_capacity(count);
        for _ in 0..count {
            let record = cursor.ascii()?;
            let level = cursor.i32()?;
            let enabled = cursor.bool()?;
            let locked = if version >= 8 { cursor.bool()? } else { false };
            let devotion_level = cursor.i32()?;
            let devotion_experience = cursor.i32()?;
            let sublevel = cursor.i32()?;
            let skill_active = cursor.bool()?;
            let skill_transition = cursor.bool()?;
            let autocast_skill_record = cursor.ascii()?;
            let autocast_controller_record = cursor.ascii()?;
            let normalized = record.to_ascii_lowercase();
            skills.push(CharacterSkill {
                record,
                level,
                enabled,
                devotion_level,
                devotion_experience,
                sublevel,
                skill_active,
                skill_transition: locked || skill_transition,
                autocast_skill_record,
                autocast_controller_record,
                devotion: normalized.contains("/devotion/") || normalized.contains("devotion"),
            });
        }
        Ok(Some(skills))
    }

    pub fn factions(&self) -> Result<Vec<FactionValue>, GdcError> {
        let Some(block) = self.blocks.iter().find(|block| block.id == 13) else {
            return Ok(Vec::new());
        };
        let mut cursor = Cursor::new(&block.payload);
        cursor.i32()?; // version
        cursor.i32()?; // player's faction
        let count = cursor.count(16_384)?;
        let mut factions = Vec::with_capacity(count);
        for index in 0..count {
            factions.push(FactionValue {
                index,
                changed: cursor.bool()?,
                unlocked: cursor.bool()?,
                value: cursor.f32()?,
                positive_boost: cursor.f32()?,
                negative_boost: cursor.f32()?,
            });
        }
        Ok(factions)
    }

    pub fn apply_faction_patch(&mut self, patch: &FactionPatch) -> Result<(), GdcError> {
        if !patch.value.is_finite() || !(-20_000.0..=25_000.0).contains(&patch.value) {
            return Err(GdcError::InvalidValue(
                "faction reputation must be between -20,000 and 25,000".into(),
            ));
        }
        let block = self
            .blocks
            .iter_mut()
            .find(|block| block.id == 13)
            .ok_or_else(|| GdcError::InvalidValue("faction block is missing".into()))?;
        let mut cursor = Cursor::new(&block.payload);
        cursor.i32()?;
        cursor.i32()?;
        let count = cursor.count(16_384)?;
        if patch.index >= count {
            return Err(GdcError::InvalidValue(format!(
                "unknown faction index: {}",
                patch.index
            )));
        }
        for index in 0..count {
            cursor.bool()?;
            cursor.bool()?;
            let value_offset = cursor.position;
            cursor.f32()?;
            cursor.f32()?;
            cursor.f32()?;
            if index == patch.index {
                block.payload[value_offset..value_offset + 4]
                    .copy_from_slice(&patch.value.to_bits().to_le_bytes());
                return Ok(());
            }
        }
        unreachable!("faction index was range checked")
    }
}

fn scan_skill_records(payload: &[u8]) -> Vec<CharacterSkill> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    let needle = b"records/skills/";
    let mut index = 0;
    while index + needle.len() <= payload.len() {
        if &payload[index..index + needle.len()] != needle {
            index += 1;
            continue;
        }
        let Some(relative_end) = payload[index..]
            .windows(4)
            .position(|window| window.eq_ignore_ascii_case(b".dbr"))
        else {
            break;
        };
        let end = index + relative_end + 4;
        let bytes = &payload[index..end];
        if bytes.iter().all(u8::is_ascii_graphic) {
            let record = String::from_utf8_lossy(bytes).into_owned();
            let normalized = record.to_ascii_lowercase();
            if seen.insert(normalized.clone()) {
                result.push(CharacterSkill {
                    record,
                    level: -1,
                    enabled: true,
                    devotion_level: -1,
                    devotion_experience: -1,
                    sublevel: -1,
                    skill_active: true,
                    skill_transition: false,
                    autocast_skill_record: String::new(),
                    autocast_controller_record: String::new(),
                    devotion: normalized.contains("/devotion/") || normalized.contains("devotion"),
                });
            }
        }
        index = end;
    }
    result
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn byte(&mut self) -> Result<u8, GdcError> {
        let value = *self.bytes.get(self.position).ok_or(GdcError::Truncated {
            position: self.position,
            needed: 1,
        })?;
        self.position += 1;
        Ok(value)
    }

    fn bool(&mut self) -> Result<bool, GdcError> {
        Ok(self.byte()? == 1)
    }

    fn i32(&mut self) -> Result<i32, GdcError> {
        let end = self.position + 4;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or(GdcError::Truncated {
                position: self.position,
                needed: 4,
            })?;
        self.position = end;
        Ok(i32::from_le_bytes(bytes.try_into().expect("slice checked")))
    }

    fn f32(&mut self) -> Result<f32, GdcError> {
        Ok(f32::from_bits(self.i32()? as u32))
    }

    fn count(&mut self, maximum: i32) -> Result<usize, GdcError> {
        let count = self.i32()?;
        if !(0..=maximum).contains(&count) {
            return Err(GdcError::InvalidValue(format!("invalid count: {count}")));
        }
        Ok(count as usize)
    }

    fn ascii(&mut self) -> Result<String, GdcError> {
        let length = self.i32()?;
        if length < 0 || length as usize > self.bytes.len().saturating_sub(self.position) {
            return Err(GdcError::InvalidText { encoding: "ASCII" });
        }
        let end = self.position + length as usize;
        let bytes = &self.bytes[self.position..end];
        if !bytes.is_ascii() {
            return Err(GdcError::InvalidText { encoding: "ASCII" });
        }
        self.position = end;
        Ok(String::from_utf8(bytes.to_vec()).expect("ASCII is UTF-8"))
    }
}
