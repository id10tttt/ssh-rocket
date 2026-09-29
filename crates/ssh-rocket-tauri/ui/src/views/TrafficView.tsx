import React, { useEffect, useRef, useState } from 'react';
import { Search } from 'lucide-react';
import { ActiveConnectionStat, AppTrafficStat, SpeedDto } from '../types';
import { Card, PageHeader, PreferenceGroup, Segmented } from '../components/Adwaita';
import { useI18n } from '../i18n';

interface TrafficViewProps {
  speed: SpeedDto;
  appTraffic: AppTrafficStat[];
  activeConnections: ActiveConnectionStat[];
  isRunning: boolean;
}

type TrafficTab = 'overview' | 'apps' | 'connections';

const formatBytes = (bytes: number) => {
  if (bytes < 1024) return `${bytes.toFixed(0)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
};

export const TrafficView: React.FC<TrafficViewProps> = ({
  speed,
  appTraffic,
  activeConnections,
  isRunning,
}) => {
  const { tr } = useI18n();
  const [tab, setTab] = useState<TrafficTab>('overview');
  const [query, setQuery] = useState('');
  const historyRef = useRef<{ up: number[]; down: number[] }>({
    up: new Array(40).fill(0),
    down: new Array(40).fill(0),
  });
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const current = historyRef.current;
    const history = {
      up: [...current.up.slice(1), speed.upload],
      down: [...current.down.slice(1), speed.download],
    };
    historyRef.current = history;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext('2d');
    if (!context) return;
    const { width, height } = canvas;
    context.clearRect(0, 0, width, height);
    const maxValue = Math.max(...history.up, ...history.down, 100 * 1024);
    const styles = getComputedStyle(document.documentElement);
    const draw = (values: number[], color: string) => {
      context.beginPath();
      values.forEach((value, index) => {
        const x = index * width / (values.length - 1);
        const y = height - 12 - value / maxValue * (height - 24);
        if (index === 0) context.moveTo(x, y);
        else context.lineTo(x, y);
      });
      context.strokeStyle = color;
      context.lineWidth = 2;
      context.stroke();
    };
    draw(history.down, styles.getPropertyValue('--success').trim());
    draw(history.up, '#3584e4');
  }, [speed, tab]);

  const filteredApps = appTraffic.filter((item) =>
    `${item.name} ${item.id}`.toLowerCase().includes(query.toLowerCase()),
  );
  const filteredConnections = activeConnections.filter((item) =>
    `${item.proc_name} ${item.peer_addr}`.toLowerCase().includes(query.toLowerCase()),
  );
  const totalUpload = appTraffic.reduce((sum, item) => sum + item.upload, 0);
  const totalDownload = appTraffic.reduce((sum, item) => sum + item.download, 0);
  const proxyTotal = appTraffic.reduce((sum, item) => sum + item.proxy_upload + item.proxy_download, 0);
  const directTotal = appTraffic.reduce((sum, item) => sum + item.direct_upload + item.direct_download, 0);

  return (
    <div className="app-page">
      <PageHeader title={tr('流量监控', 'Traffic')} />
      <div className="traffic-switcher">
        <Segmented
          value={tab}
          onChange={(value) => {
            setTab(value);
            setQuery('');
          }}
          label={tr('流量页面', 'Traffic Page')}
          options={[
            { value: 'overview', label: tr('监控总览', 'Overview') },
            { value: 'apps', label: tr('应用统计', 'App Usage') },
            { value: 'connections', label: tr('实时连接', 'Active Connections') },
          ]}
        />
      </div>

      <div className="app-page-content traffic-content">
        {tab === 'overview' && (
          <>
            <PreferenceGroup title={tr('会话与传输总览', 'Session & Traffic Overview')} description={isRunning ? tr('已连接', 'Connected') : tr('未连接', 'Disconnected')}>
              <div className="traffic-kpis">
                <div><span>{tr('总流量', 'Total Traffic')}</span><strong>{formatBytes(totalUpload + totalDownload)}</strong><small>↑ {formatBytes(totalUpload)}　↓ {formatBytes(totalDownload)}</small></div>
                <div><span>{tr('代理流量', 'Proxy Traffic')}</span><strong>{formatBytes(proxyTotal)}</strong><small>{appTraffic.filter((item) => item.proxy_upload + item.proxy_download > 0).length} {tr('个应用', 'apps')}</small></div>
                <div><span>{tr('直连流量', 'Direct Traffic')}</span><strong>{formatBytes(directTotal)}</strong><small>{appTraffic.filter((item) => item.direct_upload + item.direct_download > 0).length} {tr('个应用', 'apps')}</small></div>
              </div>
            </PreferenceGroup>
            <PreferenceGroup title={tr('实时速率', 'Real-time Speed')}>
              <Card className="traffic-chart-card">
                <div className="traffic-chart-header">
                  <span><i className="download" />{tr('下载', 'Download')} {formatBytes(speed.download)}/s</span>
                  <span><i className="upload" />{tr('上传', 'Upload')} {formatBytes(speed.upload)}/s</span>
                </div>
                <canvas ref={canvasRef} width={820} height={185} aria-label={tr('实时上传与下载速率曲线', 'Real-time upload and download speed chart')} />
              </Card>
            </PreferenceGroup>
          </>
        )}

        {tab === 'apps' && (
          <PreferenceGroup title={tr('应用流量排行', 'Application Traffic')}>
            <div className="traffic-toolbar">
              <label className="search-entry">
                <Search aria-hidden="true" />
                <input className="adw-search" value={query} placeholder={tr('搜索应用', 'Search Apps')} onChange={(event) => setQuery(event.target.value)} />
              </label>
            </div>
            <div className="table-card">
              {filteredApps.length === 0 ? <div className="list-empty">{tr('暂无应用流量数据', 'No application traffic data')}</div> : filteredApps.map((app) => (
                <div className="table-row traffic-row" key={app.id}>
                  <div className="app-avatar">{app.name.slice(0, 1).toUpperCase()}</div>
                  <div className="traffic-copy"><strong>{app.name}</strong><span>{app.id}</span></div>
                  <span className={`badge ${app.primary_type.toLowerCase()}`}>{app.primary_type}</span>
                  <div className="traffic-values"><span>↓ {formatBytes(app.download)}</span><span>↑ {formatBytes(app.upload)}</span></div>
                </div>
              ))}
            </div>
          </PreferenceGroup>
        )}

        {tab === 'connections' && (
          <PreferenceGroup title={tr('活跃连接', 'Active Connections')} description={`${filteredConnections.length} ${tr('条连接', 'connections')}`}>
            <div className="traffic-toolbar">
              <label className="search-entry">
                <Search aria-hidden="true" />
                <input className="adw-search" value={query} placeholder={tr('搜索连接', 'Search Connections')} onChange={(event) => setQuery(event.target.value)} />
              </label>
            </div>
            <div className="table-card">
              {filteredConnections.length === 0 ? <div className="list-empty">{tr('暂无活跃连接', 'No active connections')}</div> : filteredConnections.map((connection, index) => (
                <div className="table-row traffic-row" key={`${connection.local_addr}-${index}`}>
                  <div className="traffic-copy"><strong>{connection.proc_name}</strong><span>{connection.local_addr} → {connection.peer_addr}</span></div>
                  <span className={`badge ${connection.conn_type.toLowerCase()}`}>{connection.conn_type}</span>
                  <div className="traffic-values"><span>↓ {formatBytes(connection.download)}</span><span>↑ {formatBytes(connection.upload)}</span></div>
                </div>
              ))}
            </div>
          </PreferenceGroup>
        )}
      </div>
    </div>
  );
};
