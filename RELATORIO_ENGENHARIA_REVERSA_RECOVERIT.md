# Relatório de Engenharia Reversa e Arquitetura de Sistemas
## Alvo: Wondershare Recoverit (Data Recovery Suite)
**Data da Análise:** 21 de Outubro de 2026  
**Destinatário:** Engenheiro de Sistemas / Arquiteto de Software  
**Caminho do Alvo:** `C:\Program Files\Wondershare\Recovery`  
**Versão do Binário:** `15.0.0.22` (Engine Version: `14.0.0.993-3868550da`)  
**Metodologia:** Análise Estática de Binários (PE Headers, Exports, Imports, RTTI), Desmontagem de Rotas, Análise Estrutural de Plugins, Inspeção de IPC e Engenharia de Sinais/Logs.

---

## 1. Sumário Executivo

O Wondershare Recoverit é uma suíte profissional de recuperação de dados, reparo de mídia e gerenciamento de partições para ambientes corporativos e de usuário final. A engenharia do software não é monolítica; trata-se de um sistema distribuído localmente com separação estrita entre:
1. **Frontend / Apresentação (GUI):** Interface gráfica moderna baseada em C++ / Qt 5.
2. **Motor de Serviços (Engine Server):** Daemon desacoplado gerenciado via RPC (gRPC / Protocol Buffers).
3. **Camada de Abstração de Hardware e Kernel (Device Driver & OS IOCTLs):** Acesso direto a discos brutos (`\\.\PhysicalDriveN` e `\\.\Volume{GUID}`).
4. **Módulos Especialistas de Reconstrução:** Algoritmos dedicados para parsing de estruturas de arquivos (MFT, FAT, Inodes), Carving heurístico, reparo de codecs de vídeo (MP4/MOV/H.264/HEVC) e inferência de visão computacional (ONNX).

---

## 2. Topologia de Processos e Arquitetura de IPC

```
+-----------------------------------------------------------------------------------+
|                              recoverit.exe (PID: UI)                               |
|                     Interface Gráfica Qt5 + Sistema de Plugins                    |
+-----------------------------------------------------------------------------------+
       |                                                                     ^
       | gRPC Client (DrRequestServiceImpl)                                  | gRPC Stream
       | TransNotify / RPC Calls                                             | GetDataRespond
       v                                                                     |
+-----------------------------------------------------------------------------------+
|                        drengsrv.exe (PID: Engine Server)                          |
|             Porta de Escuta: 0.0.0.0:23007 (Localhost Loopback IPC)               |
+-----------------------------------------------------------------------------------+
       |                                                                     |
       | IPC / Process Control                                               | Shared Memory /
       v                                                                     | Sector Buffers
+-----------------------------------------------+                            |
|             drdaemon.exe (Daemon)             |                            v
|     Operações Críticas em Background / Watchdog|          +-----------------------------------+
+-----------------------------------------------+          |       scanopreator.dll /          |
       |                                                   |     PartitionManager.dll          |
       v                                                   |       (Core de Análise)           |
+------------------------------------------------+         +-----------------------------------+
|  Acesso ao Kernel / Discos Físicos do Windows  |                           |
|       (\\.\PhysicalDriveN, DeviceIoControl)    | <-------------------------+
+------------------------------------------------+
```

### 2.1. O Pipeline gRPC Local
- O binário `recoverit.exe` inicializa a classe cliente `dr::IDrServiceRequest` / `DrRequestServiceImpl`.
- O servidor de backend `drengsrv.exe` é iniciado como um processo independente escutando na porta TCP `0.0.0.0:23007` (limite máximo de buffer configurado para 15 MB: `max_receive_size = 15728640`, `max_send_size = 15728640`).
- **Comunicação Bidirecional:**
  - `CNotifyFromClient::TransNotify`: Envia ordens de escaneamento, filtros, comandos de pausa/cancelamento e credenciais.
  - `CNotifyFromServer::GetDataRespond`: Transmite o progresso em tempo real (`QuickScanProgress`, `kMsgCurrentScanningPath`), setores processados e eventos de descoberta de arquivos.
- **Isolamento de Falha:** Se um driver de disco trava ou um setor defeituoso gera uma exceção não tratada na leitura de baixo nível, apenas o worker ou o daemon sofre a interrupção, mantendo o processo principal e o histórico preservados.

---

## 3. O Sistema Modular de Plugins (Qt Dynamic Plugins)

Todos os módulos funcionais estão localizados na pasta `plugins/` e compilados como DLLs exportando a interface padrão do Qt:
- `qt_plugin_instance`
- `qt_plugin_query_metadata`

