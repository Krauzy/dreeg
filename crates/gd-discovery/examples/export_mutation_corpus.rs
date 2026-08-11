use gd_discovery::discover;
use gd_save::{FactionPatch, ItemContainer, NewInventoryItem, encode_mutation, parse, read_file};
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn main() {
    let output = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("usage: export_mutation_corpus <output-directory>");
    fs::create_dir_all(&output).expect("create output directory");

    let mut seen = BTreeSet::new();
    for entry in discover(&[]) {
        if !seen.insert(entry.path.clone()) {
            continue;
        }
        let mut document = read_file(&entry.path).expect("read source save");
        assert!(document.mutation_supported(), "source has opaque blocks");

        if let Some(mut stats) = document.core_stats().expect("read core stats") {
            stats.experience = stats.experience.saturating_add(1);
            document.set_core_stats(&stats).expect("update experience");
        }
        if let Some(iron) = document.iron().expect("read iron") {
            document
                .set_iron(iron.saturating_add(1))
                .expect("update iron");
        }
        if let Some(faction) = document
            .factions()
            .expect("read factions")
            .into_iter()
            .find(|faction| faction.unlocked && faction.value < 24_999.0)
        {
            document
                .apply_faction_patch(&FactionPatch {
                    index: faction.index,
                    value: faction.value + 1.0,
                })
                .expect("update faction");
        }
        let encoded = encode_mutation(&document).expect("encode mutated save");
        let reparsed = parse(&encoded).expect("reparse mutated save");
        assert!(reparsed.mutation_supported(), "output has opaque blocks");
        let target = output.join(format!("{}.gdc", entry.summary.id));
        fs::write(&target, encoded).expect("write mutation corpus file");
        println!("{} -> {}", entry.summary.name, target.display());

        let inventory = document.items().expect("read inventory for insertion test");
        if let Some(template) = inventory
            .iter()
            .find(|item| item.container == ItemContainer::Inventory && !item.base_record.is_empty())
        {
            let occupied = inventory
                .iter()
                .filter(|item| {
                    item.container == ItemContainer::Inventory && item.container_index == 0
                })
                .filter_map(|item| item.x.zip(item.y))
                .collect::<BTreeSet<_>>();
            let free_positions = (0..16)
                .flat_map(|y| (0..16).map(move |x| (x, y)))
                .filter(|position| !occupied.contains(position))
                .take(3)
                .collect::<Vec<_>>();
            let variants = [
                ("item", template.base_record.as_str()),
                ("component", "records/items/materia/comp_aether_01.dbr"),
                (
                    "augment",
                    "records/items/enchantments/augment_aether_01.dbr",
                ),
            ];
            for ((suffix, base_record), (x, y)) in variants.into_iter().zip(free_positions) {
                let mut inserted = document.clone();
                inserted
                    .add_inventory_item(&NewInventoryItem {
                        bag_index: 0,
                        x,
                        y,
                        base_record: base_record.into(),
                        stack_count: 1,
                    })
                    .expect("insert inventory item");
                let encoded = encode_mutation(&inserted).expect("encode inserted item");
                let reparsed = parse(&encoded).expect("reparse inserted item");
                assert!(
                    reparsed.mutation_supported(),
                    "inserted output has opaque blocks"
                );
                let target = output.join(format!("{}-insert-{suffix}.gdc", entry.summary.id));
                fs::write(&target, encoded).expect("write insertion corpus file");
                println!(
                    "{} {suffix} insertion -> {}",
                    entry.summary.name,
                    target.display()
                );
            }
        }
    }
}
