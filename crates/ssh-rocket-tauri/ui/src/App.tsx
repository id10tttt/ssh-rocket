import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Sidebar, NavTab } from './components/Sidebar';
import { ConnectView } from './views/ConnectView';
import { RulesView } from './views/RulesView';
import { TrafficView } from './views/TrafficView';
import { LogsView } from './views/LogsView';
import { ForwardView } from './views/ForwardView';
import { SettingsView } from './views/SettingsView';
import { StatusDot } from './components/Adwaita';
import { I18nProvider } from './components/I18nProvider';
import { resolveLanguage, translate } from './i18n';
import { 
  AppConfig, 
  DesktopApp, 
  SpeedDto, 
  AppTrafficStat, 
  ActiveConnectionStat, 
  RuntimeStatusDto 
} from './types';

export function App() {
  const [activeTab, setActiveTab] = useState<NavTab>('connect');
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [desktopApps, setDesktopApps] = useState<DesktopApp[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [statusText, setStatusText] = useState('未连接');
  const [speed, setSpeed] = useState<SpeedDto>({ upload: 0, download: 0 });
  const [appTraffic, setAppTraffic] = useState<AppTrafficStat[]>([]);
  const [activeConnections, setActiveConnections] = useState<ActiveConnectionStat[]>([]);
  const [logs, setLogs] = useState<string[]>([]);

  // Initial load
  useEffect(() => {
    invoke<AppConfig>('get_config').then(setConfig).catch(console.error);
    invoke<RuntimeStatusDto>('get_runtime_status').then(status => {
      setIsRunning(status.is_running);
      setStatusText(status.status_text);
    }).catch(console.error);
    invoke<DesktopApp[]>('get_desktop_apps').then(setDesktopApps).catch(console.error);

    // Event listeners
    const unlistenStatus = listen<string>('status-changed', (event) => {
      setStatusText(event.payload);
      if (event.payload === '已连接') {
        setIsRunning(true);
      } else if (event.payload === '未连接') {
        setIsRunning(false);
      }
    });

    const unlistenConnected = listen('connected', () => {
      setIsRunning(true);
      setStatusText('已连接');
    });

    const unlistenDisconnected = listen('disconnected', () => {
      setIsRunning(false);
      setStatusText('未连接');
      setSpeed({ upload: 0, download: 0 });
    });

    const unlistenSpeed = listen<SpeedDto>('speed', (event) => {
      setSpeed(event.payload);
    });

    const unlistenTraffic = listen<AppTrafficStat[]>('app-traffic', (event) => {
      setAppTraffic(event.payload);
    });

    const unlistenConnections = listen<ActiveConnectionStat[]>('active-connections', (event) => {
      setActiveConnections(event.payload);
    });

    const unlistenLog = listen<string>('log', (event) => {
      setLogs(prev => [...prev.slice(-300), event.payload]);
    });

    return () => {
      unlistenStatus.then(fn => fn());
      unlistenConnected.then(fn => fn());
      unlistenDisconnected.then(fn => fn());
      unlistenSpeed.then(fn => fn());
      unlistenTraffic.then(fn => fn());
      unlistenConnections.then(fn => fn());
      unlistenLog.then(fn => fn());
    };
  }, []);

  useEffect(() => {
    const theme = config?.settings.theme_mode;
    if (theme === 'light' || theme === 'dark') {
      document.documentElement.dataset.theme = theme;
    } else {
      delete document.documentElement.dataset.theme;
    }
  }, [config?.settings.theme_mode]);

  const handleSaveConfig = async (newConfig: AppConfig) => {
    setConfig(newConfig);
    try {
      await invoke('save_config', { config: newConfig });
    } catch (err) {
      console.error('Failed to save config:', err);
    }
  };

  const handleStart = async (profileId?: string) => {
    try {
      await invoke('start_service', { profileId: profileId || null });
    } catch (err: any) {
      alert(`${translate(resolveLanguage(config?.settings.language), '启动失败', 'Failed to start')}: ${err}`);
    }
  };

  const handleStop = async () => {
    try {
      await invoke('stop_service');
    } catch (err: any) {
      console.error(err);
    }
  };

  const handleImportRules = async (url: string) => {
    const res = await invoke('import_rules', { url });
    const updated = await invoke<AppConfig>('get_config');
    setConfig(updated);
    return res;
  };

  const formatSpeed = (bytes: number) => {
    if (bytes < 1024) return `${bytes.toFixed(0)} B/s`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB/s`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB/s`;
  };

  const connectionState = isRunning
    ? 'connected'
    : statusText.includes('正在') || /connecting/i.test(statusText)
      ? 'connecting'
      : 'disconnected';
  const language = resolveLanguage(config?.settings.language);
  const tr = (chinese: string, english: string) => translate(language, chinese, english);
  const localizedStatus = ({
    未连接: tr('未连接', 'Disconnected'),
    已连接: tr('已连接', 'Connected'),
    '正在连接…': tr('正在连接…', 'Connecting…'),
    '正在断开…': tr('正在断开…', 'Disconnecting…'),
  } as Record<string, string>)[statusText] ?? statusText;

  useEffect(() => {
    document.documentElement.lang = language === 'en' ? 'en' : 'zh-CN';
  }, [language]);

  return (
    <I18nProvider language={language}>
      <div className="app-shell">
        <Sidebar activeTab={activeTab} onTabChange={setActiveTab} />
        <main className="app-main">
          <div className="app-view">
            {activeTab === 'connect' && (
              <ConnectView
                config={config}
                isRunning={isRunning}
                statusText={localizedStatus}
                onStart={handleStart}
                onStop={handleStop}
                onSaveConfig={handleSaveConfig}
              />
            )}
            {activeTab === 'rules' && (
              <RulesView
                config={config}
                desktopApps={desktopApps}
                onSaveConfig={handleSaveConfig}
                onImportRules={handleImportRules}
              />
            )}
            {activeTab === 'traffic' && (
              <TrafficView
                speed={speed}
                appTraffic={appTraffic}
                activeConnections={activeConnections}
                isRunning={isRunning}
              />
            )}
            {activeTab === 'logs' && (
              <LogsView logs={logs} onClearLogs={() => setLogs([])} />
            )}
            {activeTab === 'forward' && (
              <ForwardView config={config} onSaveConfig={handleSaveConfig} />
            )}
            {activeTab === 'settings' && (
              <SettingsView config={config} onSaveConfig={handleSaveConfig} />
            )}
          </div>
          <footer className="app-statusbar">
            <StatusDot state={connectionState} />
            <span className="status-copy">{localizedStatus}</span>
            <span className="speed-copy">↑ {formatSpeed(speed.upload)}　↓ {formatSpeed(speed.download)}</span>
          </footer>
        </main>
      </div>
    </I18nProvider>
  );
}

export default App;