O carregamento é dinâmico via `QPluginLoader`, permitindo modularidade absoluta.

### Catálogo de Plugins Identificados

| Categoria | Nome da DLL | Responsabilidade de Engenharia |
| :--- | :--- | :--- |
| **Escaneamento & Core** | `DrengsrvPlugin.dll` | Bridge de controle do servidor gRPC, orquestrador do ciclo de vida da recuperação. |
| | `HardDrivesPlugin.dll` | Mapeamento e renderização de unidades físicas, lógicas e volumes montados. |
| | `PartitionRecoveryPlugin.dll` | Recuperação de partições deletadas, corrompidas ou sobrescritas. |
| | `SDCardPlugin.dll` / `SDCardScanPlugin.dll` | Fluxo especializado em mídias removíveis (Flash, SD, MicroSD). |
| | `ScanDataViewPlugin.dll` | Exibição de alta performance das árvores de arquivos escaneados. |
| **Segurança & Criptografia** | `BitLockerPlugin.dll` | Detecção de volumes BitLocker, injeção de Recovery Password e descriptografia sob demanda. |
| **Rede & Sistemas Remotos**| `NASAndLinuxPlugin.dll` | Recuperação remota de sistemas Linux e appliances NAS via protocolo SSH (sem desmontar discos). |
| **Reparo de Dados** | `VideoRepairPlugin.dll` | Reparo simples e avançado de arquivos de vídeo MP4, MOV, MKV, AVI. |
| | `DocumentRepairPlugin.dll` | Reparo estrutural de documentos corrompidos (Word, Excel, PowerPoint, PDF). |
| | `ImageRepairPlugin.dll` | Correção de cabeçalhos de imagem corrompidos e blocos JPEG. |
| | `FileRepairPlugin.dll` | Orquestrador central dos subsistemas de reparo. |
| **Inteligência Artificial** | `SDCardAiTagsPlugin.dll` | Classificação automática de imagens recuperadas por tags semânticas via modelo de visão. |
| | `AiEnhancePlugin.dll` | Upscaling e restauração de detalhes via redes neurais. |
| **Discos & Particionamento**| `PartitionManagerPlugin.dll` | Redimensionamento, formatação, conversão MBR/GPT e particionamento. |
| | `DiskClonePlugin.dll` | Clonagem setor a setor e migração de sistema operacional. |
| | `SystemCrashPlugin.dll` | Criação de mídia inicializável WinPE para recuperação de computadores que não dão boot. |
| **Visualização & UI** | `PreviewPlugin.dll` / `PlayerPlugin.dll` | Renderização antecipada de arquivos recuperados (PDF, Office, imagens RAW, mídia). |
| | `SearchPlugin.dll` / `FilterBarPlugin.dll` | Mecanismo de busca e filtros rápidos por tipo, tamanho e data. |

---

## 4. Camada de Abstração de Armazenamento e Hardware

A biblioteca `PartitionManager.dll` e o módulo `drstorage` implementam uma infraestrutura robusta em C++ no namespace `pm`:

### 4.1. Enumeração e Controle de Dispositivos (`Win32DiskProvider`)
- Abertura de handles de dispositivo via Win32:
  - `CreateFileW(L"\\\\.\\PhysicalDrive0", GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, ...)`
- **IOCTLs Cruciais Utilizados:**
  - `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX`: Coleta número de cilindros, pistas por cilindro, setores por pista e tamanho do setor (512e vs 4Kn nativo).
  - `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS`: Mapeia a qual disco físico e offset cada letra de unidade pertence.
  - `IOCTL_DISK_GET_PARTITION_INFO_EX`: Coleta GUIDs, tipo de tabela de partição (MBR vs GPT) e atributos de proteção.
  - `FSCTL_LOCK_VOLUME` e `FSCTL_DISMOUNT_VOLUME`: Utilizados para clonagem de partição e modificações estruturais em baixo nível.

### 4.2. Gerenciamento de Geometria de Partições
- `BuildGPTHeader`, `BuildMBRSector`, `BuildProtectiveMBR`: O Recoverit possui implementações próprias para calcular checksums CRC32 (`CalculateCRC32`) e reescrever a tabela de partição diretamente nos setores LBA 0 e LBA 1-33.
- `DetectBitLockerStatus`: Analisa a presença do bloco FVE (Full Volume Encryption) no cabeçalho do volume antes de tentar a recuperação direta de dados.

---

## 5. Engenharia dos Algoritmos de Escaneamento e Recuperação

