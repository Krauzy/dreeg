fn main() {
    let characters = gd_discovery::discover(&[]);
    let mut versions = characters
        .iter()
        .map(|entry| entry.summary.data_version)
        .collect::<Vec<_>>();
    versions.sort_unstable();
    versions.dedup();
    println!(
        "characters={}, data_versions={versions:?}",
        characters.len()
    );

    for (root, source) in gd_discovery::default_save_roots() {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
            let save = entry.path().join("player.gdc");
            if save.is_file() {
                let original = std::fs::read(&save).expect("read discovered save");
                match gd_save::parse(&original) {
                    Ok(document) => {
                        let items = document.items().expect("parse character items");
                        let core = document.core_stats().expect("read core stats");
                        let iron = document.iron().expect("read iron");
                        let factions = document.factions().expect("read factions");
                        let skills = document.skills().expect("read skills");
                        let block_three_version = document
                            .blocks
                            .iter()
                            .find(|block| block.id == 3)
                            .and_then(|block| block.payload.get(..4))
                            .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()));
                        let encoded = gd_save::encode(&document).expect("encode parsed save");
                        let noop_item_round_trip = if let Some(first) = items.first() {
                            let mut edited = document.clone();
                            edited
                                .apply_item_patch(&gd_save::ItemPatch::from(first))
                                .expect("apply no-op item patch");
                            gd_save::encode(&edited).expect("encode no-op item patch") == original
                        } else {
                            true
                        };
                        let variable_item_round_trip = if let Some(first) =
                            items.iter().find(|item| !item.base_record.is_empty())
                        {
                            let mut edited = document.clone();
                            let mut patch = gd_save::ItemPatch::from(first);
                            patch.base_record = "records/items/test/dreeg_roundtrip.dbr".into();
                            patch.stack_count = first.stack_count.saturating_add(1);
                            edited
                                .apply_item_patch(&patch)
                                .expect("apply variable-length item patch");
                            let reparsed = gd_save::parse(
                                &gd_save::encode_mutation(&edited).expect("encode changed item"),
                            )
                            .expect("reparse changed item");
                            reparsed
                                .items()
                                .expect("read changed items")
                                .iter()
                                .any(|item| {
                                    item.id == first.id
                                        && item.base_record == patch.base_record
                                        && item.stack_count == patch.stack_count
                                })
                        } else {
                            true
                        };
                        let core_mutation_round_trip = if let Some(mut stats) = core.clone() {
                            let mut edited = document.clone();
                            stats.attribute_points = stats.attribute_points.saturating_add(1);
                            edited
                                .set_core_stats(&stats)
                                .expect("apply core stats patch");
                            let reparsed = gd_save::parse(
                                &gd_save::encode_mutation(&edited)
                                    .expect("encode changed core stats"),
                            )
                            .expect("reparse changed core stats");
                            reparsed.core_stats().expect("read changed core stats") == Some(stats)
                        } else {
                            true
                        };
                        let iron_mutation_round_trip = if let Some(iron) = iron {
                            let mut edited = document.clone();
                            let changed_iron = iron.saturating_add(1);
                            edited.set_iron(changed_iron).expect("apply iron patch");
                            let reparsed = gd_save::parse(
                                &gd_save::encode_mutation(&edited).expect("encode changed iron"),
                            )
                            .expect("reparse changed iron");
                            reparsed.iron().expect("read changed iron") == Some(changed_iron)
                        } else {
                            true
                        };
                        let faction_mutation_round_trip = if let Some(first) = factions.first() {
                            let mut edited = document.clone();
                            let value = (first.value + 1.0).clamp(-20_000.0, 25_000.0);
                            edited
                                .apply_faction_patch(&gd_save::FactionPatch {
                                    index: first.index,
                                    value,
                                })
                                .expect("apply faction patch");
                            let reparsed = gd_save::parse(
                                &gd_save::encode_mutation(&edited).expect("encode faction patch"),
                            )
                            .expect("reparse faction patch");
                            reparsed
                                .factions()
                                .expect("read changed factions")
                                .iter()
                                .any(|faction| {
                                    faction.index == first.index && faction.value == value
                                })
                        } else {
                            true
                        };
                        let item_insertion_round_trip = if let Some(first) =
                            items.iter().find(|item| !item.base_record.is_empty())
                        {
                            let mut edited = document.clone();
                            edited
                                .add_inventory_item(&gd_save::NewInventoryItem {
                                    bag_index: 0,
                                    x: 0,
                                    y: 0,
                                    base_record: first.base_record.clone(),
                                    stack_count: 1,
                                })
                                .expect("insert inventory item");
                            let reparsed = gd_save::parse(
                                &gd_save::encode_mutation(&edited).expect("encode inserted item"),
                            )
                            .expect("reparse inserted item");
                            reparsed.items().expect("read inserted items").len() == items.len() + 1
                        } else {
                            true
                        };
                        let first_difference = original
                            .iter()
                            .zip(&encoded)
                            .position(|(left, right)| left != right);
                        println!(
                            "source={source:?}, status=ok, version={}, class_tag={}, block3={block_three_version:?}, core_level={:?}, iron={iron:?}, physique_finite={}, items={}, factions={}, skills_readable={}, mutation_supported={}, mutation_blockers={:?}, exact_round_trip={}, core_mutation_round_trip={core_mutation_round_trip}, iron_mutation_round_trip={iron_mutation_round_trip}, faction_mutation_round_trip={faction_mutation_round_trip}, item_insertion_round_trip={item_insertion_round_trip}, noop_item_round_trip={noop_item_round_trip}, variable_item_round_trip={variable_item_round_trip}, lengths={}/{}, first_difference={first_difference:?}",
                            document.data_version,
                            document.header.class_name,
                            core.as_ref().map(|stats| stats.level_in_bio),
                            core.as_ref().is_none_or(
                                |stats| stats.physique.is_finite() && stats.physique > 0.0
                            ),
                            items.len(),
                            factions.len(),
                            skills.is_some(),
                            document.mutation_supported(),
                            document.mutation_blockers(),
                            original == encoded,
                            original.len(),
                            encoded.len()
                        )
                    }
                    Err(error) => println!("source={source:?}, status=error, reason={error}"),
                }
            }
        }
    }
}
