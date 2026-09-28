import React, { useEffect, useRef, useState } from 'react';
import { 
  Activity, 
  ArrowUp, 
  ArrowDown, 
  Network, 
  Layers, 
  Radio 
} from 'lucide-react';
import { AppTrafficStat, ActiveConnectionStat, SpeedDto } from '../types';

interface TrafficViewProps {
  speed: SpeedDto;
  appTraffic: AppTrafficStat[];
  activeConnections: ActiveConnectionStat[];
  isRunning: boolean;
}

export const TrafficView: React.FC<TrafficViewProps> = ({
  speed,
  appTraffic,
  activeConnections,
  isRunning
}) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [history, setHistory] = useState<{ up: number[]; down: number[] }>({
    up: new Array(40).fill(0),
    down: new Array(40).fill(0),
  });

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  };

  const formatSpeed = (bytesPerSec: number) => {
    return `${formatBytes(bytesPerSec)}/s`;
  };

  // Update speed history
  useEffect(() => {
    setHistory(prev => ({
      up: [...prev.up.slice(1), speed.upload],
      down: [...prev.down.slice(1), speed.download],
    }));
  }, [speed]);

  // Render canvas curve
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const width = canvas.width;
    const height = canvas.height;
    ctx.clearRect(0, 0, width, height);

    const maxVal = Math.max(
      ...history.up,
      ...history.down,
      1024 * 100 // minimum 100 KB/s scale
    );

    const drawCurve = (data: number[], strokeColor: string, fillColor: string) => {
      ctx.beginPath();
      const step = width / (data.length - 1);
      data.forEach((val, i) => {
        const x = i * step;
        const y = height - (val / maxVal) * (height - 20) - 10;
        if (i === 0) {
          ctx.moveTo(x, y);
        } else {
          const prevX = (i - 1) * step;
          const prevY = height - (data[i - 1] / maxVal) * (height - 20) - 10;
          const cx = (prevX + x) / 2;
          ctx.bezierCurveTo(cx, prevY, cx, y, x, y);
        }
      });
      ctx.strokeStyle = strokeColor;
      ctx.lineWidth = 2;
      ctx.stroke();

      // Fill area
      ctx.lineTo(width, height);
      ctx.lineTo(0, height);
      ctx.closePath();
      ctx.fillStyle = fillColor;
      ctx.fill();
    };

    // Draw download (blue/emerald)
    drawCurve(history.down, '#10b981', 'rgba(16, 185, 129, 0.12)');
    // Draw upload (amber/blue)
    drawCurve(history.up, '#3b82f6', 'rgba(59, 130, 246, 0.12)');
  }, [history]);

  return (
    <div className="h-full flex flex-col space-y-4 max-w-5xl mx-auto p-2">
      {/* Top Speed Stat Cards */}
      <div className="grid grid-cols-2 gap-4">
        <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-4 flex items-center justify-between shadow-lg backdrop-blur-sm">
          <div className="flex items-center gap-3.5">
            <div className="w-11 h-11 rounded-xl bg-emerald-500/10 border border-emerald-500/20 flex items-center justify-center text-emerald-400">
              <ArrowDown className="w-5 h-5" />
            </div>
            <div>
              <span className="text-[11px] font-medium text-slate-400">实时下载速率</span>
              <h3 className="text-xl font-bold text-white font-mono mt-0.5">
                {formatSpeed(speed.download)}
              </h3>
            </div>
          </div>
        </div>

        <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-4 flex items-center justify-between shadow-lg backdrop-blur-sm">
          <div className="flex items-center gap-3.5">
            <div className="w-11 h-11 rounded-xl bg-blue-500/10 border border-blue-500/20 flex items-center justify-center text-blue-400">
              <ArrowUp className="w-5 h-5" />
            </div>
            <div>
              <span className="text-[11px] font-medium text-slate-400">实时上传速率</span>
              <h3 className="text-xl font-bold text-white font-mono mt-0.5">
                {formatSpeed(speed.upload)}
              </h3>
            </div>
          </div>
        </div>
      </div>

      {/* Speed Chart Card */}
      <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-4 shadow-lg backdrop-blur-sm flex flex-col">
        <div className="flex items-center justify-between mb-2">
          <div className="flex items-center gap-2">
            <Activity className="w-4 h-4 text-blue-400" />
            <h4 className="text-xs font-semibold text-white">速率波动曲线 (实时)</h4>
          </div>
          <div className="flex items-center gap-4 text-[11px] font-mono">
            <div className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 rounded-full bg-emerald-500" />
              <span className="text-slate-400">下载</span>
            </div>
            <div className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 rounded-full bg-blue-500" />
              <span className="text-slate-400">上传</span>
            </div>
          </div>
        </div>

        <div className="w-full h-32 relative bg-slate-950/40 rounded-xl overflow-hidden border border-slate-800/60">
          <canvas
            ref={canvasRef}
            width={880}
            height={128}
            className="w-full h-full block"
          />
        </div>
      </div>

      {/* Active Connections & App Traffic Split */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 flex-1 min-h-0">
        {/* App Traffic Table */}
        <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-4 shadow-lg backdrop-blur-sm flex flex-col min-h-0">
          <div className="flex items-center gap-2 mb-3 pb-2 border-b border-slate-800">
            <Layers className="w-4 h-4 text-indigo-400" />
            <h4 className="text-xs font-semibold text-white">应用流量排行</h4>
          </div>
          <div className="overflow-y-auto flex-1 divide-y divide-slate-800/50">
            {appTraffic.length === 0 ? (
              <div className="h-full flex items-center justify-center text-xs text-slate-500">
                暂无应用流量数据
              </div>
            ) : (
              appTraffic.map((app) => (
                <div key={app.id} className="flex items-center justify-between py-2 text-xs">
                  <div className="flex items-center gap-2.5">
                    <div className="w-7 h-7 rounded-lg bg-slate-800 flex items-center justify-center text-[11px] font-bold text-slate-300">
                      {app.name.charAt(0).toUpperCase()}
                    </div>
                    <div>
                      <div className="font-medium text-white truncate max-w-[130px]">{app.name}</div>
                      <div className="text-[10px] text-slate-500 font-mono">{app.id}</div>
                    </div>
                  </div>
                  <div className="text-right font-mono text-[11px]">
                    <div className="text-emerald-400">↓ {formatBytes(app.download)}</div>
                    <div className="text-blue-400">↑ {formatBytes(app.upload)}</div>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>

        {/* Active Connections Table */}
        <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-4 shadow-lg backdrop-blur-sm flex flex-col min-h-0">
          <div className="flex items-center gap-2 mb-3 pb-2 border-b border-slate-800">
            <Network className="w-4 h-4 text-emerald-400" />
            <h4 className="text-xs font-semibold text-white">活跃连接监控 ({activeConnections.length})</h4>
          </div>
          <div className="overflow-y-auto flex-1 divide-y divide-slate-800/50">
            {activeConnections.length === 0 ? (
              <div className="h-full flex items-center justify-center text-xs text-slate-500">
                暂无活跃连接
              </div>
            ) : (
              activeConnections.map((conn, idx) => (
                <div key={idx} className="flex items-center justify-between py-2 text-xs">
                  <div className="truncate max-w-[200px]">
                    <div className="flex items-center gap-2">
                      <span className={`px-1.5 py-0.2 rounded text-[10px] font-medium ${
                        conn.conn_type === 'Proxy' ? 'bg-blue-500/20 text-blue-400' :
                        conn.conn_type === 'Direct' ? 'bg-emerald-500/20 text-emerald-400' : 'bg-slate-700 text-slate-300'
                      }`}>
                        {conn.conn_type}
                      </span>
                      <span className="font-medium text-white">{conn.proc_name}</span>
                    </div>
                    <div className="text-[10px] text-slate-500 font-mono truncate mt-0.5">
                      {conn.peer_addr}
                    </div>
                  </div>
                  <div className="text-right font-mono text-[11px] text-slate-400">
                    <div>↓ {formatBytes(conn.download)}</div>
                    <div>↑ {formatBytes(conn.upload)}</div>
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
