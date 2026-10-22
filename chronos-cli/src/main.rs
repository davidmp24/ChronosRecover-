use anyhow::Result;
use chronos_engine::carver::engine::CarvingEngine;
use chronos_engine::fs::fat::scanner::FatScanner;
use chronos_engine::fs::ntfs::scanner::NtfsScanner;
use chronos_engine::hal::{enumerate_devices, open_device, reader::ResilientDiskReader};
use chronos_engine::partition::vbr_scanner::VbrScanner;
use std::env;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    println!("=======================================================");
    println!(" ChronosRecover - High Performance Forensic CLI v0.1.0 ");
    println!("=======================================================");

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Uso:");
        println!("  chronos-cli list                      - Lista todos os discos físicos detectados");
        println!("  chronos-cli test-read <caminho>       - Executa teste de leitura resiliente com watchdog");
        println!("  chronos-cli scan-partitions <caminho> - Detecta partições ativas e VBRs órfãos");
        println!("  chronos-cli quick-scan <caminho>      - Executa Quick Scan em NTFS/FAT32/exFAT");
        println!("  chronos-cli carve <caminho>           - Executa Deep Carving com SIMD Aho-Corasick");
        println!("\nExemplos:");
        println!(r"  chronos-cli list");
        println!(r"  chronos-cli scan-partitions \\.\PhysicalDrive1");
        println!(r"  chronos-cli quick-scan \\.\PhysicalDrive1");
        println!(r"  chronos-cli carve test_disk.img");
        return Ok(());
    }

    match args[1].as_str() {
        "list" => {
            println!("\n[+] Enumerando discos físicos via Windows/OS HAL...");
            let devices = enumerate_devices();
            if devices.is_empty() {
                println!("[-] Nenhum disco físico enumerado ou privilégios de Administrador necessários.");
                println!("    Dica: Execute o terminal como Administrador para abrir \\\\.\\PhysicalDriveN.");
            } else {
                for dev in devices {
                    println!("\n-------------------------------------------------------");
                    println!(" Dispositivo:   {}", dev.name);
                    println!(" Caminho Raw:   {}", dev.path);
                    println!(" Vendor/Model:  {} / {}", dev.vendor, dev.product);
                    println!(" Serial:        {}", dev.serial_number);
                    println!(" Capacidade:    {:.2} GB ({} bytes)", dev.geometry.total_bytes as f64 / 1e9, dev.geometry.total_bytes);
                    println!(" Setor Lógico:  {} bytes", dev.geometry.bytes_per_sector);
                    println!(" Setor Físico:  {} bytes", dev.geometry.bytes_per_physical_sector);
                    println!(" Removível:     {}", if dev.is_removable { "Sim (USB/SD)" } else { "Não (Fixo)" });
                    println!(" Barramento:    {:?}", dev.bus_type);
                }
            }
        }
        "test-read" => {
            if args.len() < 3 {
                eprintln!("Erro: Especifique o caminho do dispositivo ou arquivo de imagem.");
                return Ok(());
            }
            let target_path = &args[2];
            println!("\n[+] Abrindo dispositivo com I/O assíncrono e timeout de 3000ms: {}", target_path);

            let dev = open_device(target_path)?;
            let desc = dev.descriptor();
            println!("[+] Dispositivo aberto: {} (Total: {} bytes)", desc.name, desc.geometry.total_bytes);

            let mut reader = ResilientDiskReader::new(
                dev,
                0,
                Some(2048),
                Some(128),
            );

            let mut buf = Vec::new();
            let mut bad_sectors_found = 0u64;

            println!("[+] Iniciando varredura resiliente...");
            while !reader.is_done() {
                let chunk = reader.read_next_chunk(&mut buf, |bad_ev| {
                    bad_sectors_found += bad_ev.sector_count;
                    println!("  [!] BAD SECTOR DETECTADO no LBA {}: {}", bad_ev.start_lba, bad_ev.error_message);
                })?;

                if let Some((lba, _)) = chunk {
                    let stats = reader.stats();
                    print!("\r[+] LBA Atual: 0x{:08X} | Setores Lidos: {} | Velocidade: {:.2} MB/s | Bad Sectors: {}", 
                        lba, stats.sectors_read, stats.read_speed_mb_s, bad_sectors_found);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
            }
            println!("\n\n[✓] Teste de leitura finalizado com sucesso!");
        }
        "scan-partitions" => {
            if args.len() < 3 {
                eprintln!("Erro: Especifique o caminho do dispositivo.");
                return Ok(());
            }
            let target_path = &args[2];
            let dev = open_device(target_path)?;
            println!("\n[+] Analisando MBR, GPT e VBRs Órfãos para: {}", target_path);
            let partitions = VbrScanner::scan_partitions(&dev)?;

            if partitions.is_empty() {
                println!("[-] Nenhuma partição reconhecida encontrada.");
            } else {
                for p in partitions {
                    println!("\n  Partição #{}: {}", p.index, p.label);
                    println!("    Sistema:      {}", p.fs_type);
                    println!("    LBA Inicial:  0x{:08X} ({})", p.start_lba, p.start_lba);
                    println!("    Setores:      {}", p.sector_count);
                    println!("    Tamanho:      {:.2} GB", (p.size_bytes as f64) / 1e9);
                    println!("    Órfã (VBR):   {}", if p.is_orphan { "Sim (Carved)" } else { "Não (Tabela MBR/GPT)" });
                    println!("    Cluster:      {} bytes", p.cluster_size);
                }
            }
        }
        "quick-scan" => {
            if args.len() < 3 {
                eprintln!("Erro: Especifique o caminho do dispositivo.");
                return Ok(());
            }
            let target_path = &args[2];
            let dev = open_device(target_path)?;
            let partitions = VbrScanner::scan_partitions(&dev)?;

            for p in partitions {
                println!("\n[+] Executando Quick Scan na Partição {} ({}) no LBA 0x{:08X}...", p.index, p.fs_type, p.start_lba);
                if p.fs_type.contains("NTFS") {
                    if let Ok(mut ntfs) = NtfsScanner::new(dev.clone(), p.start_lba) {
                        ntfs.init_bitmap().ok();
                        let found = ntfs.scan_records("cli-session", 1000, |file| {
                            println!("  [{}] {} ({:.2} KB) - Recoverability: {}% - Status: {:?}",
                                file.extension.to_uppercase(), file.path, file.size_bytes as f64 / 1024.0, file.recoverability_score, file.status);
                        })?;
                        println!("[✓] NTFS Scan concluído: {} arquivos processados.", found);
                    }
                } else if p.fs_type.contains("FAT") || p.fs_type.contains("exFAT") {
                    if let Ok(fat) = FatScanner::new(dev.clone(), p.start_lba, "cli-session") {
                        let found = fat.scan(|file| {
                            println!("  [{}] {} ({:.2} KB) - Recoverability: {}% - Status: {:?}",
                                file.extension.to_uppercase(), file.path, file.size_bytes as f64 / 1024.0, file.recoverability_score, file.status);
                        })?;
                        println!("[✓] FAT/exFAT Scan concluído: {} arquivos processados.", found);
                    }
                }
            }
        }
        "carve" => {
            if args.len() < 3 {
                eprintln!("Erro: Especifique o caminho do dispositivo ou imagem.");
                return Ok(());
            }
            let target_path = &args[2];
            let dev = open_device(target_path)?;
            println!("\n[+] Iniciando Deep RAW File Carving (SIMD Aho-Corasick) em: {}", target_path);

            let carver = CarvingEngine::new();
            let mut count = 0u64;

            carver.run_carve(dev, "cli-session", 0, None, move |file| {
                count += 1;
                println!("  [CARVED #{:05}] [{}] LBA: 0x{:08X} | Tamanho: {:.2} KB | Mime: {}",
                    count, file.extension.to_uppercase(), file.starting_lba, file.size_bytes as f64 / 1024.0, file.mime_type);
            });

            println!("\n[✓] Carving finalizado com sucesso!");
        }
        cmd => {
            eprintln!("Comando desconhecido: {}", cmd);
        }
    }

    Ok(())
}
