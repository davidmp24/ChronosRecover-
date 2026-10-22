use anyhow::Result;
use chronos_engine::server::service::RecoveryEngineService;
use chronos_engine::storage::db::ForensicDatabase;
use chronos_proto::recovery::recovery_engine_server::RecoveryEngineServer;
use std::net::SocketAddr;
use std::path::PathBuf;
use tonic::transport::Server;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    println!("=======================================================");
    println!(" ChronosRecover Engine Daemon v0.1.0                   ");
    println!(" Forensic Data Recovery & Device History Auditor       ");
    println!("=======================================================");

    // Initialize SQLite WAL database in user app data directory or local folder
    let db_path = PathBuf::from("chronos_forensics.db");
    info!(path = ?db_path, "Initializing Forensic SQLite WAL Database");
    let db = ForensicDatabase::open(&db_path)?;

    let service = RecoveryEngineService::new(db);

    let addr: SocketAddr = "127.0.0.1:23007".parse()?;
    println!("[+] Chronos Engine gRPC Daemon escutando em: {}", addr);
    println!("[+] Named Pipe fallback: \\\\.\\pipe\\chronos_engine");
    println!("[+] Pressione Ctrl+C para encerrar o daemon.");

    Server::builder()
        .add_service(RecoveryEngineServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
