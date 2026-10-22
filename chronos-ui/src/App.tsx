import { useState } from 'react';
import { 
  HardDrive, 
  Clock, 
  ShieldAlert, 
  Play, 
  Pause, 
  FolderTree, 
  Binary, 
  Search, 
  DownloadCloud,
  Activity,
  Cpu,
  CheckCircle2,
  FileText,
  Image as ImageIcon,
  Video,
  Database
} from 'lucide-react';

interface StorageDevice {
  id: string;
  name: string;
  path: string;
  type: string;
  size: string;
  sectorSize: number;
  removable: boolean;
  health: 'GOOD' | 'WARNING' | 'CRITICAL';
  partitions: { label: string; fsType: string; size: string; startLba: number }[];
}

interface DiscoveredItem {
  id: string;
  name: string;
  path: string;
  size: string;
  status: 'ACTIVE' | 'DELETED' | 'ORPHAN' | 'CARVED';
  recoverability: number;
  category: 'image' | 'video' | 'audio' | 'doc' | 'archive' | 'db' | 'other';
  lba: number;
  date: string;
}

interface TimelineRecord {
  id: string;
  timestamp: string;
  eventType: 'FILE_CREATE' | 'FILE_DELETE' | 'FILE_RENAME' | 'DATA_TRUNCATE';
  sourcePath: string;
  targetPath?: string;
  usn: string;
  details: string;
}

