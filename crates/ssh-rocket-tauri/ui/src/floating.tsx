import React, { useEffect, useState } from 'react';
import ReactDOM from 'react-dom/client';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { ArrowDown, ArrowUp, Rocket } from 'lucide-react';
import './index.css';

interface SpeedDto {
  upload: number;
  download: number;
}

function FloatingHUD() {
  const [speed, setSpeed] = useState<SpeedDto>({ upload: 0, download: 0 });
  const [statusText, setStatusText] = useState('未连接');

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };

  useEffect(() => {
    const unlistenSpeed = listen<SpeedDto>('speed', (e) => setSpeed(e.payload));
    const unlistenStatus = listen<string>('status-changed', (e) => setStatusText(e.payload));

    return () => {
      unlistenSpeed.then(f => f());
      unlistenStatus.then(f => f());
    };
  }, []);

  const handleMouseDown = () => {
    const win = getCurrentWebviewWindow();
    win.startDragging();
  };

  return (
    <div
      onMouseDown={handleMouseDown}
      className="w-full h-full bg-slate-900/90 border border-slate-700/80 rounded-2xl p-2.5 flex items-center justify-between text-white backdrop-blur-md shadow-2xl cursor-move select-none"
    >
      <div className="flex items-center gap-2">
        <div className="w-8 h-8 rounded-xl bg-blue-600/20 border border-blue-500/30 flex items-center justify-center text-blue-400">
          <Rocket className="w-4 h-4" />
        </div>
        <div className="flex flex-col">
          <span className="text-[10px] text-slate-400 font-medium">SSH Rocket</span>
          <span className="text-[10px] text-emerald-400 font-mono truncate max-w-[65px]">{statusText}</span>
        </div>
      </div>

      <div className="flex flex-col items-end font-mono text-[11px] leading-tight space-y-0.5">
        <div className="flex items-center gap-1 text-emerald-400">
          <ArrowDown className="w-3 h-3" />
          <span>{formatBytes(speed.download)}/s</span>
        </div>
        <div className="flex items-center gap-1 text-blue-400">
          <ArrowUp className="w-3 h-3" />
          <span>{formatBytes(speed.upload)}/s</span>
        </div>
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <FloatingHUD />
  </React.StrictMode>
);
