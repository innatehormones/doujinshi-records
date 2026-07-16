#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[tokio::main]
async fn main() {
    // tracing 默认没 subscriber 静默丢弃；fmt() 给个 stderr 输出
    // 简单初始化就够——配合 RUST_LOG 可过滤级别（默认 INFO）。
    // 默认屏蔽 SeaORM 的 SQL 查询日志（每条 SELECT 都打一次，刷屏）；
    // 排查慢查询时 RUST_LOG=info,sea_orm=info 临时开。
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info,sea_orm=warn,sqlx=warn")
            }),
        )
        .with_target(false)
        .init();

    let cfg = doujinshi_records::config::AppConfig::load()
        .expect("failed to load config");
    cfg.ensure_dirs().expect("failed to ensure dirs");

    let db_path = cfg.db_path();

    // 应用待执行还原（用户在 Settings 点了「还原」后会写 marker，
    // 这里在 DB 打开前 swap 文件——src 坏掉保留 marker 供排查，不影响启动）。
    let restore_marker = cfg.resources_dir.join(".restore-pending.json");
    match doujinshi_records::services::backup::apply_pending_restore(&db_path, &restore_marker).await {
        Ok(Some(src)) => println!("INFO: restore applied from {}", src),
        Ok(None) => {}
        Err(e) => eprintln!("WARN: pending restore failed: {:?}", e),
    }

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
