import React from 'react';
import {
  Activity,
  ArrowLeftRight,
  Rocket,
  ScrollText,
  Settings,
  ShieldCheck,
} from 'lucide-react';
import { useI18n } from '../i18n';

export type NavTab = 'connect' | 'rules' | 'traffic' | 'logs' | 'forward' | 'settings';

interface SidebarProps {
  activeTab: NavTab;
  onTabChange: (tab: NavTab) => void;
}

export const Sidebar: React.FC<SidebarProps> = ({ activeTab, onTabChange }) => {
  const { tr } = useI18n();
  const navItems = [
    { id: 'connect', label: tr('节点连接', 'Connections'), icon: Rocket },
    { id: 'rules', label: tr('分流规则', 'Rules'), icon: ShieldCheck },
    { id: 'traffic', label: tr('流量监控', 'Traffic'), icon: Activity },
    { id: 'logs', label: tr('运行日志', 'Logs'), icon: ScrollText },
    { id: 'forward', label: tr('端口转发', 'Port Forwarding'), icon: ArrowLeftRight },
    { id: 'settings', label: tr('应用设置', 'Settings'), icon: Settings },
  ] satisfies Array<{ id: NavTab; label: string; icon: React.ComponentType<{ className?: string }> }>;

  return (
    <aside className="app-sidebar">
      <div className="sidebar-brand">
        <Rocket aria-hidden="true" />
        <span>SSH Rocket</span>
      </div>
      <nav className="sidebar-nav" aria-label={tr('主导航', 'Main navigation')}>
        {navItems.map((item) => {
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              type="button"
              className={activeTab === item.id ? 'is-active' : undefined}
              aria-current={activeTab === item.id ? 'page' : undefined}
              onClick={() => onTabChange(item.id)}
            >
              <Icon aria-hidden="true" />
              <span>{item.label}</span>
            </button>
          );
        })}
      </nav>
    </aside>
  );
};
