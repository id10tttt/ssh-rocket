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

export function FloatingHUD() {
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
      className="floating-hud"
    >
      <div className="floating-identity">
        <div className={statusText === '已连接' ? 'floating-logo is-connected' : 'floating-logo'}>
          <Rocket />
        </div>
        <div className="floating-copy">
          <strong>SSH Rocket</strong>
          <span>{statusText}</span>
        </div>
      </div>

      <div className="floating-speeds">
        <div className="download">
          <ArrowDown />
          <span>{formatBytes(speed.download)}/s</span>
        </div>
        <div className="upload">
          <ArrowUp />
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