export default function App() {
  const [activeTab, setActiveTab] = useState<'scan' | 'files' | 'timeline' | 'hex'>('scan');
  const [isScanning, setIsScanning] = useState(false);
  const [scanMode, setScanMode] = useState<'quick' | 'carve' | 'full'>('full');
  const [scanProgress] = useState(64.2);
  const [selectedDevice, setSelectedDevice] = useState<string>('dev-1');
  const [fileFilter, setFileFilter] = useState('');
  const [selectedCategory, setSelectedCategory] = useState<string>('all');
  const [exportModalOpen, setExportModalOpen] = useState(false);
  const [exportDestination, setExportDestination] = useState('D:\\Chronos_Recovered');

  const devices: StorageDevice[] = [
    {
      id: 'dev-0',
      name: 'Samsung SSD 980 PRO 1TB',
      path: '\\\\.\\PhysicalDrive0',
      type: 'NVMe / Fixed',
      size: '1000.2 GB',
      sectorSize: 4096,
      removable: false,
      health: 'GOOD',
      partitions: [
        { label: 'Windows (C:)', fsType: 'NTFS', size: '950.0 GB', startLba: 2048 },
        { label: 'Recovery Part', fsType: 'NTFS', size: '2.0 GB', startLba: 1945600 }
      ]
    },
    {
      id: 'dev-1',
      name: 'SanDisk Extreme PRO USB 3.2',
      path: '\\\\.\\PhysicalDrive1',
      type: 'Flash / Removable',
      size: '128.0 GB',
      sectorSize: 512,
      removable: true,
      health: 'WARNING',
      partitions: [
        { label: 'EOS_DIGITAL', fsType: 'exFAT', size: '128.0 GB', startLba: 32768 }
      ]
    }
  ];

  const files: DiscoveredItem[] = [
    {
      id: 'f-1',
      name: 'IMG_4802.CR2',
      path: '/DCIM/100CANON/IMG_4802.CR2',
      size: '28.4 MB',
      status: 'DELETED',
      recoverability: 100,
      category: 'image',
      lba: 0x0041F000,
      date: '2026-10-21 14:15:20'
    },
    {
      id: 'f-2',
      name: 'DCIM_0842.JPG',
      path: '/DCIM/100CANON/DCIM_0842.JPG',
      size: '4.82 MB',
      status: 'DELETED',
      recoverability: 96,
      category: 'image',
      lba: 0x0042A800,
      date: '2026-10-21 14:18:02'
    },
    {
      id: 'f-3',
      name: 'Drone_4K_Footage.MP4',
      path: '/MOVIES/Drone_4K_Footage.MP4',
      size: '1.45 GB',
      status: 'ACTIVE',
      recoverability: 100,
      category: 'video',
      lba: 0x011A0000,
      date: '2026-10-20 18:32:11'
    },
    {
      id: 'f-4',
      name: 'carved_008F4200.pdf',
      path: '/[Carved RAW]/008F4200.pdf',
      size: '3.12 MB',
      status: 'CARVED',
      recoverability: 100,
      category: 'doc',
      lba: 0x008F4200,
      date: '2026-10-18 09:22:45'
    },
    {
      id: 'f-5',
      name: 'Database_Backup.sqlite',
      path: '/App/Database_Backup.sqlite',
      size: '42.0 MB',
      status: 'DELETED',
      recoverability: 72,
      category: 'db',
      lba: 0x02100000,
      date: '2026-10-19 22:04:12'
    }
  ];

  const timelineEvents: TimelineRecord[] = [
    {
      id: 'tl-1',
      timestamp: '2026-10-21 14:18:02 UTC',
      eventType: 'FILE_DELETE',
      sourcePath: 'DCIM_0842.JPG',
      usn: '0x000000018FA200B0',
      details: 'Parent MFT #14050, Reason: USN_REASON_FILE_DELETE'
    },
    {
      id: 'tl-2',
      timestamp: '2026-10-21 14:15:20 UTC',
      eventType: 'FILE_DELETE',
      sourcePath: 'IMG_4802.CR2',
      usn: '0x000000018FA1E450',
      details: 'Parent MFT #14049, Reason: USN_REASON_FILE_DELETE'
    },
    {
      id: 'tl-3',
      timestamp: '2026-10-21 11:05:43 UTC',
      eventType: 'FILE_RENAME',
      sourcePath: 'Draft.docx',
      targetPath: 'Relatorio_Auditoria_2026.docx',
      usn: '0x000000018F900120',
      details: 'Parent MFT #8210, Reason: USN_REASON_RENAME_NEW_NAME'
    },
    {
      id: 'tl-4',
      timestamp: '2026-10-21 10:44:19 UTC',
      eventType: 'FILE_CREATE',
      sourcePath: 'Drone_4K_Footage.MP4',
      usn: '0x000000018F8422A0',
      details: 'Parent MFT #9102, Reason: USN_REASON_FILE_CREATE'
    }
  ];

  const activeDeviceObj = devices.find(d => d.id === selectedDevice) || devices[0];

  const filteredFiles = files.filter(f => {
    const matchesQuery = fileFilter === '' || f.name.toLowerCase().includes(fileFilter.toLowerCase()) || f.path.toLowerCase().includes(fileFilter.toLowerCase());
    const matchesCategory = selectedCategory === 'all' || f.category === selectedCategory;
    return matchesQuery && matchesCategory;
  });

  return (
    <div className="flex h-screen w-screen bg-[#07090e] text-slate-200 overflow-hidden select-none font-sans">
      {/* Sidebar Navigation */}
      <aside className="w-64 border-r border-[#1a2333] bg-[#0c1017] flex flex-col justify-between">
        <div>
          {/* Brand Header */}
          <div className="p-5 border-b border-[#1a2333] flex items-center gap-3">
            <div className="p-2 rounded-lg bg-gradient-to-br from-cyan-500 to-blue-600 shadow-lg shadow-cyan-500/20">
              <Cpu className="w-6 h-6 text-white" />
            </div>
            <div>
              <h1 className="text-sm font-bold tracking-wider uppercase text-cyan-400">ChronosRecover</h1>
              <p className="text-[11px] text-slate-400 font-mono">v0.1.0 • Forensic Suite</p>
            </div>
          </div>

          {/* Navigation Items */}
          <nav className="p-3 space-y-1">
            <button
              onClick={() => setActiveTab('scan')}
              className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-xs font-medium transition-all ${
                activeTab === 'scan'
                  ? 'bg-cyan-500/10 text-cyan-400 border border-cyan-500/30'
                  : 'text-slate-400 hover:bg-[#121824] hover:text-slate-200'
              }`}
            >
              <HardDrive className="w-4 h-4" />
              <span>Dispositivos & Varredura</span>
            </button>

            <button
              onClick={() => setActiveTab('files')}
              className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-xs font-medium transition-all ${
                activeTab === 'files'
                  ? 'bg-cyan-500/10 text-cyan-400 border border-cyan-500/30'
                  : 'text-slate-400 hover:bg-[#121824] hover:text-slate-200'
              }`}
            >
              <FolderTree className="w-4 h-4" />
              <div className="flex-1 flex items-center justify-between">
                <span>Arquivos Encontrados</span>
                <span className="text-[10px] px-1.5 py-0.2 rounded bg-[#1a2538] font-mono text-cyan-300">14.8k</span>
              </div>
            </button>

            <button
              onClick={() => setActiveTab('timeline')}
              className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-xs font-medium transition-all ${
                activeTab === 'timeline'
                  ? 'bg-cyan-500/10 text-cyan-400 border border-cyan-500/30'
                  : 'text-slate-400 hover:bg-[#121824] hover:text-slate-200'
              }`}
            >
              <Clock className="w-4 h-4" />
              <span>Timeline Forense ($UsnJrnl)</span>
            </button>

            <button
              onClick={() => setActiveTab('hex')}
              className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-xs font-medium transition-all ${
                activeTab === 'hex'
                  ? 'bg-cyan-500/10 text-cyan-400 border border-cyan-500/30'
                  : 'text-slate-400 hover:bg-[#121824] hover:text-slate-200'
              }`}
            >
              <Binary className="w-4 h-4" />
              <span>Hex / Preview LBA</span>
            </button>
          </nav>
        </div>

        {/* Engine Status Footer */}
        <div className="p-4 border-t border-[#1a2333] bg-[#0a0e14]">
          <div className="flex items-center justify-between text-xs mb-2">
            <span className="text-slate-400 flex items-center gap-1.5">
              <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse"></span>
              chronos-engine IPC
            </span>
            <span className="font-mono text-[10px] text-cyan-400 bg-cyan-950/60 px-1.5 py-0.5 rounded border border-cyan-800/40">ONLINE</span>
          </div>
          <p className="text-[10px] text-slate-500 font-mono truncate">127.0.0.1:23007</p>
        </div>
      </aside>

      {/* Main Content Area */}
      <main className="flex-1 flex flex-col overflow-hidden bg-[#07090e]">
        {/* Top Control Bar */}
        <header className="h-16 border-b border-[#1a2333] px-6 flex items-center justify-between bg-[#090d13]">
          <div className="flex items-center gap-4">
            <div className="text-xs">
              <span className="text-slate-400">Dispositivo Selecionado: </span>
              <span className="font-semibold text-slate-100 font-mono">{activeDeviceObj.path} ({activeDeviceObj.name})</span>
            </div>
            <span className="text-xs px-2 py-0.5 rounded bg-amber-500/10 text-amber-400 border border-amber-500/20 font-mono">
              Raw Physical Access (4KB Aligned)
            </span>
          </div>

          <div className="flex items-center gap-3">
            {/* Scan Mode Toggle */}
            <div className="flex rounded-lg bg-[#121824] p-0.5 border border-[#1e2a3f] text-xs">
              <button 
                onClick={() => setScanMode('quick')}
                className={`px-2.5 py-1 rounded-md transition-all ${scanMode === 'quick' ? 'bg-cyan-600 text-white font-medium' : 'text-slate-400'}`}
              >
                Quick Metadata
              </button>
              <button 
                onClick={() => setScanMode('carve')}
                className={`px-2.5 py-1 rounded-md transition-all ${scanMode === 'carve' ? 'bg-cyan-600 text-white font-medium' : 'text-slate-400'}`}
              >
                RAW Carving
              </button>
              <button 
                onClick={() => setScanMode('full')}
                className={`px-2.5 py-1 rounded-md transition-all ${scanMode === 'full' ? 'bg-cyan-600 text-white font-medium' : 'text-slate-400'}`}
              >
                Full Forensic
              </button>
            </div>

            <button 
              onClick={() => setIsScanning(!isScanning)}
              className={`flex items-center gap-2 px-4 py-2 rounded-lg text-xs font-medium transition-all shadow-md ${
                isScanning 
                  ? 'bg-amber-600 hover:bg-amber-500 text-white shadow-amber-600/20'
                  : 'bg-cyan-600 hover:bg-cyan-500 text-white shadow-cyan-600/20'
              }`}
            >
              {isScanning ? <Pause className="w-3.5 h-3.5" /> : <Play className="w-3.5 h-3.5" />}
              <span>{isScanning ? 'Pausar Varredura' : 'Iniciar Varredura'}</span>
            </button>

            <button 
              onClick={() => setExportModalOpen(true)}
              className="flex items-center gap-2 px-4 py-2 rounded-lg text-xs font-medium bg-[#141b27] hover:bg-[#1c2536] text-slate-300 border border-[#232f46] transition-all"
            >
              <DownloadCloud className="w-3.5 h-3.5 text-cyan-400" />
              <span>Exportação Segura</span>
            </button>
          </div>
        </header>

        {/* Dynamic View Body */}
        <div className="flex-1 overflow-y-auto p-6 space-y-6">
          {/* Real-time Scan Dashboard Banner */}
          <div className="bg-[#0d121c] border border-[#1d273a] rounded-xl p-5 shadow-xl relative overflow-hidden">
            <div className="flex items-center justify-between mb-3">
              <div>
                <span className="text-xs font-semibold uppercase tracking-wider text-cyan-400">Status do Motor</span>
                <h3 className="text-lg font-bold text-slate-100 flex items-center gap-2 mt-0.5">
                  <Activity className="w-4 h-4 text-cyan-400 animate-pulse" />
                  MFT / exFAT Stream + SIMD Aho-Corasick Carving
                </h3>
              </div>
              <div className="text-right">
                <span className="text-2xl font-bold font-mono text-cyan-400">{scanProgress}%</span>
                <p className="text-[11px] text-slate-400 font-mono">118.7 MB/s • ETA: 00:08:14</p>
              </div>
            </div>

            {/* Progress Bar */}
            <div className="w-full bg-[#141a27] rounded-full h-3 overflow-hidden p-0.5 border border-[#232d42]">
              <div 
                className="bg-gradient-to-r from-cyan-500 to-blue-500 h-full rounded-full transition-all duration-300 shadow-sm"
                style={{ width: `${scanProgress}%` }}
              />
            </div>

            {/* Telemetry Stats Bar */}
            <div className="grid grid-cols-4 gap-4 mt-4 pt-4 border-t border-[#182133] text-xs">
              <div>
                <span className="text-slate-400">LBA Atual:</span>
                <p className="font-mono text-slate-200 mt-0.5">0x07A4F200 / 0x0EE00000</p>
              </div>
              <div>
                <span className="text-slate-400">Arquivos Descobertos:</span>
                <p className="font-mono text-emerald-400 mt-0.5 font-bold">14,892 itens</p>
              </div>
              <div>
                <span className="text-slate-400">Bad Sectors (Saltados):</span>
                <p className="font-mono text-rose-400 mt-0.5 font-bold">12 blocos (Adaptive Skip)</p>
              </div>
              <div>
                <span className="text-slate-400">Eventos de Histórico:</span>
                <p className="font-mono text-purple-400 mt-0.5 font-bold">8,340 registros USN</p>
              </div>
            </div>
          </div>

          {/* Device Selection & Partition Map */}
          {activeTab === 'scan' && (
            <div className="space-y-4">
              <h2 className="text-xs font-semibold uppercase tracking-wider text-slate-400">Unidades Físicas Detectadas (Win32 HAL)</h2>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {devices.map((dev) => (
                  <div 
                    key={dev.id}
                    onClick={() => setSelectedDevice(dev.id)}
                    className={`p-5 rounded-xl border transition-all cursor-pointer shadow-md ${
                      selectedDevice === dev.id
                        ? 'border-cyan-500 bg-[#0e1624] ring-1 ring-cyan-500/50'
                        : 'border-[#1e2a3f] bg-[#0c111a] hover:border-slate-600'
                    }`}
                  >
                    <div className="flex items-start justify-between">
                      <div className="flex items-center gap-3">
                        <div className="p-3 rounded-lg bg-[#141d2c] text-cyan-400 border border-[#233148]">
                          <HardDrive className="w-5 h-5" />
                        </div>
                        <div>
                          <h4 className="text-sm font-semibold text-slate-100">{dev.name}</h4>
                          <p className="text-xs font-mono text-slate-400">{dev.path}</p>
                        </div>
                      </div>
                      <span className={`text-[10px] font-mono px-2 py-0.5 rounded font-semibold border ${
                        dev.health === 'GOOD' 
                          ? 'bg-emerald-500/10 text-emerald-400 border-emerald-500/30'
                          : 'bg-amber-500/10 text-amber-400 border-amber-500/30'
                      }`}>
                        {dev.health}
                      </span>
                    </div>

                    <div className="mt-4 pt-3 border-t border-[#172030] grid grid-cols-3 gap-2 text-xs">
                      <div>
                        <span className="text-slate-500 text-[11px]">Capacidade</span>
                        <p className="font-mono text-slate-300 font-medium">{dev.size}</p>
                      </div>
                      <div>
                        <span className="text-slate-500 text-[11px]">Setor Físico</span>
                        <p className="font-mono text-slate-300 font-medium">{dev.sectorSize} B</p>
                      </div>
                      <div>
                        <span className="text-slate-500 text-[11px]">Tipo de Barramento</span>
                        <p className="font-mono text-slate-300 font-medium">{dev.type}</p>
                      </div>
                    </div>

                    {/* Partitions List */}
                    <div className="mt-3 pt-3 border-t border-[#172030] space-y-1">
                      <span className="text-[10px] uppercase font-semibold text-slate-500">Partições Mapeadas:</span>
                      {dev.partitions.map((p, idx) => (
                        <div key={idx} className="flex items-center justify-between text-xs font-mono text-slate-400 bg-[#090e16] px-2 py-1 rounded border border-[#182130]">
                          <span>{p.label} ({p.fsType})</span>
                          <span>{p.size} • LBA {p.startLba}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* File Explorer Tab */}
          {activeTab === 'files' && (
            <div className="border border-[#1d273a] bg-[#0c111a] rounded-xl overflow-hidden">
              <div className="p-3 border-b border-[#1d273a] flex items-center justify-between gap-4">
                <div className="flex items-center gap-2 bg-[#141b27] px-3 py-1.5 rounded-lg border border-[#243149] w-72">
                  <Search className="w-3.5 h-3.5 text-slate-400" />
                  <input 
                    type="text" 
                    value={fileFilter}
                    onChange={(e) => setFileFilter(e.target.value)}
                    placeholder="Filtrar por nome ou extensão..." 
                    className="bg-transparent text-xs text-slate-200 outline-none w-full"
                  />
                </div>

                {/* Category Filter Pills */}
                <div className="flex items-center gap-1.5 text-xs">
                  {['all', 'image', 'video', 'doc', 'db'].map((cat) => (
                    <button
                      key={cat}
                      onClick={() => setSelectedCategory(cat)}
                      className={`px-2.5 py-1 rounded-md text-[11px] font-medium transition-all ${
                        selectedCategory === cat
                          ? 'bg-cyan-500/20 text-cyan-300 border border-cyan-500/40'
                          : 'text-slate-400 hover:bg-[#141b27]'
                      }`}
                    >
                      {cat.toUpperCase()}
                    </button>
                  ))}
                </div>

                <div className="flex items-center gap-2 text-xs">
                  <span className="text-slate-400">Total Encontrado:</span>
                  <span className="font-mono font-bold text-cyan-400">{filteredFiles.length}</span>
                </div>
              </div>

              <table className="w-full text-left text-xs font-mono">
                <thead className="bg-[#101622] text-slate-400 border-b border-[#1d273a]">
                  <tr>
                    <th className="p-3">Nome do Arquivo</th>
                    <th className="p-3">Status</th>
                    <th className="p-3">Tamanho</th>
                    <th className="p-3">Recoverability</th>
                    <th className="p-3">LBA Inicial</th>
                    <th className="p-3">Data / Hora</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-[#172030] text-slate-300">
                  {filteredFiles.map((file) => (
                    <tr key={file.id} className="hover:bg-[#131b29] transition-all">
                      <td className="p-3 font-sans font-medium text-slate-100 flex items-center gap-2">
                        {file.category === 'image' && <ImageIcon className="w-4 h-4 text-cyan-400" />}
                        {file.category === 'video' && <Video className="w-4 h-4 text-blue-400" />}
                        {file.category === 'doc' && <FileText className="w-4 h-4 text-purple-400" />}
                        {file.category === 'db' && <Database className="w-4 h-4 text-emerald-400" />}
                        <div>
                          <div>{file.name}</div>
                          <div className="text-[10px] text-slate-500 font-mono truncate max-w-xs">{file.path}</div>
                        </div>
                      </td>
                      <td className="p-3">
                        <span className={`px-2 py-0.5 rounded text-[10px] border font-bold ${
                          file.status === 'DELETED' 
                            ? 'bg-rose-500/10 text-rose-400 border-rose-500/20'
                            : file.status === 'CARVED'
                            ? 'bg-purple-500/10 text-purple-400 border-purple-500/20'
                            : 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20'
                        }`}>
                          {file.status}
                        </span>
                      </td>
                      <td className="p-3">{file.size}</td>
                      <td className="p-3">
                        <div className="flex items-center gap-2">
                          <div className="w-16 bg-[#1a2333] h-2 rounded-full overflow-hidden">
                            <div 
                              className={`h-full rounded-full ${
                                file.recoverability >= 90 ? 'bg-emerald-400' : file.recoverability >= 70 ? 'bg-amber-400' : 'bg-rose-400'
                              }`} 
                              style={{ width: `${file.recoverability}%` }}
                            />
                          </div>
                          <span className={`font-bold text-[11px] ${
                            file.recoverability >= 90 ? 'text-emerald-400' : 'text-amber-400'
                          }`}>
                            {file.recoverability}%
                          </span>
                        </div>
                      </td>
                      <td className="p-3 text-cyan-400">0x{file.lba.toString(16).toUpperCase()}</td>
                      <td className="p-3 text-slate-400">{file.date}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {/* Forensic Timeline View */}
          {activeTab === 'timeline' && (
            <div className="border border-[#1d273a] bg-[#0c111a] rounded-xl p-5 space-y-4">
              <div className="flex items-center justify-between border-b border-[#1a2333] pb-3">
                <div className="flex items-center gap-2 text-purple-400">
                  <Clock className="w-4 h-4" />
                  <h3 className="text-sm font-bold uppercase tracking-wider">Histórico de Transações do Dispositivo ($UsnJrnl / $LogFile)</h3>
                </div>
                <span className="text-xs text-slate-500 font-mono">Total de Eventos: 8,340</span>
              </div>

              <div className="space-y-3 font-mono text-xs">
                {timelineEvents.map((ev) => (
                  <div key={ev.id} className="p-3 rounded-lg bg-[#111724] border border-[#1b2538] flex items-center justify-between">
                    <div>
                      <span className={`px-2 py-0.5 rounded text-[10px] mr-2 border font-bold ${
                        ev.eventType === 'FILE_DELETE' ? 'bg-rose-500/10 text-rose-400 border-rose-500/20' :
                        ev.eventType === 'FILE_RENAME' ? 'bg-blue-500/10 text-blue-400 border-blue-500/20' :
                        'bg-emerald-500/10 text-emerald-400 border-emerald-500/20'
                      }`}>
                        {ev.eventType}
                      </span>
                      <span className="text-slate-200 font-medium">{ev.sourcePath}</span>
                      {ev.targetPath && (
                        <>
                          <span className="text-slate-500 mx-2">→</span>
                          <span className="text-cyan-300 font-medium">{ev.targetPath}</span>
                        </>
                      )}
                      <span className="text-slate-500 ml-2 font-mono">({ev.usn})</span>
                    </div>
                    <span className="text-slate-400">{ev.timestamp}</span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Hex Viewer */}
          {activeTab === 'hex' && (
            <div className="border border-[#1d273a] bg-[#0c111a] rounded-xl p-5 font-mono text-xs">
              <div className="flex items-center justify-between pb-3 border-b border-[#1b2538] mb-4">
                <div className="flex items-center gap-2 text-cyan-400">
                  <Binary className="w-4 h-4" />
                  <span>Hex Raw Viewer: LBA 0x0042A800 (Carved JPEG Header - SOI FF D8 FF E0)</span>
                </div>
                <span className="text-[11px] text-slate-500">Bytes per row: 16 • Alignment: 512B</span>
              </div>
              <pre className="text-slate-300 leading-relaxed overflow-x-auto p-4 bg-[#080c13] rounded-lg border border-[#162031]">
{`0042A800  FF D8 FF E0 00 10 4A 46  49 46 00 01 01 01 00 48  |......JFIF.....H|
0042A810  00 48 00 00 FF DB 00 43  00 08 06 06 07 06 05 08  |.H.....C........|
0042A820  07 07 07 09 09 08 0A 0C  14 0D 0C 0B 0B 0C 19 12  |................|
0042A830  13 0F 14 1D 1A 1F 1E 1D  1A 1C 1C 20 24 2E 27 20  |........... $.' |
0042A840  22 2C 23 1C 1C 28 37 29  2C 30 31 34 34 34 1F 27  |",#..(7),01444.'|
0042A850  39 3D 38 32 3C 2E 33 34  32 FF C0 00 11 08 0C 00  |9=82<.342.......|`}
              </pre>
            </div>
          )}
        </div>
      </main>

      {/* Safe Exporter Modal */}
      {exportModalOpen && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center p-4 z-50">
          <div className="bg-[#0c111a] border border-[#223049] rounded-2xl w-full max-w-lg p-6 shadow-2xl space-y-4">
            <div className="flex items-center gap-3">
              <div className="p-2.5 rounded-xl bg-cyan-500/10 text-cyan-400 border border-cyan-500/30">
                <ShieldAlert className="w-6 h-6" />
              </div>
              <div>
                <h3 className="text-base font-bold text-slate-100">Exportador Seguro de Recuperação</h3>
                <p className="text-xs text-slate-400">Verificação de disco físico via IOCTL_VOLUME_DISK_EXTENTS</p>
              </div>
            </div>

            <div className="p-3.5 rounded-lg bg-emerald-500/10 border border-emerald-500/30 text-xs text-emerald-300 flex items-start gap-2.5">
              <CheckCircle2 className="w-5 h-5 flex-shrink-0 text-emerald-400 mt-0.5" />
              <div>
                <p className="font-semibold">Destino Validado e Seguro</p>
                <p className="text-[11px] text-emerald-400/80 mt-0.5">
                  A unidade de destino (D:\) reside no Disco Físico #0, enquanto a unidade de origem é o Disco Físico #1. Não há risco de sobregravação.
                </p>
              </div>
            </div>

            <div className="space-y-1.5 text-xs">
              <label className="text-slate-400">Caminho da Pasta de Destino:</label>
              <input 
                type="text" 
                value={exportDestination}
                onChange={(e) => setExportDestination(e.target.value)}
                className="w-full bg-[#131a27] border border-[#223149] rounded-lg px-3 py-2 text-slate-200 font-mono text-xs outline-none focus:border-cyan-500"
              />
            </div>

            <div className="flex items-center justify-end gap-3 pt-3 border-t border-[#1a2333]">
              <button 
                onClick={() => setExportModalOpen(false)}
                className="px-4 py-2 rounded-lg text-xs font-medium text-slate-400 hover:bg-[#131a26]"
              >
                Cancelar
              </button>
              <button 
                onClick={() => setExportModalOpen(false)}
                className="px-4 py-2 rounded-lg text-xs font-medium bg-cyan-600 hover:bg-cyan-500 text-white shadow-lg shadow-cyan-600/20"
              >
                Confirmar e Iniciar Extração
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