O núcleo algorítmico do software reside em `scanopreator.dll`. Ele divide o escaneamento em **três camadas complementares**:

```
[Dispositivo de Armazenamento / Imagem de Disco]
                     │
       ┌─────────────┴─────────────┐
       ▼                           ▼
[Quick Scan: MFT/FAT Tables]    [Deep Scan: Filesystem Parser]
• Parsing de $MFT, $LogFile    • Reconstrução de Runlists
• Leitura de tabelas FAT/FAT32 • Desfragmentação de Inodes
• Busca instantânea via        • Suporte: NTFS, exFAT, EXT4,
  Everything.exe                 APFS, HFS+, Btrfs
       │                           │
       └─────────────┬─────────────┘
                     ▼
        [RAW Scan / Carving Engine]
        • Busca por assinaturas mágicas de cabeçalho e rodapé
        • AVCRawScanOperator / AVIRawAnalyser
        • Reconstrução de blocos não mapeados (Unmatched Blocks)
        • Heurística para JPEGs e Vídeos fragmentados (drjpgfragscan)
```

### 5.1. Camada 1: Quick Scan (Escaneamento de Metadados)
- **NTFS:**
  - Carrega os primeiros registros de `$MFT` e avalia atributos essenciais: `$STANDARD_INFORMATION` (datas e permissões), `$FILE_NAME` (árvore de diretórios), `$DATA` (conteúdo residente ou runlist não-residente) e `$BITMAP` (setores alocados vs livres).
  - Utiliza o executável embutido `Everything.exe` (Voidtools) para instantaneamente indexar arquivos existentes em volumes NTFS sem varredura física demorada.
- **FAT16 / FAT32 / exFAT:**
  - Executa `ReadFatSector` e `BuildClusterChainStartCache`. Mapeia os ponteiros da File Allocation Table para identificar cadeias de clusters marcadas como desalocadas (`0x00000000`) cujo diretório pai ainda contém entradas válidas com o primeiro byte modificado para `0xE5`.

### 5.2. Camada 2: Deep Scan (Operadores Especializados de Sistema de Arquivos)
Para casos de formatação de volume ou corrupção severa da partição, o Recoverit emprega operadores de escaneamento orientados a objetos:
- `CNtfsDelScanOperator`: Reassociação de nós do `$INDEX_ROOT` e `$ATTRIBUTE_LIST` para reconstruir caminhos de pastas mesmo quando os registros iniciais foram destruídos.
- `BuildRunListFromClusterChain`: Converte cadeias contíguas e esparsas de clusters em runlists lógicas válidas.
- `ApfsDelScanOperator` / `ApfsFormatScanOperator`: Analisa os checkpoints e Object Map (omap) do Apple File System (APFS), reconstruindo snapshots anteriores de sistemas macOS.
- `Ext4DelScanOperator` / `Ext4FileSystemScanner`: Processa Block Groups, Inode Tables e Extents Trees do Linux (ext2/ext3/ext4).
- `BtrfsDelScanOperator`: Realiza traversals nos b-trees do Btrfs.
- `HfspDelScanOperator`: Reconstrói nós do catálogo B-Tree do formato legaciatário da Apple (HFS+).

### 5.3. Camada 3: RAW Carving e Reconstrução Heurística
Quando as tabelas de arquivos foram completamente sobrescritas:
- **Operador:** `AVCRawScanOperator` e `AVIRawAnalyser`.
- **Estratégia:** `AVRawHeaderCrackSelectStrategy`. O motor lê blocos contíguos do disco (tamanho típico: 64 KB a 1 MB) e compara os offsets com um catálogo de assinaturas binárias (Magic Bytes):
  - Exemplo: `LocalFileHeader` para ZIP/Office (`50 4B 03 04`), `ftyp` para MP4/MOV, cabeçalhos de JPEG-XL (`JxlSignatureCheck`), câmeras digitais profissionais (CR2, NEF, ARW, FBX, RED R3D).
- **Tratamento de Fragmentação:**
  - O componente `drjpgfragscan.log` implementa algoritmos de verificação de continuidade de coeficientes DCT (Discrete Cosine Transform) e marcadores Restart (`RST0` - `RST7`) para detectar onde um arquivo JPEG foi interrompido e localizar o fragmento seguinte no disco livre.

### 5.4. Persistência de Sessão (Scan Resume)
- O motor implementa `kMsgSaveScanRecordStart` e `kMsgLoadScanRecordStart`.
- A cada `N` blocos processados, o estado da árvore de nós (`TreeItemData`) e a lista de setores escaneados são salvos em disco (formato proprietário ou banco SQLite).
- **Vantagem para o Usuário:** O usuário pode pausar uma varredura de um HD de 8 TB e retomá-la instantaneamente sem reiniciar a leitura do zero.

