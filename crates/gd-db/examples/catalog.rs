fn main() {
    let location = gd_db::discover_game_database().expect("Grim Dawn installation not found");
    let database = gd_db::load_database(&location).expect("database should load");
    let catalog = &database.catalog;
    let mut base = 0;
    let mut prefix = 0;
    let mut suffix = 0;
    let mut component = 0;
    let mut augment = 0;
    let mut ascendant = 0;
    let mut icons = 0;
    for item in catalog {
        match item.kind {
            gd_db::CatalogKind::Base => base += 1,
            gd_db::CatalogKind::Prefix => prefix += 1,
            gd_db::CatalogKind::Suffix => suffix += 1,
            gd_db::CatalogKind::Component => component += 1,
            gd_db::CatalogKind::Augment => augment += 1,
            gd_db::CatalogKind::Ascendant => ascendant += 1,
        }
        if item.icon_path.is_some() {
            icons += 1;
        }
    }
    println!(
        "databases={}, localization={}, catalog={}, icons={icons}, base={base}, prefix={prefix}, suffix={suffix}, component={component}, augment={augment}, ascendant={ascendant}",
        location.database_files.len(),
        location.localization_files.len(),
        catalog.len(),
    );
    let missing_artwork = catalog
        .iter()
        .filter(|item| {
            matches!(
                item.kind,
                gd_db::CatalogKind::Base
                    | gd_db::CatalogKind::Component
                    | gd_db::CatalogKind::Augment
            ) && item.icon_path.is_none()
        })
        .collect::<Vec<_>>();
    println!(
        "selectable-records-without-artwork={}",
        missing_artwork.len()
    );
    for item in &missing_artwork {
        println!("fallback-artwork={}, record={}", item.name, item.record);
    }
    for item in database
        .catalog
        .iter()
        .filter(|item| item.icon_path.is_some())
        .take(8)
    {
        println!(
            "icon={}, record={}",
            item.icon_path.as_deref().unwrap_or_default(),
            item.record
        );
    }
    std::fs::create_dir_all("target/icon-research").expect("create icon output");
    let resources = gd_db::load_resource_index(&location).expect("load resource index");
    for (kind, label) in [
        (gd_db::CatalogKind::Base, "item"),
        (gd_db::CatalogKind::Component, "component"),
        (gd_db::CatalogKind::Augment, "augment"),
    ] {
        let sample = database
            .catalog
            .iter()
            .find(|item| item.kind == kind && item.icon_path.is_some())
            .and_then(|item| item.icon_path.as_deref())
            .expect("catalog icon");
        let thumbnail = resources
            .thumbnail_png(sample, 72)
            .expect("decode catalog icon")
            .expect("catalog icon resource");
        std::fs::write(format!("target/icon-research/{label}.png"), &thumbnail)
            .expect("write sample icon");
        println!("thumbnail-{label}={sample}, png_bytes={}", thumbnail.len());
    }
    for tag in [
        "tagSkillClassName0109",
        "tagSkillClassName0308",
        "tagSkillClassName0910",
        "tagSkillClassName0209",
    ] {
        println!(
            "class={tag}, localized={}",
            database
                .localization
                .get(tag)
                .map_or("<missing>", String::as_str)
        );
    }
    let mut class_tags = database
        .localization
        .iter()
        .filter(|(tag, _)| {
            tag.strip_prefix("tagSkillClassName")
                .is_some_and(|codes| codes.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .collect::<Vec<_>>();
    class_tags.sort_by_key(|(tag, _)| tag.as_str());
    assert_eq!(
        class_tags.len(),
        55,
        "all mastery and dual-class names should be localized"
    );
    println!("class-icons-covered={}", class_tags.len());
    for (tag, name) in class_tags {
        println!("class-icon={tag}, localized={name}");
    }
}
