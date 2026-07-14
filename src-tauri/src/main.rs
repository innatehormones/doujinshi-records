#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[tokio::main]
async fn main() {
    let cfg = doujinshi_records::config::AppConfig::load()
        .expect("failed to load config");
    cfg.ensure_dirs().expect("failed to ensure dirs");

    let db_path = cfg.db_path();
    match doujinshi_records::db::recovery::probe_and_recover(&db_path).await {
        Ok(doujinshi_records::db::recovery::RecoveryAction::BackedUp { backup_path }) => {
            eprintln!("WARN: corrupt db moved to {}, recreating", backup_path.display());
        }
        Ok(doujinshi_records::db::recovery::RecoveryAction::Noop) => {}
        Err(e) => {
            eprintln!("db recovery probe failed: {:?}", e);
            std::process::exit(1);
        }
    }
    let conn = doujinshi_records::db::connect(&cfg.db_path())
        .await
        .expect("failed to connect db");
    doujinshi_records::db::migrations::init_schema_versioned_with_covers_dir(
        &conn,
        Some(&cfg.covers_dir()),
    )
    .await
    .expect("failed to init schema");

    println!("resources dir: {}", cfg.resources_dir.display());
    println!("db: {}", cfg.db_path().display());

    doujinshi_records::run(cfg, conn).await;
}