---

## 6. Módulo de Inteligência Artificial e Visão Computacional

Na pasta `model/`, o Recoverit embarca uma pipeline de Inteligência Artificial para enriquecer a experiência de recuperação de fotos:

### Componentes:
- **Runtime:** `onnxruntime.dll` (Microsoft ONNX Runtime C++).
- **Modelo:** `image_classifier_int8.ort` (Rede neural convolucional / Transformer quantizada em inteiros de 8 bits para alto throughput de CPU sem necessidade de GPU dedicada).
- **Embeddings:** `label_embeddings.npy` (Vetores de embeddings para classificação zero-shot).
- **Labels:** `label_names.json` (Categorias semânticas: `"a photo of a child or baby"`, `"a photo of a pet"`, `"a photo of a beach"`, `"a passport or ID card"`, `"a screenshot of a chat"`, etc.).

### Lógica Operacional:
1. Durante a restauração de um cartão SD (`SDCardAiTagsPlugin.dll`), os arquivos de imagem recuperados são enviados a uma thread de inferência em background.
2. O modelo executa inferência vetorial rápida.
3. As fotos são organizadas na interface não apenas por extensão ou pasta, mas por **tags inteligentes de conteúdo** (Ex.: "Animais", "Documentos/IDs", "Viagens", "Casamentos").

---

## 7. Subsistema de Reparo de Arquivos Corrompidos

O Recoverit se destaca por não apenas extrair bytes brutos, mas reparar arquivos danificados através de `repaircore.dll` e `WsVideoRepairLib.dll`.

### 7.1. Reparo de Documentos do Office (`xlnt` Engine)
- Utiliza a biblioteca `xlnt` e parsers XML para:
  - Validar a integridade do contêiner ZIP (arquivos `.xlsx`, `.docx`, `.pptx`).
  - Reconstruir arquivos `[Content_Types].xml` corrompidos.
  - Reparar tabelas de células e atributos corrompidos sem falhar a abertura no Microsoft Office.

### 7.2. Pipeline de Reparo de Vídeo (Dual-Mode)

```
                       [Arquivo de Vídeo Corrompido]
                                     │
                      Tentativa 1: Reparo Rápido
                    (Quick Repair - repaircore.dll)
                                     │
                 ┌───────────────────┴───────────────────┐
                 ▼                                       ▼
             [Sucesso]                               [Falha]
       (Cabeçalho corrigido)               (Fluxo moov / codec ausente)
                                                         │
                                           Tentativa 2: Reparo Avançado
                                           (Advanced Repair com Sample)
                                                         │
                                        ┌────────────────┴────────────────┐
                                        ▼                                 ▼
                                  [Vídeo Amostra]               [Arquivo Danificado]
                              (Gravado na mesma câmera)          (Payload mdat bruto)
                                        │                                 │
                                        └────────────────┬────────────────┘
                                                         ▼
                                          [Reconstrução do Átomo moov]
                                          • Extração de SPS/PPS (H.264/HEVC)
                                          • Geração da Sample Table (stbl/stsz)
                                          • Cálculo de Offsets (stco)
                                                         │
                                                         ▼
                                            [Arquivo Final Reproduzível]
```

1. **Reparo Rápido (`VideoRepair_QuickRepair`):**
   - Corrige pequenos erros estruturais no cabeçalho do contêiner MP4/MOV (por exemplo, tamanho incorreto do atom `mdat` ou ponteiro `moov` desalinhado).
2. **Reparo Avançado (`VideoRepair_ADRepair` via `loadSample`):**
   - Quando a gravação é interrompida abruptamente (queda de energia na câmera, bateria removida), o cabeçalho `moov` não é gravado, deixando apenas o payload de vídeo bruto no disco (`mdat`).
   - O usuário fornece um vídeo saudável gravado com o mesmo dispositivo.
   - O motor extrai parâmetros críticos de decodificação:
     - Informações da Sequence Parameter Set (SPS) e Picture Parameter Set (PPS).
     - Descritores de faixas de áudio (AAC sample rates, channel layout).
   - O software varre o payload corrompido, localiza os NAL Units do codec de vídeo e reconstrói as tabelas de temporização e índices de amostras (`stsz`, `stco`, `stsc`), gerando um cabeçalho `moov` compatível que torna o arquivo 100% reproduzível.

---

## 8. Particionamento e Recuperação de Desastres

