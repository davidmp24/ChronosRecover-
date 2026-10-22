const fs = require('fs');
const path = require('path');

const SECTOR_SIZE = 512;
const TOTAL_SECTORS = 32768; // 16 MB image
const IMAGE_PATH = path.join(__dirname, '..', 'test_forensic_disk.img');

console.log('====================================================');
console.log(' Gerador de Imagem de Disco Sintética para Testes   ');
console.log(' ChronosRecover Forensic Verification Suite         ');
console.log('====================================================');

const buffer = Buffer.alloc(TOTAL_SECTORS * SECTOR_SIZE, 0);

// 1. Escrever MBR no LBA 0
console.log('[+] Gravando Master Boot Record (MBR) no LBA 0...');
// Assinatura de boot 0x55 0xAA
buffer[510] = 0x55;
buffer[511] = 0xAA;
// Partição 1: Tipo 0x0B (FAT32), LBA inicial = 2048, Tamanho = 20000 setores
const p1_offset = 446;
buffer[p1_offset + 4] = 0x0B; // FAT32
buffer.writeUInt32LE(2048, p1_offset + 8);   // start_lba
buffer.writeUInt32LE(20000, p1_offset + 12); // sector_count

// 2. Escrever VBR FAT32 no LBA 2048
const vbr_offset = 2048 * SECTOR_SIZE;
console.log('[+] Gravando VBR FAT32 no LBA 2048...');
buffer.write('\xEB\x58\x90MSDOS', vbr_offset, 'binary');
buffer.writeUInt16LE(512, vbr_offset + 11); // bytes por setor
buffer[vbr_offset + 13] = 8;                 // 8 setores por cluster (4KB)
buffer.writeUInt16LE(32, vbr_offset + 14);  // 32 setores reservados
buffer[vbr_offset + 16] = 2;                 // 2 FATs
buffer.writeUInt32LE(20000, vbr_offset + 32);// total de setores
buffer.writeUInt32LE(100, vbr_offset + 36);  // setores por FAT
buffer.writeUInt32LE(2, vbr_offset + 44);    // Root dir cluster (2)
buffer.write('CHRONOS_SD ', vbr_offset + 71, 'ascii'); // Volume label
buffer[vbr_offset + 510] = 0x55;
buffer[vbr_offset + 511] = 0xAA;

// 3. Escrever entrada de arquivo deletado no Root Directory (Cluster 2)
// Data start sector = 2048 + 32 + (2 * 100) = 2280
const root_dir_offset = 2280 * SECTOR_SIZE;
console.log('[+] Gravando arquivo DELETADO (0xE5) e arquivo ATIVO no Root Directory (LBA 2280)...');

// Entrada 1: Arquivo Ativo "RELATORIO.TXT"
buffer.write('RELATORITXT', root_dir_offset, 'ascii');
buffer[root_dir_offset + 11] = 0x20; // Archive
buffer.writeUInt16LE(0, root_dir_offset + 20); // Cluster Hi
buffer.writeUInt16LE(3, root_dir_offset + 26); // Cluster Lo (3)
buffer.writeUInt32LE(1024, root_dir_offset + 28); // 1024 bytes

// Entrada 2: Arquivo Deletado "\xE5CIM_01.JPG" (Original: DCIM_01.JPG)
const del_offset = root_dir_offset + 32;
buffer[del_offset] = 0xE5; // Marca de arquivo DELETADO no FAT!
buffer.write('CIM_01 JPG', del_offset + 1, 'ascii');
buffer[del_offset + 11] = 0x20; // Archive
buffer.writeUInt16LE(0, del_offset + 20); // Cluster Hi
buffer.writeUInt16LE(4, del_offset + 26); // Cluster Lo (4)
buffer.writeUInt32LE(4096, del_offset + 28); // 4096 bytes

// 4. Injetar arquivo JPEG completo (Cluster 4 / LBA 2296)
const jpeg_offset = (2280 + (4 - 2) * 8) * SECTOR_SIZE;
console.log(`[+] Injetando imagem JPEG completa com Header FF D8 FF e Footer FF D9 no LBA ${jpeg_offset / SECTOR_SIZE}...`);
buffer[jpeg_offset] = 0xFF;
buffer[jpeg_offset + 1] = 0xD8;
buffer[jpeg_offset + 2] = 0xFF;
buffer[jpeg_offset + 3] = 0xE0;
buffer.write('JFIF\0', jpeg_offset + 6, 'ascii');
buffer.fill(0x55, jpeg_offset + 12, jpeg_offset + 2040);
buffer[jpeg_offset + 2040] = 0xFF;
buffer[jpeg_offset + 2041] = 0xD9; // Footer JPEG

// 5. Injetar arquivo PDF órfão no LBA 6000 para teste do RAW Carver
const pdf_offset = 6000 * SECTOR_SIZE;
console.log('[+] Injetando documento PDF no LBA 6000 para teste do Carver...');
buffer.write('%PDF-1.4\n1 0 obj\n<< /Title (Chronos Forensic Report) >>\nendobj\n%%EOF', pdf_offset, 'utf-8');

// 6. Injetar banco de dados SQLite 3 no LBA 8000
const sqlite_offset = 8000 * SECTOR_SIZE;
console.log('[+] Injetando Banco SQLite 3 no LBA 8000...');
buffer.write('SQLite format 3\0', sqlite_offset, 'binary');
buffer.writeUInt16BE(4096, sqlite_offset + 16); // page size 4096
buffer.writeUInt32BE(4, sqlite_offset + 28);    // 4 pages = 16 KB

// Gravar no arquivo de saída
fs.writeFileSync(IMAGE_PATH, buffer);
console.log('\n[✓] Imagem sintética gerada com sucesso: ' + IMAGE_PATH);
console.log(`    Tamanho: ${(buffer.length / (1024 * 1024)).toFixed(2)} MB (${buffer.length} bytes)`);
console.log('    Pronta para testes de MBR, FAT32, Detecção 0xE5 e RAW Carving!\n');
