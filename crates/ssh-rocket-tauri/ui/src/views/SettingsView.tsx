import React from 'react';
import { Settings as SettingsIcon, Shield, Laptop, Network } from 'lucide-react';
import { AppConfig } from '../types';

interface SettingsViewProps {
  config: AppConfig | null;
  onSaveConfig: (cfg: AppConfig) => void;
}

export const SettingsView: React.FC<SettingsViewProps> = ({ config, onSaveConfig }) => {
  const handleToggle = (key: 'auto_start' | 'system_proxy') => {
    if (!config) return;
    onSaveConfig({
      ...config,
      settings: {
        ...config.settings,
        [key]: !config.settings[key],
      },
    });
  };

  const handleDnsChange = (val: string) => {
    if (!config) return;
    onSaveConfig({
      ...config,
      settings: {
        ...config.settings,
        dns_server: val,
      },
    });
  };

  return (
    <div className="space-y-6 max-w-3xl mx-auto p-2">
      <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-6 shadow-lg backdrop-blur-sm space-y-6">
        <div className="flex items-center gap-2.5 pb-4 border-b border-slate-800">
          <SettingsIcon className="w-5 h-5 text-blue-400" />
          <h3 className="text-sm font-semibold text-white">常规与系统偏好</h3>
        </div>

        {/* Setting Items */}
        <div className="space-y-4">
          <div className="flex items-center justify-between py-2">
            <div>
              <h4 className="text-xs font-semibold text-white">开机自动启动</h4>
              <p className="text-[11px] text-slate-400 mt-0.5">登录桌面系统后在后台自动静默启动服务</p>
            </div>
            <button
              onClick={() => handleToggle('auto_start')}
              className={`w-12 h-6 rounded-full transition-colors relative ${
                config?.settings.auto_start ? 'bg-blue-600' : 'bg-slate-800'
              }`}
            >
              <div
                className={`w-4 h-4 rounded-full bg-white transition-transform absolute top-1 ${
                  config?.settings.auto_start ? 'left-7' : 'left-1'
                }`}
              />
            </button>
          </div>

          <div className="flex items-center justify-between py-2 border-t border-slate-800/60">
            <div>
              <h4 className="text-xs font-semibold text-white">系统全局代理联动</h4>
              <p className="text-[11px] text-slate-400 mt-0.5">连接成功后自动配置系统网络代理环境 (GNOME / macOS)</p>
            </div>
            <button
              onClick={() => handleToggle('system_proxy')}
              className={`w-12 h-6 rounded-full transition-colors relative ${
                config?.settings.system_proxy ? 'bg-blue-600' : 'bg-slate-800'
              }`}
            >
              <div
                className={`w-4 h-4 rounded-full bg-white transition-transform absolute top-1 ${
                  config?.settings.system_proxy ? 'left-7' : 'left-1'
                }`}
              />
            </button>
          </div>

          <div className="py-2 border-t border-slate-800/60">
            <h4 className="text-xs font-semibold text-white mb-1">远端 DNS 解析服务器</h4>
            <p className="text-[11px] text-slate-400 mb-2">防止本地 DNS 污染，默认采用 Google Public DNS</p>
            <input
              type="text"
              value={config?.settings.dns_server || '8.8.8.8'}
              onChange={e => handleDnsChange(e.target.value)}
              className="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-xs text-white focus:outline-none focus:border-blue-500 font-mono"
            />
          </div>
        </div>
      </div>

      <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-6 shadow-lg backdrop-blur-sm space-y-3">
        <h4 className="text-xs font-semibold text-white">关于 SSH Rocket (Tauri 2.0)</h4>
        <p className="text-xs text-slate-400 leading-relaxed">
          基于 Rust 原生内核驱动的跨平台 SSH 透明代理工具，专为 Linux (Wayland) 与 macOS 打造。
        </p>
        <div className="text-[11px] text-slate-500 font-mono pt-2">
          v2.0.0 • GPL-3.0-or-later
        </div>
      </div>
    </div>
  );
};
