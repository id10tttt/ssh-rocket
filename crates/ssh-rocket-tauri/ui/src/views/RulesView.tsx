import React, { useState, useEffect } from 'react';
import { 
  ShieldCheck, 
  Search, 
  Plus, 
  Trash2, 
  Download, 
  Layers, 
  Globe2, 
  Ban, 
  Check, 
  ExternalLink 
} from 'lucide-react';
import { AppConfig, RuleAction, DesktopApp, DomainRule, IpRule } from '../types';

interface RulesViewProps {
  config: AppConfig | null;
  desktopApps: DesktopApp[];
  onSaveConfig: (cfg: AppConfig) => void;
  onImportRules: (url: string) => Promise<any>;
}

type TabType = 'apps' | 'custom' | 'imported';

export const RulesView: React.FC<RulesViewProps> = ({
  config,
  desktopApps,
  onSaveConfig,
  onImportRules
}) => {
  const [activeTab, setActiveTab] = useState<TabType>('apps');
  const [searchQuery, setSearchQuery] = useState('');
  const [showAddModal, setShowAddModal] = useState(false);
  const [showImportModal, setShowImportModal] = useState(false);
  const [importUrl, setImportUrl] = useState(
    config?.settings.rule_source || 'https://johnshall.github.io/Shadowrocket-ADBlock-Rules-Forever/sr_top500_banlist_ad.conf'
  );
  const [importing, setImporting] = useState(false);

  // New Custom Rule state
  const [newRuleType, setNewRuleType] = useState<'domain' | 'ip'>('domain');
  const [newRulePattern, setNewRulePattern] = useState('');
  const [newRuleKind, setNewRuleKind] = useState<'Domain' | 'DomainSuffix' | 'DomainKeyword'>('DomainSuffix');
  const [newRuleAction, setNewRuleAction] = useState<RuleAction>('proxy');

  const handleAppActionChange = (executable: string, action: RuleAction) => {
    if (!config) return;
    const currentRules = [...config.settings.app_rules];
    const filtered = currentRules.filter(r => r.executable !== executable);
    filtered.push({ executable, action });
    onSaveConfig({
      ...config,
      settings: { ...config.settings, app_rules: filtered }
    });
  };

  const getAppAction = (executable: string): RuleAction => {
    const found = config?.settings.app_rules.find(r => r.executable === executable);
    return found ? found.action : 'direct';
  };

  const handleAddCustomRule = () => {
    if (!config || !newRulePattern.trim()) return;
    if (newRuleType === 'domain') {
      const domainRules = [...config.settings.domain_rules, {
        pattern: newRulePattern.trim(),
        kind: newRuleKind,
        action: newRuleAction,
      }];
      onSaveConfig({
        ...config,
        settings: { ...config.settings, domain_rules: domainRules }
      });
    } else {
      const ipRules = [...config.settings.ip_rules, {
        network: newRulePattern.trim(),
        action: newRuleAction,
      }];
      onSaveConfig({
        ...config,
        settings: { ...config.settings, ip_rules: ipRules }
      });
    }
    setNewRulePattern('');
    setShowAddModal(false);
  };

  const handleDeleteDomainRule = (index: number) => {
    if (!config) return;
    const updated = config.settings.domain_rules.filter((_, i) => i !== index);
    onSaveConfig({
      ...config,
      settings: { ...config.settings, domain_rules: updated }
    });
  };

  const handleDeleteIpRule = (index: number) => {
    if (!config) return;
    const updated = config.settings.ip_rules.filter((_, i) => i !== index);
    onSaveConfig({
      ...config,
      settings: { ...config.settings, ip_rules: updated }
    });
  };

  const handleTriggerImport = async () => {
    if (!importUrl.trim()) return;
    setImporting(true);
    try {
      await onImportRules(importUrl.trim());
      setShowImportModal(false);
    } catch (err: any) {
      alert(`导入失败: ${err}`);
    } finally {
      setImporting(false);
    }
  };

  const filteredApps = desktopApps.filter(app => 
    app.name.toLowerCase().includes(searchQuery.toLowerCase()) || 
    app.executable.toLowerCase().includes(searchQuery.toLowerCase())
  );

  return (
    <div className="h-full flex flex-col space-y-4 max-w-5xl mx-auto p-2">
      {/* Top Header & Tab Controls */}
      <div className="flex items-center justify-between">
        <div className="flex bg-slate-900/80 border border-slate-800 rounded-xl p-1 gap-1">
          <button
            onClick={() => setActiveTab('apps')}
            className={`flex items-center gap-2 px-3.5 py-1.5 rounded-lg text-xs font-medium transition-all ${
              activeTab === 'apps' ? 'bg-blue-600 text-white shadow' : 'text-slate-400 hover:text-slate-200'
            }`}
          >
            <Layers className="w-3.5 h-3.5" />
            <span>应用分流 ({desktopApps.length})</span>
          </button>
          <button
            onClick={() => setActiveTab('custom')}
            className={`flex items-center gap-2 px-3.5 py-1.5 rounded-lg text-xs font-medium transition-all ${
              activeTab === 'custom' ? 'bg-blue-600 text-white shadow' : 'text-slate-400 hover:text-slate-200'
            }`}
          >
            <Globe2 className="w-3.5 h-3.5" />
            <span>自定义规则 ({(config?.settings.domain_rules.length || 0) + (config?.settings.ip_rules.length || 0)})</span>
          </button>
          <button
            onClick={() => setActiveTab('imported')}
            className={`flex items-center gap-2 px-3.5 py-1.5 rounded-lg text-xs font-medium transition-all ${
              activeTab === 'imported' ? 'bg-blue-600 text-white shadow' : 'text-slate-400 hover:text-slate-200'
            }`}
          >
            <Ban className="w-3.5 h-3.5" />
            <span>订阅规则 ({(config?.settings.imported_domain_rules.length || 0) + (config?.settings.imported_ip_rules.length || 0)})</span>
          </button>
        </div>

        <div className="flex items-center gap-3">
          <div className="relative">
            <Search className="w-4 h-4 absolute left-3 top-2.5 text-slate-400" />
            <input
              type="text"
              value={searchQuery}
              onChange={e => setSearchQuery(e.target.value)}
              placeholder="搜索应用或规则..."
              className="bg-slate-900 border border-slate-800 rounded-xl pl-9 pr-3 py-1.5 text-xs text-white focus:outline-none focus:border-blue-500 w-52"
            />
          </div>

          {activeTab === 'custom' && (
            <button
              onClick={() => setShowAddModal(true)}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-blue-600 hover:bg-blue-500 text-white text-xs font-medium rounded-xl shadow transition-all"
            >
              <Plus className="w-4 h-4" />
              <span>添加规则</span>
            </button>
          )}

          {activeTab === 'imported' && (
            <button
              onClick={() => setShowImportModal(true)}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-blue-600 hover:bg-blue-500 text-white text-xs font-medium rounded-xl shadow transition-all"
            >
              <Download className="w-4 h-4" />
              <span>导入订阅</span>
            </button>
          )}
        </div>
      </div>

      {/* Main Tab Content Display */}
      <div className="flex-1 bg-slate-900/60 border border-slate-800 rounded-2xl overflow-hidden shadow-lg backdrop-blur-sm flex flex-col">
        {/* Tab 1: App Rules */}
        {activeTab === 'apps' && (
          <div className="overflow-y-auto flex-1 divide-y divide-slate-800/60">
            {filteredApps.map((app) => {
              const currentAction = getAppAction(app.executable);
              return (
                <div key={app.executable} className="flex items-center justify-between px-5 py-3 hover:bg-slate-800/30 transition-all">
                  <div className="flex items-center gap-3.5">
                    <div className="w-9 h-9 rounded-xl bg-slate-800 flex items-center justify-center text-sm font-bold text-slate-300 border border-slate-700">
                      {app.name.charAt(0).toUpperCase()}
                    </div>
                    <div>
                      <h4 className="text-xs font-semibold text-white">{app.name}</h4>
                      <p className="text-[11px] text-slate-400 font-mono mt-0.5">{app.executable}</p>
                    </div>
                  </div>

                  <div className="flex items-center gap-1.5 bg-slate-950/60 border border-slate-800 rounded-lg p-1">
                    <button
                      onClick={() => handleAppActionChange(app.executable, 'direct')}
                      className={`px-3 py-1 rounded-md text-xs font-medium transition-all ${
                        currentAction === 'direct' ? 'bg-slate-700 text-white shadow' : 'text-slate-400 hover:text-slate-200'
                      }`}
                    >
                      直连
                    </button>
                    <button
                      onClick={() => handleAppActionChange(app.executable, 'proxy')}
                      className={`px-3 py-1 rounded-md text-xs font-medium transition-all ${
                        currentAction === 'proxy' ? 'bg-blue-600 text-white shadow' : 'text-slate-400 hover:text-slate-200'
                      }`}
                    >
                      代理
                    </button>
                    <button
                      onClick={() => handleAppActionChange(app.executable, 'block')}
                      className={`px-3 py-1 rounded-md text-xs font-medium transition-all ${
                        currentAction === 'block' ? 'bg-rose-600 text-white shadow' : 'text-slate-400 hover:text-slate-200'
                      }`}
                    >
                      拦截
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {/* Tab 2: Custom Rules */}
        {activeTab === 'custom' && (
          <div className="overflow-y-auto flex-1 divide-y divide-slate-800/60">
            {config?.settings.domain_rules.map((rule, idx) => (
              <div key={`domain-${idx}`} className="flex items-center justify-between px-5 py-3 hover:bg-slate-800/30 transition-all">
                <div className="flex items-center gap-3">
                  <span className="px-2 py-0.5 rounded text-[10px] font-mono font-medium bg-blue-900/40 text-blue-400 border border-blue-800/60">
                    {rule.kind}
                  </span>
                  <span className="text-xs font-mono text-white">{rule.pattern}</span>
                </div>
                <div className="flex items-center gap-3">
                  <span className={`text-[11px] font-medium px-2 py-0.5 rounded ${
                    rule.action === 'proxy' ? 'bg-blue-500/20 text-blue-400' :
                    rule.action === 'direct' ? 'bg-emerald-500/20 text-emerald-400' : 'bg-rose-500/20 text-rose-400'
                  }`}>
                    {rule.action.toUpperCase()}
                  </span>
                  <button
                    onClick={() => handleDeleteDomainRule(idx)}
                    className="text-slate-500 hover:text-rose-400 transition-all p-1"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </div>
              </div>
            ))}

            {config?.settings.ip_rules.map((rule, idx) => (
              <div key={`ip-${idx}`} className="flex items-center justify-between px-5 py-3 hover:bg-slate-800/30 transition-all">
                <div className="flex items-center gap-3">
                  <span className="px-2 py-0.5 rounded text-[10px] font-mono font-medium bg-emerald-900/40 text-emerald-400 border border-emerald-800/60">
                    IP-CIDR
                  </span>
                  <span className="text-xs font-mono text-white">{rule.network}</span>
                </div>
                <div className="flex items-center gap-3">
                  <span className={`text-[11px] font-medium px-2 py-0.5 rounded ${
                    rule.action === 'proxy' ? 'bg-blue-500/20 text-blue-400' :
                    rule.action === 'direct' ? 'bg-emerald-500/20 text-emerald-400' : 'bg-rose-500/20 text-rose-400'
                  }`}>
                    {rule.action.toUpperCase()}
                  </span>
                  <button
                    onClick={() => handleDeleteIpRule(idx)}
                    className="text-slate-500 hover:text-rose-400 transition-all p-1"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Tab 3: Imported Rules */}
        {activeTab === 'imported' && (
          <div className="overflow-y-auto flex-1 divide-y divide-slate-800/60">
            {config?.settings.imported_domain_rules.slice(0, 100).map((rule, idx) => (
              <div key={`imp-domain-${idx}`} className="flex items-center justify-between px-5 py-2.5 hover:bg-slate-800/30 transition-all">
                <div className="flex items-center gap-3">
                  <span className="px-2 py-0.5 rounded text-[10px] font-mono font-medium bg-slate-800 text-slate-400">
                    {rule.kind}
                  </span>
                  <span className="text-xs font-mono text-slate-300">{rule.pattern}</span>
                </div>
                <span className="text-[11px] font-medium px-2 py-0.5 rounded bg-rose-500/10 text-rose-400">
                  {rule.action.toUpperCase()}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Add Custom Rule Modal */}
      {showAddModal && (
        <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
          <div className="bg-slate-900 border border-slate-800 rounded-2xl w-full max-w-md p-6 shadow-2xl">
            <h3 className="text-sm font-semibold text-white mb-4">添加自定义分流规则</h3>
            
            <div className="space-y-4 text-xs">
              <div>
                <label className="block text-slate-400 mb-1">规则类型</label>
                <div className="flex gap-2">
                  <button
                    onClick={() => setNewRuleType('domain')}
                    className={`flex-1 py-1.5 rounded-lg border text-xs font-medium transition-all ${
                      newRuleType === 'domain' ? 'bg-blue-600/20 border-blue-500 text-blue-400' : 'bg-slate-950 border-slate-800 text-slate-400'
                    }`}
                  >
                    域名规则
                  </button>
                  <button
                    onClick={() => setNewRuleType('ip')}
                    className={`flex-1 py-1.5 rounded-lg border text-xs font-medium transition-all ${
                      newRuleType === 'ip' ? 'bg-blue-600/20 border-blue-500 text-blue-400' : 'bg-slate-950 border-slate-800 text-slate-400'
                    }`}
                  >
                    IP-CIDR
                  </button>
                </div>
              </div>

              {newRuleType === 'domain' && (
                <div>
                  <label className="block text-slate-400 mb-1">域名匹配方式</label>
                  <select
                    value={newRuleKind}
                    onChange={e => setNewRuleKind(e.target.value as any)}
                    className="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500"
                  >
                    <option value="DomainSuffix">DOMAIN-SUFFIX (后缀匹配)</option>
                    <option value="Domain">DOMAIN (完全匹配)</option>
                    <option value="DomainKeyword">DOMAIN-KEYWORD (关键字匹配)</option>
                  </select>
                </div>
              )}

              <div>
                <label className="block text-slate-400 mb-1">
                  {newRuleType === 'domain' ? '域名或后缀' : 'IP / CIDR 网段'}
                </label>
                <input
                  type="text"
                  value={newRulePattern}
                  onChange={e => setNewRulePattern(e.target.value)}
                  placeholder={newRuleType === 'domain' ? '例如 google.com' : '例如 192.168.1.0/24'}
                  className="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500 font-mono"
                />
              </div>

              <div>
                <label className="block text-slate-400 mb-1">分流动作</label>
                <div className="flex gap-2">
                  <button
                    onClick={() => setNewRuleAction('proxy')}
                    className={`flex-1 py-1.5 rounded-lg border text-xs font-medium transition-all ${
                      newRuleAction === 'proxy' ? 'bg-blue-600 text-white border-blue-500' : 'bg-slate-950 border-slate-800 text-slate-400'
                    }`}
                  >
                    代理 (PROXY)
                  </button>
                  <button
                    onClick={() => setNewRuleAction('direct')}
                    className={`flex-1 py-1.5 rounded-lg border text-xs font-medium transition-all ${
                      newRuleAction === 'direct' ? 'bg-emerald-600 text-white border-emerald-500' : 'bg-slate-950 border-slate-800 text-slate-400'
                    }`}
                  >
                    直连 (DIRECT)
                  </button>
                  <button
                    onClick={() => setNewRuleAction('block')}
                    className={`flex-1 py-1.5 rounded-lg border text-xs font-medium transition-all ${
                      newRuleAction === 'block' ? 'bg-rose-600 text-white border-rose-500' : 'bg-slate-950 border-slate-800 text-slate-400'
                    }`}
                  >
                    拦截 (REJECT)
                  </button>
                </div>
              </div>
            </div>

            <div className="flex justify-end gap-3 mt-6">
              <button
                onClick={() => setShowAddModal(false)}
                className="px-4 py-2 text-xs text-slate-400 hover:text-white"
              >
                取消
              </button>
              <button
                onClick={handleAddCustomRule}
                className="px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white text-xs font-medium rounded-xl shadow"
              >
                确认添加
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Import Modal */}
      {showImportModal && (
        <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
          <div className="bg-slate-900 border border-slate-800 rounded-2xl w-full max-w-lg p-6 shadow-2xl">
            <h3 className="text-sm font-semibold text-white mb-2">导入 Shadowrocket / SwitchyOmega 规则源</h3>
            <p className="text-xs text-slate-400 mb-4">
              支持一键下载并解析远端 HTTPS 规则订阅文件，自动并入拦截与直连规则表。
            </p>

            <div className="space-y-4 text-xs">
              <div>
                <label className="block text-slate-400 mb-1">规则订阅链接 (仅限 HTTPS)</label>
                <input
                  type="text"
                  value={importUrl}
                  onChange={e => setImportUrl(e.target.value)}
                  placeholder="https://..."
                  className="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white font-mono focus:outline-none focus:border-blue-500"
                />
              </div>
            </div>

            <div className="flex justify-end gap-3 mt-6">
              <button
                onClick={() => setShowImportModal(false)}
                disabled={importing}
                className="px-4 py-2 text-xs text-slate-400 hover:text-white"
              >
                取消
              </button>
              <button
                onClick={handleTriggerImport}
                disabled={importing}
                className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white text-xs font-medium rounded-xl shadow disabled:opacity-50"
              >
                {importing && <div className="w-3.5 h-3.5 border-2 border-white border-t-transparent rounded-full animate-spin" />}
                <span>{importing ? '正在下载导入...' : '开始导入'}</span>
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
