use super::{GdcBlock, GdcDocument, GdcError, RawMarker, RawMarkerKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemContainer {
    Inventory,
    Equipment,
    WeaponSetOne,
    WeaponSetTwo,
    Stash,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterItem {
    pub id: String,
    pub container: ItemContainer,
    pub container_index: usize,
    pub slot_index: usize,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub base_record: String,
    pub prefix_record: String,
    pub suffix_record: String,
    pub modifier_record: String,
    pub transmute_record: String,
    pub seed: i32,
    pub component_record: String,
    pub component_bonus_record: String,
    pub component_seed: i32,
    pub augment_record: String,
    pub ascendant_affix_record: String,
    pub ascendant_affix_two_handed_record: String,
    pub augment_seed: i32,
    pub component_combines: i32,
    pub stack_count: i32,
    pub ascendant_rerolls: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPatch {
    pub id: String,
    pub base_record: String,
    pub prefix_record: String,
    pub suffix_record: String,
    pub modifier_record: String,
    pub transmute_record: String,
    pub component_record: String,
    pub component_bonus_record: String,
    pub augment_record: String,
    pub ascendant_affix_record: String,
    pub ascendant_affix_two_handed_record: String,
    pub stack_count: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewInventoryItem {
    pub bag_index: usize,
    pub x: i32,
    pub y: i32,
    pub base_record: String,
    pub stack_count: i32,
}

impl From<&CharacterItem> for ItemPatch {
    fn from(item: &CharacterItem) -> Self {
        Self {
            id: item.id.clone(),
            base_record: item.base_record.clone(),
            prefix_record: item.prefix_record.clone(),
            suffix_record: item.suffix_record.clone(),
            modifier_record: item.modifier_record.clone(),
            transmute_record: item.transmute_record.clone(),
            component_record: item.component_record.clone(),
            component_bonus_record: item.component_bonus_record.clone(),
            augment_record: item.augment_record.clone(),
            ascendant_affix_record: item.ascendant_affix_record.clone(),
            ascendant_affix_two_handed_record: item.ascendant_affix_two_handed_record.clone(),
            stack_count: item.stack_count,
        }
    }
}

#[derive(Debug, Clone)]
struct TextField {
    length_offset: usize,
    data_end: usize,
}

#[derive(Debug, Clone)]
struct ParsedItem {
    item: CharacterItem,
    block_index: usize,
    base: TextField,
    prefix: TextField,
    suffix: TextField,
    modifier: TextField,
    transmute: TextField,
    component: TextField,
    component_bonus: TextField,
    augment: TextField,
    ascendant: Option<TextField>,
    ascendant_two_handed: Option<TextField>,
    stack_count_offset: usize,
}

impl GdcDocument {
    pub fn items(&self) -> Result<Vec<CharacterItem>, GdcError> {
        Ok(parse_document_items(self)?
            .into_iter()
            .map(|parsed| parsed.item)
            .collect())
    }

    pub fn apply_item_patch(&mut self, patch: &ItemPatch) -> Result<(), GdcError> {
        validate_patch(patch)?;
        let parsed = parse_document_items(self)?
            .into_iter()
            .find(|candidate| candidate.item.id == patch.id)
            .ok_or_else(|| GdcError::InvalidValue(format!("unknown item: {}", patch.id)))?;
        let block = self
            .blocks
            .get_mut(parsed.block_index)
            .ok_or_else(|| GdcError::InvalidValue("item block not found".into()))?;

        let mut replacements = vec![
            (parsed.base, patch.base_record.as_str()),
            (parsed.prefix, patch.prefix_record.as_str()),
            (parsed.suffix, patch.suffix_record.as_str()),
            (parsed.modifier, patch.modifier_record.as_str()),
            (parsed.transmute, patch.transmute_record.as_str()),
            (parsed.component, patch.component_record.as_str()),
            (
                parsed.component_bonus,
                patch.component_bonus_record.as_str(),
            ),
            (parsed.augment, patch.augment_record.as_str()),
        ];
        if let Some(field) = parsed.ascendant {
            replacements.push((field, patch.ascendant_affix_record.as_str()));
        } else if !patch.ascendant_affix_record.is_empty() {
            return Err(GdcError::InvalidValue(
                "this format does not contain an Ascendant affix".into(),
            ));
        }
        if let Some(field) = parsed.ascendant_two_handed {
            replacements.push((field, patch.ascendant_affix_two_handed_record.as_str()));
        } else if !patch.ascendant_affix_two_handed_record.is_empty() {
            return Err(GdcError::InvalidValue(
                "this format does not contain a two-handed Ascendant affix".into(),
            ));
        }

        replacements.sort_by_key(|(field, _)| std::cmp::Reverse(field.length_offset));
        let mut stack_count_offset = parsed.stack_count_offset;
        for (field, value) in replacements {
            let delta = replace_text(block, &field, value)?;
            if field.length_offset < stack_count_offset {
                stack_count_offset = stack_count_offset
                    .checked_add_signed(delta)
                    .ok_or_else(|| GdcError::InvalidValue("invalid item offset".into()))?;
            }
        }
        block.payload[stack_count_offset..stack_count_offset + 4]
            .copy_from_slice(&patch.stack_count.to_le_bytes());
        Ok(())
    }

    pub fn inventory_bag_count(&self) -> Result<usize, GdcError> {
        let Some(block) = self.blocks.iter().find(|block| block.id == 3) else {
            return Ok(0);
        };
        Ok(inventory_bag_targets(block)?.len())
    }

    pub fn add_inventory_item(&mut self, item: &NewInventoryItem) -> Result<(), GdcError> {
        validate_new_item(item)?;
        let block_index = self
            .blocks
            .iter()
            .position(|block| block.id == 3)
            .ok_or_else(|| GdcError::InvalidValue("inventory block is missing".into()))?;
        let targets = inventory_bag_targets(&self.blocks[block_index])?;
        let target = targets.get(item.bag_index).ok_or_else(|| {
            GdcError::InvalidValue(format!("unknown inventory bag: {}", item.bag_index + 1))
        })?;
        let (inserted, inserted_markers) = encode_new_item(target.version, item)?;
        let delta = inserted.len();
        let block = &mut self.blocks[block_index];

        block
            .payload
            .splice(target.insertion_offset..target.insertion_offset, inserted);
        let updated_count = target
            .item_count
            .checked_add(1)
            .ok_or_else(|| GdcError::InvalidValue("too many inventory items".into()))?;
        block.payload[target.count_offset..target.count_offset + 4]
            .copy_from_slice(&updated_count.to_le_bytes());
        let old_length = u32::from_le_bytes(
            block.payload[target.length_offset..target.length_offset + 4]
                .try_into()
                .expect("nested length range checked"),
        );
        let new_length =
            old_length
                .checked_add(u32::try_from(delta).map_err(|_| {
                    GdcError::InvalidValue("new inventory item is too large".into())
                })?)
                .ok_or_else(|| GdcError::InvalidValue("inventory bag is too large".into()))?;
        block.payload[target.length_offset..target.length_offset + 4]
            .copy_from_slice(&new_length.to_le_bytes());

        for marker in &mut block.markers {
            if marker.offset >= target.insertion_offset {
                marker.offset += delta;
            }
        }
        block
            .markers
            .extend(inserted_markers.into_iter().map(|marker| RawMarker {
                offset: target.insertion_offset + marker.offset,
                kind: marker.kind,
            }));
        block.markers.sort_by_key(|marker| marker.offset);

        // Re-read the modified logical block before it reaches the encoder.
        parse_inventory_block(block, block_index, &mut Vec::new())?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct InventoryBagTarget {
    version: i32,
    count_offset: usize,
    length_offset: usize,
    insertion_offset: usize,
    item_count: i32,
}

fn inventory_bag_targets(block: &GdcBlock) -> Result<Vec<InventoryBagTarget>, GdcError> {
    let mut cursor = LogicalCursor::new(&block.payload);
    let version = cursor.i32()?;
    if !cursor.bool()? {
        return Ok(Vec::new());
    }
    let sack_count = cursor.count(128)?;
    cursor.i32()?;
    cursor.i32()?;
    let mut targets = Vec::with_capacity(sack_count);
    for _ in 0..sack_count {
        let nested_id = cursor.i32()?;
        if nested_id != 0 {
            return Err(GdcError::InvalidValue(format!(
                "unexpected nested block: {nested_id}"
            )));
        }
        let length_offset = cursor.position;
        let length = cursor.u32()? as usize;
        let nested_end = cursor
            .position
            .checked_add(length)
            .filter(|end| *end <= cursor.end)
            .ok_or(GdcError::InvalidBlockLength {
                block_id: 0,
                length: length as i32,
            })?;
        let previous_end = cursor.end;
        cursor.end = nested_end;
        cursor.bool()?;
        let count_offset = cursor.position;
        let item_count = cursor.i32()?;
        if !(0..=16_384).contains(&item_count) {
            return Err(GdcError::InvalidValue(format!(
                "invalid count: {item_count}"
            )));
        }
        for index in 0..item_count as usize {
            cursor.item(
                version,
                0,
                ItemContainer::Inventory,
                targets.len(),
                index,
                ItemTail::Coordinates,
            )?;
        }
        let insertion_offset = cursor.position;
        if insertion_offset != nested_end {
            return Err(GdcError::InvalidValue(
                "inventory bag contains unconsumed bytes".into(),
            ));
        }
        cursor.end = previous_end;
        cursor.u32()?;
        targets.push(InventoryBagTarget {
            version,
            count_offset,
            length_offset,
            insertion_offset,
            item_count,
        });
    }
    Ok(targets)
}

fn validate_new_item(item: &NewInventoryItem) -> Result<(), GdcError> {
    if item.base_record.is_empty()
        || !item.base_record.is_ascii()
        || item.base_record.len() > 1_024
        || !item.base_record.starts_with("records/")
        || !item.base_record.ends_with(".dbr")
    {
        return Err(GdcError::InvalidValue(
            "the new item must use a valid records/.../*.dbr path".into(),
        ));
    }
    if !(1..=1_000_000).contains(&item.stack_count) {
        return Err(GdcError::InvalidValue(
            "item quantity must be between 1 and 1,000,000".into(),
        ));
    }
    if !(0..=255).contains(&item.x) || !(0..=255).contains(&item.y) {
        return Err(GdcError::InvalidValue(
            "inventory coordinates must be between 0 and 255".into(),
        ));
    }
    Ok(())
}

fn encode_new_item(
    version: i32,
    item: &NewInventoryItem,
) -> Result<(Vec<u8>, Vec<RawMarker>), GdcError> {
    let mut bytes = Vec::new();
    let mut markers = Vec::new();
    let push_i32 = |value: i32, bytes: &mut Vec<u8>, markers: &mut Vec<RawMarker>| {
        markers.push(RawMarker {
            offset: bytes.len(),
            kind: RawMarkerKind::CipherI32,
        });
        bytes.extend_from_slice(&value.to_le_bytes());
    };
    let push_ascii =
        |value: &str, bytes: &mut Vec<u8>, markers: &mut Vec<RawMarker>| -> Result<(), GdcError> {
            let length = i32::try_from(value.len())
                .map_err(|_| GdcError::InvalidValue("item record is too large".into()))?;
            push_i32(length, bytes, markers);
            bytes.extend_from_slice(value.as_bytes());
            Ok(())
        };

    push_ascii(&item.base_record, &mut bytes, &mut markers)?;
    for _ in 0..4 {
        push_ascii("", &mut bytes, &mut markers)?;
    }
    push_i32(0, &mut bytes, &mut markers); // seed
    push_ascii("", &mut bytes, &mut markers)?; // component
    push_ascii("", &mut bytes, &mut markers)?; // component bonus
    push_i32(0, &mut bytes, &mut markers); // component seed
    push_ascii("", &mut bytes, &mut markers)?; // augment
    push_i32(0, &mut bytes, &mut markers); // unknown
    push_i32(0, &mut bytes, &mut markers); // augment seed
    if version >= 11 {
        push_ascii("", &mut bytes, &mut markers)?; // ascendant affix
        push_ascii("", &mut bytes, &mut markers)?; // second ascendant field
    }
    push_i32(0, &mut bytes, &mut markers); // component combines
    push_i32(item.stack_count, &mut bytes, &mut markers);
    if version >= 11 {
        push_i32(0, &mut bytes, &mut markers); // seed rerolls
        push_i32(0, &mut bytes, &mut markers); // affix rerolls
    }
    push_i32(item.x, &mut bytes, &mut markers);
    push_i32(item.y, &mut bytes, &mut markers);
    Ok((bytes, markers))
}

fn validate_patch(patch: &ItemPatch) -> Result<(), GdcError> {
    if !(0..=1_000_000).contains(&patch.stack_count) {
        return Err(GdcError::InvalidValue(
            "item quantity must be between 0 and 1,000,000".into(),
        ));
    }
    for (label, value) in [
        ("base record", &patch.base_record),
        ("prefix", &patch.prefix_record),
        ("suffix", &patch.suffix_record),
        ("modifier", &patch.modifier_record),
        ("transmute", &patch.transmute_record),
        ("component", &patch.component_record),
        ("component bonus", &patch.component_bonus_record),
        ("augment", &patch.augment_record),
        ("Ascendant affix", &patch.ascendant_affix_record),
        (
            "two-handed Ascendant affix",
            &patch.ascendant_affix_two_handed_record,
        ),
    ] {
        if !value.is_ascii() || value.len() > 1_024 {
            return Err(GdcError::InvalidValue(format!(
                "{label} must be ASCII and contain at most 1,024 characters"
            )));
        }
        if !value.is_empty() && (!value.starts_with("records/") || !value.ends_with(".dbr")) {
            return Err(GdcError::InvalidValue(format!(
                "{label} must be a valid records/.../*.dbr path"
            )));
        }
    }
    Ok(())
}

fn replace_text(block: &mut GdcBlock, field: &TextField, value: &str) -> Result<isize, GdcError> {
    let replacement_length = i32::try_from(value.len())
        .map_err(|_| GdcError::InvalidValue("item record is too large".into()))?;
    let replacement = replacement_length
        .to_le_bytes()
        .into_iter()
        .chain(value.bytes())
        .collect::<Vec<_>>();
    let old_end = field.data_end;
    let old_length = old_end.saturating_sub(field.length_offset);
    let delta = replacement.len() as isize - old_length as isize;
    let containing_lengths = block
        .markers
        .iter()
        .enumerate()
        .filter(|(_, marker)| {
            marker.kind == RawMarkerKind::Length && marker.offset + 4 <= field.length_offset
        })
        .filter_map(|(index, marker)| {
            block.markers[index + 1..]
                .iter()
                .find(|candidate| candidate.kind == RawMarkerKind::Checksum)
                .filter(|checksum| checksum.offset >= old_end)
                .map(|_| marker.offset)
        })
        .collect::<Vec<_>>();
    block
        .payload
        .splice(field.length_offset..old_end, replacement);
    for offset in containing_lengths {
        let old = u32::from_le_bytes(
            block.payload[offset..offset + 4]
                .try_into()
                .expect("length marker range checked while parsing"),
        );
        let updated = old
            .checked_add_signed(delta as i32)
            .ok_or_else(|| GdcError::InvalidValue("invalid internal length".into()))?;
        block.payload[offset..offset + 4].copy_from_slice(&updated.to_le_bytes());
    }
    for marker in &mut block.markers {
        if marker.offset > field.length_offset {
            marker.offset = marker
                .offset
                .checked_add_signed(delta)
                .ok_or_else(|| GdcError::InvalidValue("invalid item marker".into()))?;
        }
    }
    Ok(delta)
}

fn parse_document_items(document: &GdcDocument) -> Result<Vec<ParsedItem>, GdcError> {
    let mut result = Vec::new();
    for (block_index, block) in document.blocks.iter().enumerate() {
        match block.id {
            3 => parse_inventory_block(block, block_index, &mut result)?,
            4 => parse_stash_block(block, block_index, &mut result)?,
            _ => {}
        }
    }
    Ok(result)
}

fn parse_inventory_block(
    block: &GdcBlock,
    block_index: usize,
    output: &mut Vec<ParsedItem>,
) -> Result<(), GdcError> {
    let mut cursor = LogicalCursor::new(&block.payload);
    let version = cursor.i32()?;
    if !cursor.bool()? {
        return Ok(());
    }
    let sack_count = cursor.count(128)?;
    cursor.i32()?;
    cursor.i32()?;
    for sack_index in 0..sack_count {
        cursor.nested(|nested| {
            nested.bool()?;
            let item_count = nested.count(16_384)?;
            for item_index in 0..item_count {
                output.push(nested.item(
                    version,
                    block_index,
                    ItemContainer::Inventory,
                    sack_index,
                    item_index,
                    ItemTail::Coordinates,
                )?);
            }
            Ok(())
        })?;
    }
    cursor.bool()?;
    for slot in 0..12 {
        output.push(cursor.item(
            version,
            block_index,
            ItemContainer::Equipment,
            0,
            slot,
            ItemTail::Equipment,
        )?);
    }
    cursor.bool()?;
    for slot in 0..2 {
        output.push(cursor.item(
            version,
            block_index,
            ItemContainer::WeaponSetOne,
            0,
            slot,
            ItemTail::Equipment,
        )?);
    }
    cursor.bool()?;
    for slot in 0..2 {
        output.push(cursor.item(
            version,
            block_index,
            ItemContainer::WeaponSetTwo,
            0,
            slot,
            ItemTail::Equipment,
        )?);
    }
    cursor.finish()?;
    Ok(())
}

fn parse_stash_block(
    block: &GdcBlock,
    block_index: usize,
    output: &mut Vec<ParsedItem>,
) -> Result<(), GdcError> {
    let mut cursor = LogicalCursor::new(&block.payload);
    let version = cursor.i32()?;
    let stash_count = cursor.count(128)?;
    for stash_index in 0..stash_count {
        cursor.nested(|nested| {
            nested.i32()?;
            nested.i32()?;
            let item_count = nested.count(65_536)?;
            for item_index in 0..item_count {
                output.push(nested.item(
                    version,
                    block_index,
                    ItemContainer::Stash,
                    stash_index,
                    item_index,
                    ItemTail::Coordinates,
                )?);
            }
            if version >= 11 {
                for _ in 0..4 {
                    nested.i32()?;
                }
                nested.utf16()?;
            }
            Ok(())
        })?;
    }
    cursor.finish()?;
    Ok(())
}

#[derive(Clone, Copy)]
enum ItemTail {
    Coordinates,
    Equipment,
}

struct LogicalCursor<'a> {
    bytes: &'a [u8],
    position: usize,
    end: usize,
}

impl<'a> LogicalCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            end: bytes.len(),
        }
    }

    fn item(
        &mut self,
        version: i32,
        block_index: usize,
        container: ItemContainer,
        container_index: usize,
        slot_index: usize,
        tail: ItemTail,
    ) -> Result<ParsedItem, GdcError> {
        let (base, base_record) = self.ascii()?;
        let (prefix, prefix_record) = self.ascii()?;
        let (suffix, suffix_record) = self.ascii()?;
        let (modifier, modifier_record) = self.ascii()?;
        let (transmute, transmute_record) = self.ascii()?;
        let seed = self.i32()?;
        let (component, component_record) = self.ascii()?;
        let (component_bonus, component_bonus_record) = self.ascii()?;
        let component_seed = self.i32()?;
        let (augment, augment_record) = self.ascii()?;
        self.i32()?; // unknown
        let augment_seed = self.i32()?;
        let (ascendant, ascendant_affix_record) = if version >= 11 {
            let (field, value) = self.ascii()?;
            (Some(field), value)
        } else {
            (None, String::new())
        };
        let (ascendant_two_handed, ascendant_affix_two_handed_record) = if version >= 11 {
            let (field, value) = self.ascii()?;
            (Some(field), value)
        } else {
            (None, String::new())
        };
        let component_combines = self.i32()?;
        let stack_count_offset = self.position;
        let stack_count = self.i32()?;
        if version >= 11 {
            self.i32()?; // seed rerolls
        }
        let ascendant_rerolls = if version >= 11 { self.i32()? } else { 0 };
        let (x, y) = match tail {
            ItemTail::Coordinates => {
                let x = self.i32()?;
                let y = self.i32()?;
                (Some(x), Some(y))
            }
            ItemTail::Equipment => {
                self.bool()?;
                (None, None)
            }
        };
        let prefix_id = match container {
            ItemContainer::Inventory => "inventory",
            ItemContainer::Equipment => "equipment",
            ItemContainer::WeaponSetOne => "weapon-set-one",
            ItemContainer::WeaponSetTwo => "weapon-set-two",
            ItemContainer::Stash => "stash",
        };
        Ok(ParsedItem {
            item: CharacterItem {
                id: format!("{prefix_id}:{container_index}:{slot_index}"),
                container,
                container_index,
                slot_index,
                x,
                y,
                base_record,
                prefix_record,
                suffix_record,
                modifier_record,
                transmute_record,
                seed,
                component_record,
                component_bonus_record,
                component_seed,
                augment_record,
                ascendant_affix_record,
                ascendant_affix_two_handed_record,
                augment_seed,
                component_combines,
                stack_count,
                ascendant_rerolls,
            },
            block_index,
            base,
            prefix,
            suffix,
            modifier,
            transmute,
            component,
            component_bonus,
            augment,
            ascendant,
            ascendant_two_handed,
            stack_count_offset,
        })
    }

    fn nested(
        &mut self,
        body: impl FnOnce(&mut Self) -> Result<(), GdcError>,
    ) -> Result<(), GdcError> {
        let nested_id = self.i32()?;
        if nested_id != 0 {
            return Err(GdcError::InvalidValue(format!(
                "unexpected nested block: {nested_id}"
            )));
        }
        let length = self.u32()? as usize;
        let nested_end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.end)
            .ok_or(GdcError::InvalidBlockLength {
                block_id: nested_id,
                length: length as i32,
            })?;
        let previous_end = self.end;
        self.end = nested_end;
        body(self)?;
        if self.position != nested_end {
            return Err(GdcError::InvalidValue(format!(
                "nested block ended at {}, expected {nested_end}",
                self.position
            )));
        }
        self.end = previous_end;
        self.u32()?; // stored checksum marker
        Ok(())
    }

    fn ascii(&mut self) -> Result<(TextField, String), GdcError> {
        let length_offset = self.position;
        let length = self.i32()?;
        if length < 0 || length as usize > self.end.saturating_sub(self.position) {
            return Err(GdcError::InvalidValue(format!(
                "invalid item-record length: {length}"
            )));
        }
        let data_start = self.position;
        let data_end = data_start + length as usize;
        let bytes = &self.bytes[data_start..data_end];
        if !bytes.is_ascii() {
            return Err(GdcError::InvalidText { encoding: "ASCII" });
        }
        self.position = data_end;
        Ok((
            TextField {
                length_offset,
                data_end,
            },
            String::from_utf8(bytes.to_vec()).expect("ASCII is valid UTF-8"),
        ))
    }

    fn utf16(&mut self) -> Result<(), GdcError> {
        let length = self.i32()?;
        let byte_length = usize::try_from(length)
            .ok()
            .and_then(|length| length.checked_mul(2))
            .filter(|length| *length <= self.end.saturating_sub(self.position))
            .ok_or_else(|| GdcError::InvalidValue("invalid UTF-16 text".into()))?;
        self.position += byte_length;
        Ok(())
    }

    fn count(&mut self, maximum: i32) -> Result<usize, GdcError> {
        let count = self.i32()?;
        if !(0..=maximum).contains(&count) {
            return Err(GdcError::InvalidValue(format!("invalid count: {count}")));
        }
        Ok(count as usize)
    }

    fn bool(&mut self) -> Result<bool, GdcError> {
        let value = *self.bytes.get(self.position).ok_or(GdcError::Truncated {
            position: self.position,
            needed: 1,
        })?;
        self.position += 1;
        Ok(value == 1)
    }

    fn i32(&mut self) -> Result<i32, GdcError> {
        Ok(self.u32()? as i32)
    }

    fn u32(&mut self) -> Result<u32, GdcError> {
        let end = self.position + 4;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or(GdcError::Truncated {
                position: self.position,
                needed: 4,
            })?;
        self.position = end;
        Ok(u32::from_le_bytes(bytes.try_into().expect("slice checked")))
    }

    fn finish(&self) -> Result<(), GdcError> {
        if self.position == self.end {
            Ok(())
        } else {
            Err(GdcError::InvalidValue(format!(
                "item block contains {} unconsumed bytes",
                self.end - self.position
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fangs_item_v11_uses_the_verified_field_order() {
        let requested = NewInventoryItem {
            bag_index: 0,
            x: 3,
            y: 2,
            base_record: "records/items/gearweapons/swords/weapon_sworda01.dbr".into(),
            stack_count: 4,
        };
        let (payload, markers) = encode_new_item(11, &requested).unwrap();
        let mut cursor = LogicalCursor::new(&payload);
        let parsed = cursor
            .item(11, 0, ItemContainer::Inventory, 0, 0, ItemTail::Coordinates)
            .unwrap();

        assert_eq!(parsed.item.base_record, requested.base_record);
        assert_eq!(parsed.item.stack_count, requested.stack_count);
        assert_eq!(parsed.item.x, Some(requested.x));
        assert_eq!(parsed.item.y, Some(requested.y));
        assert_eq!(parsed.item.ascendant_rerolls, 0);
        assert!(!markers.is_empty());
        cursor.finish().unwrap();
    }

    #[test]
    fn components_and_augments_are_encoded_as_standalone_inventory_items() {
        for record in [
            "records/items/materia/comp_aether_01.dbr",
            "records/items/enchantments/augment_aether_01.dbr",
        ] {
            let requested = NewInventoryItem {
                bag_index: 2,
                x: 4,
                y: 1,
                base_record: record.into(),
                stack_count: 1,
            };
            let (payload, _) = encode_new_item(11, &requested).unwrap();
            let mut cursor = LogicalCursor::new(&payload);
            let parsed = cursor
                .item(11, 0, ItemContainer::Inventory, 2, 0, ItemTail::Coordinates)
                .unwrap();

            assert_eq!(parsed.item.base_record, record);
            assert!(parsed.item.component_record.is_empty());
            assert!(parsed.item.augment_record.is_empty());
            assert_eq!(parsed.item.stack_count, 1);
            assert_eq!(parsed.item.x, Some(4));
            assert_eq!(parsed.item.y, Some(1));
            cursor.finish().unwrap();
        }
    }
}
