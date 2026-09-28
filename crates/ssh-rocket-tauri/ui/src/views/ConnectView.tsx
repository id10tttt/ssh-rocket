import React, { useState } from 'react';
import { 
  Server, 
  Key, 
  Power, 
  Eye, 
  EyeOff, 
  Globe, 
  Radio, 
  CheckCircle2, 
  AlertCircle 
} from 'lucide-react';
import { AppConfig, Profile } from '../types';

interface ConnectViewProps {
  config: AppConfig | null;
  isRunning: boolean;
  statusText: string;
  onStart: (profileId?: string) => void;
  onStop: () => void;
  onSaveConfig: (cfg: AppConfig) => void;
}

export const ConnectView: React.FC<ConnectViewProps> = ({
  config,
  isRunning,
  statusText,
  onStart,
  onStop,
  onSaveConfig,
}) => {
  const [showPassword, setShowPassword] = useState(false);
  const activeProfile = config?.profiles.find(p => p.id === config.active_profile_id) || config?.profiles[0];

  const handleProfileFieldChange = (field: keyof Profile, value: any) => {
    if (!config || !activeProfile) return;
    const updatedProfiles = config.profiles.map(p => {
      if (p.id === activeProfile.id) {
        return { ...p, [field]: value };
      }
      return p;
    });
    onSaveConfig({ ...config, profiles: updatedProfiles });
  };

  const handleAuthChange = (type: 'password' | 'private_key', value: string) => {
    if (!config || !activeProfile) return;
    const auth_method = type === 'password' ? { Password: value } : { PrivateKey: { path: value, passphrase: null } };
    handleProfileFieldChange('auth_method', auth_method);
  };

  const getPasswordValue = () => {
    if (activeProfile?.auth_method && 'Password' in activeProfile.auth_method) {
      return activeProfile.auth_method.Password;
    }
    return '';
  };

  const getPrivateKeyPath = () => {
    if (activeProfile?.auth_method && 'PrivateKey' in activeProfile.auth_method) {
      return activeProfile.auth_method.PrivateKey.path;
    }
    return '';
  };

  const isPrivateKey = activeProfile?.auth_method && 'PrivateKey' in activeProfile.auth_method;

  return (
    <div className="space-y-6 max-w-4xl mx-auto p-2">
      {/* Top Status & Connect Action Hero */}
      <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-6 flex items-center justify-between shadow-xl backdrop-blur-sm">
        <div className="flex items-center gap-4">
          <div className={`w-14 h-14 rounded-2xl flex items-center justify-center border transition-all duration-300 ${
            isRunning 
              ? 'bg-emerald-500/10 border-emerald-500/30 text-emerald-400' 
              : 'bg-slate-800/60 border-slate-700/60 text-slate-400'
          }`}>
            <Radio className={`w-7 h-7 ${isRunning ? 'animate-pulse' : ''}`} />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className={`inline-block w-2.5 h-2.5 rounded-full ${isRunning ? 'bg-emerald-500' : 'bg-slate-500'}`} />
              <h2 className="text-lg font-semibold text-white">{statusText || (isRunning ? '已连接' : '未连接')}</h2>
            </div>
            <p className="text-xs text-slate-400 mt-1">
              {isRunning 
                ? `已接管系统网络流量 -> ${activeProfile?.host || '远程代理'}` 
                : '点击右侧按钮启动透明代理加速与流量分流'}
            </p>
          </div>
        </div>

        <div>
          {isRunning ? (
            <button
              onClick={onStop}
              className="flex items-center gap-2 px-6 py-3 bg-rose-600 hover:bg-rose-500 text-white font-medium rounded-xl shadow-lg shadow-rose-900/30 active:scale-95 transition-all"
            >
              <Power className="w-5 h-5" />
              <span>断开连接</span>
            </button>
          ) : (
            <button
              onClick={() => onStart(activeProfile?.id)}
              className="flex items-center gap-2 px-6 py-3 bg-blue-600 hover:bg-blue-500 text-white font-medium rounded-xl shadow-lg shadow-blue-900/30 active:scale-95 transition-all"
            >
              <Power className="w-5 h-5" />
              <span>启动代理</span>
            </button>
          )}
        </div>
      </div>

      {/* Dual Column Configuration Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Card 1: Remote SSH Node */}
        <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-5 shadow-lg backdrop-blur-sm">
          <div className="flex items-center gap-2.5 mb-4 pb-3 border-b border-slate-800/80">
            <Server className="w-5 h-5 text-blue-400" />
            <h3 className="text-sm font-semibold text-white">远程 SSH 节点</h3>
          </div>

          <div className="space-y-4 text-xs">
            <div>
              <label className="block text-slate-400 mb-1.5 font-medium">配置名称</label>
              <input
                type="text"
                disabled={isRunning}
                value={activeProfile?.name || ''}
                onChange={e => handleProfileFieldChange('name', e.target.value)}
                placeholder="例如: 香港高速节点"
                className="w-full bg-slate-950/70 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500 disabled:opacity-50"
              />
            </div>

            <div className="grid grid-cols-3 gap-3">
              <div className="col-span-2">
                <label className="block text-slate-400 mb-1.5 font-medium">服务器地址 (Host)</label>
                <input
                  type="text"
                  disabled={isRunning}
                  value={activeProfile?.host || ''}
                  onChange={e => handleProfileFieldChange('host', e.target.value)}
                  placeholder="1.2.3.4 或域名"
                  className="w-full bg-slate-950/70 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500 disabled:opacity-50"
                />
              </div>
              <div>
                <label className="block text-slate-400 mb-1.5 font-medium">SSH 端口</label>
                <input
                  type="number"
                  disabled={isRunning}
                  value={activeProfile?.port || 22}
                  onChange={e => handleProfileFieldChange('port', parseInt(e.target.value) || 22)}
                  className="w-full bg-slate-950/70 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500 disabled:opacity-50"
                />
              </div>
            </div>

            <div>
              <label className="block text-slate-400 mb-1.5 font-medium">登录用户名</label>
              <input
                type="text"
                disabled={isRunning}
                value={activeProfile?.user || ''}
                onChange={e => handleProfileFieldChange('user', e.target.value)}
                placeholder="root"
                className="w-full bg-slate-950/70 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500 disabled:opacity-50"
              />
            </div>

            <div>
              <div className="flex items-center justify-between mb-1.5">
                <label className="text-slate-400 font-medium">认证方式</label>
                <div className="flex gap-2">
                  <button
                    disabled={isRunning}
                    onClick={() => handleAuthChange('password', '')}
                    className={`px-2 py-0.5 rounded text-[11px] font-medium transition-all ${
                      !isPrivateKey ? 'bg-blue-600 text-white' : 'bg-slate-800 text-slate-400'
                    }`}
                  >
                    密码
                  </button>
                  <button
                    disabled={isRunning}
                    onClick={() => handleAuthChange('private_key', '~/.ssh/id_rsa')}
                    className={`px-2 py-0.5 rounded text-[11px] font-medium transition-all ${
                      isPrivateKey ? 'bg-blue-600 text-white' : 'bg-slate-800 text-slate-400'
                    }`}
                  >
                    私钥文件
                  </button>
                </div>
              </div>

              {!isPrivateKey ? (
                <div className="relative">
                  <input
                    type={showPassword ? 'text' : 'password'}
                    disabled={isRunning}
                    value={getPasswordValue()}
                    onChange={e => handleAuthChange('password', e.target.value)}
                    placeholder="请输入 SSH 密码"
                    className="w-full bg-slate-950/70 border border-slate-800 rounded-lg pl-3 pr-9 py-2 text-white focus:outline-none focus:border-blue-500 disabled:opacity-50"
                  />
                  <button
                    type="button"
                    onClick={() => setShowPassword(!showPassword)}
                    className="absolute right-2.5 top-2 text-slate-400 hover:text-slate-200"
                  >
                    {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
                  </button>
                </div>
              ) : (
                <input
                  type="text"
                  disabled={isRunning}
                  value={getPrivateKeyPath()}
                  onChange={e => handleAuthChange('private_key', e.target.value)}
                  placeholder="私钥路径, 如 /home/user/.ssh/id_ed25519"
                  className="w-full bg-slate-950/70 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500 disabled:opacity-50"
                />
              )}
            </div>
          </div>
        </div>

        {/* Card 2: Local Socks & DNS */}
        <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-5 shadow-lg backdrop-blur-sm flex flex-col justify-between">
          <div>
            <div className="flex items-center gap-2.5 mb-4 pb-3 border-b border-slate-800/80">
              <Globe className="w-5 h-5 text-emerald-400" />
              <h3 className="text-sm font-semibold text-white">本地监听与分流设置</h3>
            </div>

            <div className="space-y-4 text-xs">
              <div>
                <label className="block text-slate-400 mb-1.5 font-medium">本地 Socks5 代理端口</label>
                <div className="bg-slate-950/70 border border-slate-800 rounded-lg px-3 py-2 text-slate-300 font-mono">
                  127.0.0.1:17880
                </div>
                <p className="text-[11px] text-slate-500 mt-1">由系统守护核心自动管理监听</p>
              </div>

              <div>
                <label className="block text-slate-400 mb-1.5 font-medium">远端 DNS 解析服务器</label>
                <input
                  type="text"
                  disabled={isRunning}
                  value={config?.settings.dns_server || '8.8.8.8'}
                  onChange={e => {
                    if (config) {
                      onSaveConfig({
                        ...config,
                        settings: { ...config.settings, dns_server: e.target.value }
                      });
                    }
                  }}
                  className="w-full bg-slate-950/70 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500 disabled:opacity-50"
                />
              </div>

              <div className="pt-2">
                <div className="bg-slate-950/40 border border-slate-800/60 rounded-xl p-3 space-y-2">
                  <div className="flex items-center gap-2 text-slate-300">
                    <CheckCircle2 className="w-4 h-4 text-emerald-400" />
                    <span>透明代理与 TUN 模式</span>
                  </div>
                  <p className="text-[11px] text-slate-500">
                    通过特权 Helper 会话接管系统网络路由，实现全局应用与域名的毫秒级分流。
                  </p>
                </div>
              </div>
            </div>
          </div>

          <div className="text-[11px] text-slate-500 text-center py-2">
            配置修改将实时保存在本地磁盘
          </div>
        </div>
      </div>
    </div>
  );
};
