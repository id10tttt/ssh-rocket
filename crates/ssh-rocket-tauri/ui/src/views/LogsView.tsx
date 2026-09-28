import React, { useState } from 'react';
import { ScrollText, Trash2, Search } from 'lucide-react';

interface LogsViewProps {
  logs: string[];
  onClearLogs: () => void;
}

export const LogsView: React.FC<LogsViewProps> = ({ logs, onClearLogs }) => {
  const [filter, setFilter] = useState<'all' | 'runtime' | 'helper' | 'ssh'>('all');
  const [search, setSearch] = useState('');

  const filteredLogs = logs.filter(line => {
    if (filter === 'runtime' && !line.includes('[runtime]')) return false;
    if (filter === 'helper' && !line.includes('[helper]')) return false;
    if (filter === 'ssh' && !line.includes('[ssh]')) return false;
    if (search && !line.toLowerCase().includes(search.toLowerCase())) return false;
    return true;
  });

  return (
    <div className="h-full flex flex-col space-y-4 max-w-5xl mx-auto p-2">
      {/* Top Filter Bar */}
      <div className="flex items-center justify-between">
        <div className="flex bg-slate-900 border border-slate-800 rounded-xl p-1 gap-1">
          <button
            onClick={() => setFilter('all')}
            className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all ${
              filter === 'all' ? 'bg-blue-600 text-white shadow' : 'text-slate-400 hover:text-slate-200'
            }`}
          >
            全部日志 ({logs.length})
          </button>
          <button
            onClick={() => setFilter('ssh')}
            className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all ${
              filter === 'ssh' ? 'bg-blue-600 text-white shadow' : 'text-slate-400 hover:text-slate-200'
            }`}
          >
            SSH 核心
          </button>
          <button
            onClick={() => setFilter('helper')}
            className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all ${
              filter === 'helper' ? 'bg-blue-600 text-white shadow' : 'text-slate-400 hover:text-slate-200'
            }`}
          >
            Helper 守护
          </button>
        </div>

        <div className="flex items-center gap-3">
          <div className="relative">
            <Search className="w-4 h-4 absolute left-3 top-2 text-slate-400" />
            <input
              type="text"
              value={search}
              onChange={e => setSearch(e.target.value)}
              placeholder="过滤日志..."
              className="bg-slate-900 border border-slate-800 rounded-xl pl-9 pr-3 py-1.5 text-xs text-white focus:outline-none focus:border-blue-500 w-48"
            />
          </div>

          <button
            onClick={onClearLogs}
            className="flex items-center gap-1.5 px-3 py-1.5 bg-slate-800 hover:bg-slate-700 text-slate-300 hover:text-rose-400 text-xs font-medium rounded-xl border border-slate-700 transition-all"
          >
            <Trash2 className="w-4 h-4" />
            <span>清空</span>
          </button>
        </div>
      </div>

      {/* Terminal-like Log Console */}
      <div className="flex-1 bg-slate-950/80 border border-slate-800 rounded-2xl p-4 font-mono text-xs overflow-y-auto shadow-inner space-y-1">
        {filteredLogs.length === 0 ? (
          <div className="h-full flex items-center justify-center text-slate-600">
            暂无日志记录
          </div>
        ) : (
          filteredLogs.map((log, index) => {
            let textColor = 'text-slate-300';
            if (log.includes('失败') || log.includes('error') || log.includes('Error')) {
              textColor = 'text-rose-400';
            } else if (log.includes('成功') || log.includes('已恢复') || log.includes('就绪')) {
              textColor = 'text-emerald-400';
            } else if (log.includes('[reconnect]') || log.includes('重连')) {
              textColor = 'text-amber-400';
            }

            return (
              <div key={index} className={`leading-relaxed break-all ${textColor}`}>
                {log}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
};
