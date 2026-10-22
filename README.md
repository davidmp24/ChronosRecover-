# ChronosRecover ⏱️🛡️
### Industrial Forensic Data Recovery & Device History Auditor Suite

[![Rust](https://img.shields.io/badge/Rust-1.78%2B-orange.svg)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2.0-blue.svg)](https://tauri.app/)
[![React](https://img.shields.io/badge/React-18-cyan.svg)](https://react.dev/)
[![TailwindCSS](https://img.shields.io/badge/TailwindCSS-3.4-38bdf8.svg)](https://tailwindcss.com/)
[![License](https://img.shields.io/badge/License-Proprietary-red.svg)]()

**ChronosRecover** é um software industrial de alta performance projetado para **Engenharia Forense Digital**, auditoria de histórico de dispositivos de armazenamento e recuperação avançada de dados deletados, corrompidos, formatados ou fragmentados.

O sistema opera em nível físico e lógico bruto (**Raw Disk Access**), com suporte a **HDs mecânicos, SSDs NVMe/SATA, Pendrives USB, Cartões SD (FAT32/exFAT)** e imagens de disco forenses (`.raw`, `.img`, `.dd`).

---

## 🏛️ 1. Arquitetura do Sistema

O projeto adota um padrão estrito de desacoplamento de processos através de IPC assíncrono (gRPC + Protocol Buffers):

```
+-----------------------------------------------------------------------------------+
|                        chronos-ui (Tauri 2 + React + TS)                          |
|             Interface Reativa com Telemetria LBA, Timeline e Hex Viewer           |
+-----------------------------------------------------------------------------------+
                                        |  gRPC Stream (Porta 23007 / Named Pipe)
                                        v
+-----------------------------------------------------------------------------------+
|                       chronos-engine (Backend Daemon em Rust)                     |
|           HAL Win32 Raw Disk, Parsers Forenses, SQLite WAL e Safe Exporter        |
+-----------------------------------------------------------------------------------+
        |                                       |
        | Ring Buffer Pool (4-8 MB)             | Watchdog 3000ms & Sector Skipping
        v                                       v
+-----------------------------------+   +-------------------------------------------+
| Multi-Threaded SIMD Aho-Corasick  |   | Hardware Abstraction Layer (HAL)          |
| Carving de Imagens, Vídeos e Docs |   | \\.\PhysicalDriveN, IOCTLs, CreateFileW   |
+-----------------------------------+   +-------------------------------------------+
```

### Componentes do Workspace:
1. **`chronos-proto`**: Especificação gRPC em Protocol Buffers (`recovery.proto`) com streaming em tempo real e paginação virtual para milhões de arquivos.
2. **`chronos-engine`**: Motor core em Rust com acesso direto ao hardware, drivers nativos de sistemas de arquivos e persistência de sessão em **SQLite WAL**.
3. **`chronos-cli`**: Utilitário de linha de comando para diagnósticos de hardware, testes de resiliência e varreduras automatizadas.
4. **`chronos-ui`**: Aplicação desktop desenvolvida em **Tauri 2 + React 18 + TypeScript + TailwindCSS** com visual dark industrial.

---

## ⚡ 2. Módulos Técnicos e Funcionalidades

### 🔌 Camada de Abstração de Hardware (HAL) & Resiliência
- **Win32 Raw Physical Access:** Abertura direta de `\\.\PhysicalDriveN` e `\\.\Volume{GUID}` via `CreateFileW` em modo estrito `GENERIC_READ` com flags `FILE_FLAG_OVERLAPPED | FILE_FLAG_NO_BUFFERING` e alinhamento de 4KB.
- **Watchdog com Timeout de 3000ms:** Cada bloco lido é monitorado por um watchdog assíncrono via IOCP/Overlapped. Se a mídia apresentar atraso excessivo ou travamento mecânico, a I/O é abortada no kernel via `CancelIoEx`, marcando o setor como defeituoso.
- **Adaptive Sector Skipping:** Algoritmo exponencial de salto de setores ($1 \to 2 \to 4 \to 8 \to \dots \to 64$ setores) para transpassar áreas corrompidas de NAND ou setores físicos danificados sem travar o motor.
- **Suporte Multiplataforma & Imagens Forenses:** Suporte a Linux (`O_DIRECT` / `pread`) e leitor virtual de imagens (`.img`, `.raw`, `.dd`) com tabela de injeção de falhas para testes determinísticos.

### 🧩 Analisador de Partições & Boot Sectors Órfãos
- **MBR & GPT Parser:** Validação do LBA 0 (incluindo Protective MBR `0xEE`), LBA 1-33 (GPT Primária) e últimos 33 setores (Backup GPT), checando os hashes CRC32 do cabeçalho e da tabela de partições.
- **Carving de VBRs Órfãos:** Scanner profundo para localizar partições mesmo se a tabela tiver sido apagada ou formatada:
  - **NTFS:** `EB 52 90 4E 54 46 53` (`\xEB\x52\x90NTFS `)
  - **FAT32:** `EB 58 90 4D 53 44 4F 53` (`\xEB\x58\x90MSDOS`)
  - **exFAT:** `EB 76 90 45 58 46 41 54` (`\xEB\x76\x90EXFAT `)
  - **EXT4:** Superblock Magic `0xEF53` no offset 1024.

### 🔍 Drivers de Sistemas de Arquivos & Histórico Forense
- **Driver NTFS (`fs/ntfs`):**
  - Leitura e parsing de registros da `$MFT` de 1024 bytes com validação de fixups USA (*Update Sequence Array*).
  - Identificação de arquivos deletados através da flag `is_in_use == false` (`0x0000`).
  - Decodificação completa de **Data Runs (Runlists)** com offsets relativos/deltas com sinal e arquivos esparsos.
  - **Cálculo de Recoverability Score (0 a 100%):** Cruzamento da *Runlist* do arquivo deletado com o `$BITMAP` do volume para verificar se os clusters continuam livres ou foram sobregravados.
  - **Auditoria de Histórico do Dispositivo (`$UsnJrnl:$J`):** Reconstituição da linha do tempo cronológica com eventos de `FILE_CREATE`, `FILE_DELETE`, `FILE_RENAME` e `DATA_TRUNCATE`.
- **Driver FAT32 & exFAT (`fs/fat` - Foco em Pendrives e SD Cards):**
  - Detecção de entradas deletadas marcadas com byte `0xE5` (FAT32) e `0x05`/`0x85` (exFAT).
  - Reconstrutor de **Long File Names (LFN)** em ordem reversa com validação de checksum.
  - Reconstituição de cadeias de clusters mesmo quando o sistema operacional zera a File Allocation Table na exclusão.
- **Driver EXT4 Linux (`fs/ext4`):**
  - Mapeamento de extents via cabeçalho `eh_magic == 0xF30A` e varredura de Inodes deletados (`dtime != 0` e `links_count == 0`).

### 🚀 Motor de Deep RAW File Carving Concorrente
- **Arquitetura Producer-Consumer:** Pool de *Ring Buffers* alinhados de 4 MB a 8 MB gerenciados via canais `crossbeam`, sem sobrecarga de desalocação contínua de memória.
- **SIMD Aho-Corasick:** Autômato de alta performance capaz de buscar dezenas de assinaturas binárias em uma única passagem linear da memória.
- **Catálogo de Assinaturas Suportadas:**
  - **Imagens:** JPEG (`FF D8 FF` ... `FF D9`), PNG (`89 50 4E 47` ... `49 45 4E 44`), WebP (`RIFF...WEBP`), RAW de câmeras profissionais (CR2, NEF, ARW, DNG via TIFF LE `49 49 2A 00` e BE `4D 4D 00 2A`).
  - **Vídeo & Áudio:** MP4/MOV (átomos `ftyp`, `moov`, `mdat`), MKV/WebM (`1A 45 DF A3`), AVI (`RIFF...AVI `), MP3 (ID3), WAV.
  - **Documentos & Arquivos:** PDF (`%PDF-` ... `%%EOF`), ZIP/DOCX/XLSX/PPTX (`PK 03 04` inspecionando `[Content_Types].xml`), RAR (`52 61 72 21 1A 07`), 7z (`37 7A BC AF 27 1C`), SQLite 3 (`SQLite format 3\0`).
- **Análise de Fragmentação em Cartões SD:** Verificação de continuidade de marcadores de reinicialização JPEG (`RST0` a `RST7`) e cálculo de entropia de Shannon de 512 bytes para detecção de quebras de fluxo.

### 🛡️ Trava de Segurança Forense (`Safe Exporter`)
- Antes de iniciar a gravação de qualquer arquivo recuperado, o sistema executa `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` na pasta de destino. Se o destino estiver no **mesmo disco físico de origem**, a operação é **bloqueada imediatamente**, impedindo que a recuperação destrua clusters deletados ainda não recuperados.

---

## 🛠️ 3. Como Compilar e Executar

### Pré-requisitos
- **Rust Toolchain:** Rust 1.78+ (`rustup`)
- **Node.js:** Versão 18+ e `npm`
- **Privilégios de Administrador:** Necessários no Windows para abrir `\\.\PhysicalDriveN`.

---

### Passo 1: Gerar a Imagem de Teste Sintética
Para testar todos os motores sem risco a unidades reais, gere a imagem forense pré-configurada de 16 MB:
```cmd
node scripts/create_test_image.cjs
```
*(Gera o arquivo `test_forensic_disk.img` contendo MBR, FAT32, entradas `0xE5`, arquivos JPEG, PDF e SQLite)*.

---

### Passo 2: Executar Testes Unitários e de Integração
Execute a suíte de testes em Rust para validar o reader resiliente, a matemática de runlists, o cálculo de integridade no `$BITMAP` e o carving SIMD:
```bash
cargo test --workspace
```

---

### Passo 3: Utilizar a CLI Forense (`chronos-cli`)
A CLI permite testes diretos e diagnósticos:

```bash
# 1. Listar discos físicos conectados no Windows
cargo run -p chronos-cli -- list

# 2. Testar leitura resiliente com watchdog na imagem de teste
cargo run -p chronos-cli -- test-read test_forensic_disk.img

# 3. Detectar partições ativas e boot sectors órfãos
cargo run -p chronos-cli -- scan-partitions test_forensic_disk.img

# 4. Executar Quick Scan (recuperação de metadados e arquivos deletados)
cargo run -p chronos-cli -- quick-scan test_forensic_disk.img

# 5. Executar Deep Carving via SIMD Aho-Corasick
cargo run -p chronos-cli -- carve test_forensic_disk.img
```

---

### Passo 4: Iniciar o Daemon Backend (`chronos-engine`)
Inicie o motor como um serviço gRPC de alta performance escutando em `127.0.0.1:23007`:
```bash
cargo run -p chronos-engine
```

---

### Passo 5: Executar o Frontend Desktop (`chronos-ui`)
Em outro terminal, acesse a pasta da UI e inicie o ambiente:
```cmd
cd chronos-ui
npm install
npm run dev
```
Acesse a aplicação no seu navegador em: **`http://localhost:5173`** (ou compile o executável desktop nativo com `npm run tauri dev`).

---

## 📋 4. Estrutura do Repositório

```
Recovery MP/
├── Cargo.toml                       # Workspace Cargo
├── .gitignore                       # Filtro de binários, imagens e temporários
├── README.md                        # Documentação Técnica
├── scripts/
│   └── create_test_image.cjs        # Gerador determinístico de imagens de teste
├── chronos-proto/                   # Definições gRPC / Protobuf
│   └── proto/recovery.proto
├── chronos-engine/                  # Daemon Core em Rust
│   ├── schema.sql                   # Banco SQLite WAL
│   └── src/
│       ├── hal/                     # Win32 Raw Disk, Watchdog 3s, Sector Skipping
│       ├── partition/               # MBR, GPT, Orphan VBR Carving
│       ├── fs/                      # NTFS ($MFT, $BITMAP, $UsnJrnl), FAT32, exFAT, EXT4
│       ├── carver/                  # SIMD Aho-Corasick & Ring Buffers
│       └── server/                  # gRPC Service & Safe Exporter
├── chronos-cli/                     # Utilitário CLI
└── chronos-ui/                      # Desktop GUI (Tauri + React + Tailwind)
```

---

## ⚖️ 5. Aviso Legal e Conformidade Forense
Este software foi desenvolvido para fins legítimos de **Recuperação de Dados, Auditoria de Sistemas e Computação Forense**. Todas as operações de leitura em discos brutos são estritamente executadas em modo de leitura (`GENERIC_READ` / `O_RDONLY`). Nunca tente recuperar dados salvando-os na mesma unidade física de origem.
