import React from 'react';
import { 
  Rocket, 
  ShieldCheck, 
  Activity, 
  ScrollText, 
  Settings as SettingsIcon,
  AppWindow
} from 'lucide-react';

export type NavTab = 'connect' | 'rules' | 'traffic' | 'logs' | 'settings';

interface SidebarProps {
  activeTab: NavTab;
  onTabChange: (tab: NavTab) => void;
  isRunning: boolean;
  onToggleHud: () => void;
}

export const Sidebar: React.FC<SidebarProps> = ({
  activeTab,
  onTabChange,
  isRunning,
  onToggleHud
}) => {
  const navItems = [
    { id: 'connect', label: '连接配置', icon: Rocket },
    { id: 'rules', label: '分流规则', icon: ShieldCheck },
    { id: 'traffic', label: '实时流量', icon: Activity },
    { id: 'logs', label: '运行日志', icon: ScrollText },
    { id: 'settings', label: '系统设置', icon: SettingsIcon },
  ];

  return (
    <aside className="w-56 h-screen bg-slate-900 border-r border-slate-800 flex flex-col justify-between p-3 select-none">
      <div>
        {/* App Title & Brand */}
        <div className="flex items-center gap-3 px-3 py-4 mb-4 border-b border-slate-800">
          <div className="w-9 h-9 rounded-xl bg-blue-600/20 border border-blue-500/30 flex items-center justify-center text-blue-400">
            <Rocket className="w-5 h-5" />
          </div>
          <div>
            <h1 className="text-sm font-semibold text-white tracking-wide">SSH Rocket</h1>
            <div className="flex items-center gap-1.5 mt-0.5">
              <span className={`w-2 h-2 rounded-full ${isRunning ? 'bg-emerald-500 animate-pulse' : 'bg-slate-500'}`} />
              <span className="text-xs text-slate-400">{isRunning ? '运行中' : '未连接'}</span>
            </div>
          </div>
        </div>

        {/* Navigation Links */}
        <nav className="space-y-1">
          {navItems.map((item) => {
            const Icon = item.icon;
            const active = activeTab === item.id;
            return (
              <button
                key={item.id}
                onClick={() => onTabChange(item.id as NavTab)}
                className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all duration-150 ${
                  active
                    ? 'bg-blue-600/20 text-blue-400 border border-blue-500/30'
                    : 'text-slate-400 hover:text-slate-200 hover:bg-slate-800/60'
                }`}
              >
                <Icon className={`w-4 h-4 ${active ? 'text-blue-400' : 'text-slate-400'}`} />
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>
      </div>

      {/* Bottom Floating HUD Toggle */}
      <div className="border-t border-slate-800 pt-3">
        <button
          onClick={onToggleHud}
          className="w-full flex items-center gap-2.5 px-3 py-2 text-xs font-medium text-slate-400 hover:text-slate-200 hover:bg-slate-800/60 rounded-lg transition-all"
        >
          <AppWindow className="w-4 h-4 text-slate-400" />
          <span>悬浮监控球</span>
        </button>
      </div>
    </aside>
  );
};
