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
      alert(`启动失败: ${err}`);
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

  const handleToggleHud = async () => {
    try {
      await invoke('toggle_floating_window');
    } catch (err) {
      console.error(err);
    }
  };

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-slate-950 text-slate-100">
      <Sidebar
        activeTab={activeTab}
        onTabChange={setActiveTab}
        isRunning={isRunning}
        onToggleHud={handleToggleHud}
      />
      <main className="flex-1 h-screen overflow-y-auto p-6">
        {activeTab === 'connect' && (
          <ConnectView
            config={config}
            isRunning={isRunning}
            statusText={statusText}
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
          <LogsView
            logs={logs}
            onClearLogs={() => setLogs([])}
          />
        )}
        {activeTab === 'forward' && (
          <ForwardView
            config={config}
            onSaveConfig={handleSaveConfig}
          />
        )}
        {activeTab === 'settings' && (
          <SettingsView
            config={config}
            onSaveConfig={handleSaveConfig}
          />
        )}
      </main>
    </div>
  );
}

export default App;
