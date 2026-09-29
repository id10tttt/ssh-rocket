import React from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Languages, MonitorCog, Moon } from 'lucide-react';
import { AppConfig, Settings } from '../types';
import { ActionRow, PageHeader, PreferenceGroup, Switch } from '../components/Adwaita';
import { useI18n } from '../i18n';

interface SettingsViewProps {
  config: AppConfig | null;
  onSaveConfig: (cfg: AppConfig) => void;
}

export const SettingsView: React.FC<SettingsViewProps> = ({ config, onSaveConfig }) => {
  const { tr } = useI18n();
  if (!config) {
    return <div className="app-page"><PageHeader title={tr('应用设置', 'Settings')} /></div>;
  }

  const updateSettings = (patch: Partial<Settings>) => {
    onSaveConfig({ ...config, settings: { ...config.settings, ...patch } });
  };

  const updateFloating = (patch: Partial<Settings['floating_widget']>) => {
    updateSettings({ floating_widget: { ...config.settings.floating_widget, ...patch } });
  };

  const toggleFloating = async () => {
    const enabled = !config.settings.floating_widget.enabled;
    updateFloating({ enabled });
    try {
      await invoke('set_floating_window_visible', { visible: enabled });
    } catch (error) {
      console.error(error);
    }
  };

  return (
    <div className="app-page">
      <PageHeader title={tr('应用设置', 'Settings')} />
      <div className="app-page-content settings-content">
        <div className="settings-columns">
          <PreferenceGroup title={tr('外观', 'Appearance')}>
            <ActionRow title={tr('主题', 'Theme')} prefix={<Moon />}>
              <select
                className="row-select"
                value={config.settings.theme_mode}
                onChange={(event) => updateSettings({ theme_mode: event.target.value as Settings['theme_mode'] })}
              >
                <option value="auto">{tr('跟随系统', 'Follow System')}</option>
                <option value="light">{tr('浅色', 'Light')}</option>
                <option value="dark">{tr('深色', 'Dark')}</option>
              </select>
            </ActionRow>
          </PreferenceGroup>

          <PreferenceGroup title={tr('语言与区域', 'Language & Region')}>
            <ActionRow title={tr('界面语言', 'Interface Language')} prefix={<Languages />}>
              <select
                className="row-select"
                value={config.settings.language}
                onChange={(event) => updateSettings({ language: event.target.value as Settings['language'] })}
              >
                <option value="auto">{tr('跟随系统', 'Follow System')}</option>
                <option value="chinese">简体中文</option>
                <option value="english">English</option>
              </select>
            </ActionRow>
          </PreferenceGroup>
        </div>

        <PreferenceGroup title={tr('小工具与插件', 'Widgets & Plugins')} description={tr('配置桌面悬浮监控组件', 'Configure the desktop floating monitor')}>
          <ActionRow
            title={tr('悬浮监控球', 'Floating Monitor')}
            subtitle={tr('在桌面显示实时网络速度与硬件占用', 'Show live network speed and hardware usage on the desktop')}
            prefix={<MonitorCog />}
          >
            <Switch
              checked={config.settings.floating_widget.enabled}
              label={tr('悬浮监控球', 'Floating Monitor')}
              onClick={toggleFloating}
            />
          </ActionRow>
          <ActionRow title={tr('闲置透明度', 'Idle Opacity')} subtitle="10% – 90%">
            <input
              className="row-number"
              type="number"
              min={10}
              max={90}
              step={5}
              value={Math.round(config.settings.floating_widget.idle_opacity * 100)}
              onChange={(event) => updateFloating({ idle_opacity: Number(event.target.value) / 100 })}
            />
          </ActionRow>
          <ActionRow title={tr('淡出延迟', 'Fade Delay')}>
            <input
              className="row-number"
              type="number"
              min={1}
              max={30}
              value={config.settings.floating_widget.fade_delay_secs}
              onChange={(event) => updateFloating({ fade_delay_secs: Number(event.target.value) })}
            />
          </ActionRow>
          <ActionRow title={tr('网速小数位数', 'Speed Decimal Places')} subtitle={tr('0 – 3 位', '0 – 3 digits')}>
            <input
              className="row-number"
              type="number"
              min={0}
              max={3}
              value={config.settings.floating_widget.speed_decimals}
              onChange={(event) => updateFloating({ speed_decimals: Number(event.target.value) })}
            />
          </ActionRow>
        </PreferenceGroup>
      </div>
    </div>
  );
};
