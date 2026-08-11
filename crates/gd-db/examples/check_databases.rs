fn main() {
    let location = gd_db::discover_game_database().expect("Grim Dawn installation not found");
    for database_file in &location.database_files {
        let mut candidate = location.clone();
        candidate.database_files = vec![database_file.clone()];
        match gd_db::load_database(&candidate) {
            Ok(database) => println!(
                "ok records={} file={}",
                database.catalog.len(),
                database_file.display()
            ),
            Err(error) => println!("error={error} file={}", database_file.display()),
        }
    }
}
