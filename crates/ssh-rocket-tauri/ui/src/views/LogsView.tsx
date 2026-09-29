import React, { useState } from 'react';
import { Copy, Search, Trash2 } from 'lucide-react';
import { IconButton, PageHeader, Segmented } from '../components/Adwaita';
import { useI18n } from '../i18n';

interface LogsViewProps {
  logs: string[];
  onClearLogs: () => void;
}

type LogFilter = 'all' | 'runtime' | 'helper' | 'ssh';

export const LogsView: React.FC<LogsViewProps> = ({ logs, onClearLogs }) => {
  const { tr } = useI18n();
  const [filter, setFilter] = useState<LogFilter>('all');
  const [search, setSearch] = useState('');

  const filteredLogs = logs.filter((line) => {
    if (filter !== 'all' && !line.toLowerCase().includes(`[${filter}]`)) return false;
    return !search || line.toLowerCase().includes(search.toLowerCase());
  });

  const copyLogs = async () => {
    await navigator.clipboard.writeText(filteredLogs.join('\n'));
  };

  return (
    <div className="app-page">
      <PageHeader title={tr('运行日志', 'Logs')}>
        <IconButton label={tr('复制日志', 'Copy Logs')} onClick={copyLogs}><Copy /></IconButton>
        <IconButton label={tr('清空日志', 'Clear Logs')} onClick={onClearLogs}><Trash2 /></IconButton>
      </PageHeader>
      <div className="app-page-content logs-content">
        <div className="toolbar-row">
          <Segmented
            value={filter}
            onChange={setFilter}
            label={tr('日志类型', 'Log Type')}
            options={[
              { value: 'all', label: `${tr('全部', 'All')} (${logs.length})` },
              { value: 'runtime', label: tr('系统', 'System') },
              { value: 'helper', label: 'Helper' },
              { value: 'ssh', label: 'SSH' },
            ]}
          />
          <label className="search-entry">
            <Search aria-hidden="true" />
            <input
              className="adw-search"
              value={search}
              placeholder={tr('搜索日志', 'Search Logs')}
              aria-label={tr('搜索日志', 'Search Logs')}
              onChange={(event) => setSearch(event.target.value)}
            />
          </label>
        </div>
        <div className="log-console" role="log" aria-live="polite">
          {filteredLogs.length === 0 ? (
            <div className="log-empty">{tr('暂无日志记录', 'No log entries')}</div>
          ) : filteredLogs.map((log, index) => {
            const kind = /失败|error/i.test(log)
              ? 'error'
              : /成功|已恢复|就绪/.test(log)
                ? 'success'
                : /reconnect|重连/i.test(log)
                  ? 'warning'
                  : '';
            return <div className={kind} key={`${index}-${log}`}>{log}</div>;
          })}
        </div>
      </div>
    </div>
  );
};
