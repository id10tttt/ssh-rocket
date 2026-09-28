import React, { useState } from 'react';
import { 
  ArrowLeftRight, 
  Plus, 
  Trash2, 
  Edit3, 
  CheckCircle2, 
  Globe, 
  ArrowRight,
  ArrowLeft,
  X
} from 'lucide-react';
import { AppConfig, PortForwardRule, ForwardType } from '../types';

interface ForwardViewProps {
  config: AppConfig | null;
  onSaveConfig: (cfg: AppConfig) => void;
}

export const ForwardView: React.FC<ForwardViewProps> = ({ config, onSaveConfig }) => {
  const [showModal, setShowModal] = useState(false);
  const [editingRule, setEditingRule] = useState<PortForwardRule | null>(null);

  // Form states
  const [name, setName] = useState('');
  const [forwardType, setForwardType] = useState<ForwardType>('local');
  const [profileId, setProfileId] = useState('');
  const [localHost, setLocalHost] = useState('127.0.0.1');
  const [localPort, setLocalPort] = useState('');
  const [remoteHost, setRemoteHost] = useState('127.0.0.1');
  const [remotePort, setRemotePort] = useState('');

  const rules = config?.port_forwards || [];
  const profiles = config?.profiles || [];

  const handleOpenAdd = () => {
    setEditingRule(null);
    setName('');
    setForwardType('local');
    setProfileId(profiles[0]?.id || '');
    setLocalHost('127.0.0.1');
    setLocalPort('');
    setRemoteHost('127.0.0.1');
    setRemotePort('');
    setShowModal(true);
  };

  const handleOpenEdit = (rule: PortForwardRule) => {
    setEditingRule(rule);
    setName(rule.name);
    setForwardType(rule.forward_type);
    setProfileId(rule.profile_id);
    setLocalHost(rule.local_host || '127.0.0.1');
    setLocalPort(rule.local_port ? rule.local_port.toString() : '');
    setRemoteHost(rule.remote_host || '127.0.0.1');
    setRemotePort(rule.remote_port ? rule.remote_port.toString() : '');
    setShowModal(true);
  };

  const handleSave = () => {
    if (!config) return;
    const lPort = parseInt(localPort, 10);
    const rPort = parseInt(remotePort, 10);
    if (!lPort || !rPort || lPort < 1 || lPort > 65535 || rPort < 1 || rPort > 65535) {
      alert('请输入有效的 1-65535 端口号');
      return;
    }
    if (!profileId) {
      alert('请先选择或添加 SSH 节点连接');
      return;
    }

    let ruleName = name.trim();
    if (!ruleName) {
      ruleName = forwardType === 'local' 
        ? `${localHost}:${lPort} -> ${remoteHost}:${rPort}`
        : `${remoteHost}:${rPort} <- ${localHost}:${lPort}`;
    }

    const newRule: PortForwardRule = {
      id: editingRule ? editingRule.id : crypto.randomUUID(),
      name: ruleName,
      profile_id: profileId,
      forward_type: forwardType,
      local_host: localHost.trim() || '127.0.0.1',
      local_port: lPort,
      remote_host: remoteHost.trim() || '127.0.0.1',
      remote_port: rPort,
      enabled: editingRule ? editingRule.enabled : true,
    };

    let updatedRules = [...(config.port_forwards || [])];
    if (editingRule) {
      updatedRules = updatedRules.map(r => r.id === editingRule.id ? newRule : r);
    } else {
      updatedRules.push(newRule);
    }

    onSaveConfig({
      ...config,
      port_forwards: updatedRules,
    });
    setShowModal(false);
  };

  const handleToggle = (ruleId: string) => {
    if (!config) return;
    const updated = (config.port_forwards || []).map(r => {
      if (r.id === ruleId) {
        return { ...r, enabled: !r.enabled };
      }
      return r;
    });
    onSaveConfig({ ...config, port_forwards: updated });
  };

  const handleDelete = (ruleId: string) => {
    if (!config) return;
    const updated = (config.port_forwards || []).filter(r => r.id !== ruleId);
    onSaveConfig({ ...config, port_forwards: updated });
  };

  return (
    <div className="space-y-6 max-w-4xl mx-auto p-2">
      {/* Top Header Card */}
      <div className="bg-slate-900/60 border border-slate-800 rounded-2xl p-6 flex items-center justify-between shadow-xl backdrop-blur-sm">
        <div className="flex items-center gap-4">
          <div className="w-12 h-12 rounded-2xl bg-blue-600/20 border border-blue-500/30 flex items-center justify-center text-blue-400">
            <ArrowLeftRight className="w-6 h-6" />
          </div>
          <div>
            <h2 className="text-lg font-semibold text-white">端口转发</h2>
            <p className="text-xs text-slate-400 mt-0.5">
              将本地端口绑定到目标服务器，或将目标服务器端口绑定到本地
            </p>
          </div>
        </div>

        <button
          onClick={handleOpenAdd}
          className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white text-sm font-medium rounded-xl shadow-lg shadow-blue-900/30 active:scale-95 transition-all"
        >
          <Plus className="w-4 h-4" />
          <span>添加规则</span>
        </button>
      </div>

      {/* Rules List / Empty State */}
      {rules.length === 0 ? (
        <div className="bg-slate-900/40 border border-slate-800 rounded-2xl p-12 text-center">
          <ArrowLeftRight className="w-12 h-12 text-slate-600 mx-auto mb-3" />
          <h3 className="text-sm font-semibold text-slate-300">暂无端口转发规则</h3>
          <p className="text-xs text-slate-500 mt-1 mb-4">
            创建本地转发以访问远程内网服务，或创建远程转发以暴露本地接口
          </p>
          <button
            onClick={handleOpenAdd}
            className="px-4 py-2 bg-slate-800 hover:bg-slate-700 text-slate-200 text-xs font-medium rounded-lg transition-all"
          >
            添加第一条规则
          </button>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {rules.map((rule) => {
            const profile = profiles.find(p => p.id === rule.profile_id);
            const isLocal = rule.forward_type === 'local';

            return (
              <div
                key={rule.id}
                className="bg-slate-900/60 border border-slate-800 rounded-2xl p-5 shadow-lg backdrop-blur-sm flex flex-col justify-between"
              >
                <div>
                  {/* Card Header */}
                  <div className="flex items-center justify-between pb-3 border-b border-slate-800/80">
                    <div className="flex items-center gap-2 min-w-0">
                      <span className={`w-2.5 h-2.5 rounded-full ${rule.enabled ? 'bg-emerald-500' : 'bg-slate-600'}`} />
                      <h4 className="text-sm font-semibold text-white truncate" title={rule.name}>
                        {rule.name}
                      </h4>
                    </div>

                    <div className="flex items-center gap-2">
                      <span className={`px-2 py-0.5 rounded text-[11px] font-medium border ${
                        isLocal 
                          ? 'bg-blue-600/10 border-blue-500/30 text-blue-400' 
                          : 'bg-purple-600/10 border-purple-500/30 text-purple-400'
                      }`}>
                        {isLocal ? '本地转发 (-L)' : '远程转发 (-R)'}
                      </span>
                      <button
                        onClick={() => handleToggle(rule.id)}
                        className={`w-10 h-5 rounded-full transition-colors relative ${
                          rule.enabled ? 'bg-blue-600' : 'bg-slate-800'
                        }`}
                      >
                        <div
                          className={`w-3.5 h-3.5 rounded-full bg-white transition-transform absolute top-0.5 ${
                            rule.enabled ? 'left-6' : 'left-0.5'
                          }`}
                        />
                      </button>
                    </div>
                  </div>

                  {/* Visual Topology Diagram */}
                  <div className="my-4 p-3 bg-slate-950/70 border border-slate-800/80 rounded-xl flex items-center justify-between text-xs font-mono">
                    <div className="text-left">
                      <div className="text-[10px] text-slate-500 uppercase font-sans">本地端口</div>
                      <div className="text-white font-semibold">{rule.local_host}:{rule.local_port}</div>
                    </div>

                    <div className="flex flex-col items-center px-2">
                      <div className="flex items-center gap-1 text-slate-400 font-sans text-[11px]">
                        {isLocal ? (
                          <>
                            <span className="text-blue-400">转发至</span>
                            <ArrowRight className="w-3.5 h-3.5 text-blue-400" />
                          </>
                        ) : (
                          <>
                            <ArrowLeft className="w-3.5 h-3.5 text-purple-400" />
                            <span className="text-purple-400">暴露自</span>
                          </>
                        )}
                      </div>
                      <div className="text-[10px] text-slate-500 font-sans mt-0.5 max-w-[100px] truncate" title={profile?.name || '未知节点'}>
                        via {profile?.name || 'SSH'}
                      </div>
                    </div>

                    <div className="text-right">
                      <div className="text-[10px] text-slate-500 uppercase font-sans">目标地址</div>
                      <div className="text-white font-semibold">{rule.remote_host}:{rule.remote_port}</div>
                    </div>
                  </div>
                </div>

                {/* Card Actions Footer */}
                <div className="flex items-center justify-between pt-2 border-t border-slate-800/40 text-xs text-slate-400">
                  <span className="text-[11px] text-slate-500">
                    SSH: {profile ? `${profile.host}:${profile.port}` : '未指定'}
                  </span>
                  <div className="flex items-center gap-1">
                    <button
                      onClick={() => handleOpenEdit(rule)}
                      className="p-1.5 hover:text-slate-200 hover:bg-slate-800 rounded-lg transition-all"
                      title="编辑"
                    >
                      <Edit3 className="w-4 h-4" />
                    </button>
                    <button
                      onClick={() => handleDelete(rule.id)}
                      className="p-1.5 hover:text-rose-400 hover:bg-rose-950/30 rounded-lg transition-all"
                      title="删除"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {/* Add / Edit Dialog Modal */}
      {showModal && (
        <div className="fixed inset-0 z-50 bg-black/70 backdrop-blur-sm flex items-center justify-center p-4">
          <div className="bg-slate-900 border border-slate-800 rounded-2xl w-full max-w-md p-6 shadow-2xl space-y-4">
            <div className="flex items-center justify-between pb-3 border-b border-slate-800">
              <h3 className="text-sm font-semibold text-white">
                {editingRule ? '编辑端口转发规则' : '新建端口转发规则'}
              </h3>
              <button
                onClick={() => setShowModal(false)}
                className="text-slate-400 hover:text-slate-200"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="space-y-3.5 text-xs">
              <div>
                <label className="block text-slate-400 mb-1 font-medium">规则备注名称</label>
                <input
                  type="text"
                  value={name}
                  onChange={e => setName(e.target.value)}
                  placeholder="例如: 直连生产 MySQL"
                  className="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500"
                />
              </div>

              <div>
                <label className="block text-slate-400 mb-1 font-medium">转发类型</label>
                <div className="grid grid-cols-2 gap-2">
                  <button
                    type="button"
                    onClick={() => setForwardType('local')}
                    className={`px-3 py-2 rounded-lg text-xs font-medium border text-left transition-all ${
                      forwardType === 'local'
                        ? 'bg-blue-600/20 border-blue-500 text-blue-300'
                        : 'bg-slate-950 border-slate-800 text-slate-400 hover:text-slate-200'
                    }`}
                  >
                    <div className="font-semibold">本地转发 (-L)</div>
                    <div className="text-[10px] opacity-75 mt-0.5">本地访问远端内网服务</div>
                  </button>
                  <button
                    type="button"
                    onClick={() => setForwardType('remote')}
                    className={`px-3 py-2 rounded-lg text-xs font-medium border text-left transition-all ${
                      forwardType === 'remote'
                        ? 'bg-purple-600/20 border-purple-500 text-purple-300'
                        : 'bg-slate-950 border-slate-800 text-slate-400 hover:text-slate-200'
                    }`}
                  >
                    <div className="font-semibold">远程转发 (-R)</div>
                    <div className="text-[10px] opacity-75 mt-0.5">暴露本地服务至远端</div>
                  </button>
                </div>
              </div>

              <div>
                <label className="block text-slate-400 mb-1 font-medium">SSH 连接 (从节点中选择)</label>
                <select
                  value={profileId}
                  onChange={e => setProfileId(e.target.value)}
                  className="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500"
                >
                  {profiles.map(p => (
                    <option key={p.id} value={p.id}>
                      {p.name} ({p.host}:{p.port})
                    </option>
                  ))}
                </select>
                {profiles.length === 0 && (
                  <p className="text-[11px] text-rose-400 mt-1">请先在「连接配置」中添加至少一个 SSH 节点</p>
                )}
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-slate-400 mb-1 font-medium">本地绑定地址</label>
                  <input
                    type="text"
                    value={localHost}
                    onChange={e => setLocalHost(e.target.value)}
                    placeholder="127.0.0.1"
                    className="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500"
                  />
                </div>
                <div>
                  <label className="block text-slate-400 mb-1 font-medium">本地监听端口</label>
                  <input
                    type="number"
                    value={localPort}
                    onChange={e => setLocalPort(e.target.value)}
                    placeholder="例如: 13306"
                    className="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500"
                  />
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="block text-slate-400 mb-1 font-medium">目标主机地址</label>
                  <input
                    type="text"
                    value={remoteHost}
                    onChange={e => setRemoteHost(e.target.value)}
                    placeholder="127.0.0.1"
                    className="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500"
                  />
                </div>
                <div>
                  <label className="block text-slate-400 mb-1 font-medium">目标服务端口</label>
                  <input
                    type="number"
                    value={remotePort}
                    onChange={e => setRemotePort(e.target.value)}
                    placeholder="例如: 3306"
                    className="w-full bg-slate-950 border border-slate-800 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-blue-500"
                  />
                </div>
              </div>
            </div>

            <div className="flex items-center justify-end gap-2 pt-3 border-t border-slate-800">
              <button
                type="button"
                onClick={() => setShowModal(false)}
                className="px-4 py-2 text-xs font-medium text-slate-400 hover:text-slate-200"
              >
                取消
              </button>
              <button
                type="button"
                onClick={handleSave}
                className="px-5 py-2 bg-blue-600 hover:bg-blue-500 text-white text-xs font-medium rounded-lg shadow-lg shadow-blue-900/30 transition-all"
              >
                保存
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