A infraestrutura `pm::controllers::PartitionController` e `pm::infrastructure::WinPEEnvironment` provê recuperação de emergência:
- **Modo Offline WinPE:**
  - Se o sistema operacional principal não inicializa, o Recoverit gera uma mídia WinPE customizada através de `sys_mbr2gpt.exe` e `pm_pe_executor.exe`.
  - Configura o Boot Configuration Data (BCD) do Windows via chamadas nativas (`BackupBcdStore`, `ConfigureBCD`, `CopyWinREResources`).
- **Clonagem e Migração:**
  - Realiza cópia direta de setores (`CloneNTFSPartitionBySegments` e `CloneESPPartition`), ajustando partições de destino para alinhamento em blocos de 1 MB (4K alignment para SSDs modernos).

---

## 9. Recuperação Remota de Redes / NAS / Linux

Em vez de exigir a remoção física de 4 a 12 discos de um servidor NAS (que frequentemente utilizam RAID com matrizes complexas de striping):
- O plugin `NASAndLinuxPlugin.dll` estabelece uma conexão cliente SSH (porta configurável, credenciais de usuário root/sudo).
- Utiliza comandos e scripts de terminal remotos para ler dados em modo raw a partir dos dispositivos `/dev/sdX` ou `/dev/mdX`.
- Transmite os dados pela rede via túnel criptografado para o motor `drengsrv.exe`, que executa a análise localmente na máquina cliente sem exigir a instalação de softwares pesados no servidor de produção.

---

## 10. Blueprint e Recomendações para Construção de uma Nova Solução

Caso um engenheiro de sistemas deseje criar uma solução de recuperação de dados baseada nas melhores práticas observadas nesta arquitetura, a seguinte especificação técnica deve ser adotada:

### 10.1. Stack Tecnológica Recomendada
- **Core Engine / Backend:** C++20 ou Rust (para gerenciamento seguro de memória e controle de concorrência em I/O assíncrono).
- **Interface Gráfica:** Qt 6 (C++) ou Tauri / Flutter desktop (acoplados via gRPC local).
- **Camada de Comunicação:** gRPC + Protobuf sobre Named Pipes (Windows: `\\.\pipe\data_recovery_engine`) ou Unix Domain Sockets (Linux/macOS), reduzindo overhead em comparação a sockets TCP loopback.

### 10.2. Diretrizes de Arquitetura do Sistema
1. **Desacoplamento Processual Estrito:**
   - Nunca execute tarefas de escaneamento de disco bruto ou parse de MFT dentro da thread da UI. Um único setor defeituoso (I/O hang) travará a renderização da janela.
2. **Abstração de Hardware com Cancelamento e Timeout:**
   - Todas as chamadas `ReadFile` contra volumes físicos devem ser assíncronas (`FILE_FLAG_OVERLAPPED`) associadas a um Windows I/O Completion Port (IOCP).
   - Implementar timeout estrito de hardware: se um setor demorar mais de 3 segundos para responder, abortar a leitura via `CancelIoEx` e marcar o cluster como "Bad Sector" sem derrubar a aplicação.
3. **Pipeline de Carving Concorrente (Producer-Consumer):**
   - Utilizar um pool de buffers circulares (Ring Buffers) de 2 a 8 MB.
   - Uma thread *Reader* realiza leituras sequenciais contíguas (maximizando o throughput de leitura sequencial do disco).
   - Um pool de threads *Carvers* consome os buffers e realiza pattern matching de assinaturas mágicas e validação de checksums em paralelo.
4. **Mecanismo de Snapshots e Resume:**
   - Adotar um banco de dados leve como SQLite WAL (Write-Ahead Logging) ou RocksDB embarcado para registrar nós descobertos em tempo real, permitindo pausar e reiniciar o processo instantaneamente.
5. **Módulo de Reparo Orientado a Amostras:**
   - Para reconstrução de mídia MP4/MOV, implementar o parsing de átomos ISO Base Media (`ISO/IEC 14496-12`) permitindo a injeção do átomo `moov` a partir de uma amostra de referência de mesma resolução/codec.

---

### Conclusão

A arquitetura do Wondershare Recoverit demonstra um alto grau de maturidade em engenharia de sistemas: desacoplamento total entre UI e Core via gRPC, extensibilidade por plugins Qt, robustez no tratamento de sistemas de arquivos heterogêneos (NTFS, FAT, APFS, EXT4) e agregação de tecnologias modernas como inferência quantizada de IA e reconstrução estrutural de codecs multimídia. O modelo apresentado serve como referência sólida para a concepção de soluções industriais de recuperação e forense de dados.
