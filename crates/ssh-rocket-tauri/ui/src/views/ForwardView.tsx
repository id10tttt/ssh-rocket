import React, { useState } from 'react';
import { ArrowLeftRight, ArrowRight, Check, Edit3, Plus, Trash2 } from 'lucide-react';
import { AppConfig, ForwardType, PortForwardRule } from '../types';
import {
  Button,
  Card,
  Dialog,
  EmptyState,
  Field,
  IconButton,
  PageHeader,
  StatusDot,
  Switch,
} from '../components/Adwaita';
import { useI18n } from '../i18n';

interface ForwardViewProps {
  config: AppConfig | null;
  onSaveConfig: (cfg: AppConfig) => void;
}

export const ForwardView: React.FC<ForwardViewProps> = ({ config, onSaveConfig }) => {
  const { tr } = useI18n();
  const [showDialog, setShowDialog] = useState(false);
  const [editingRule, setEditingRule] = useState<PortForwardRule | null>(null);
  const [name, setName] = useState('');
  const [forwardType, setForwardType] = useState<ForwardType>('local');
  const [profileId, setProfileId] = useState('');
  const [localHost, setLocalHost] = useState('127.0.0.1');
  const [localPort, setLocalPort] = useState('');
  const [remoteHost, setRemoteHost] = useState('127.0.0.1');
  const [remotePort, setRemotePort] = useState('');
  const [errors, setErrors] = useState<{ localPort?: string; remotePort?: string; profile?: string }>({});

  const rules = config?.port_forwards ?? [];
  const profiles = config?.profiles ?? [];

  const openDialog = (rule?: PortForwardRule) => {
    setEditingRule(rule ?? null);
    setName(rule?.name ?? '');
    setForwardType(rule?.forward_type ?? 'local');
    setProfileId(rule?.profile_id ?? profiles[0]?.id ?? '');
    setLocalHost(rule?.local_host || '127.0.0.1');
    setLocalPort(rule?.local_port ? String(rule.local_port) : '');
    setRemoteHost(rule?.remote_host || '127.0.0.1');
    setRemotePort(rule?.remote_port ? String(rule.remote_port) : '');
    setErrors({});
    setShowDialog(true);
  };

  const handleSave = () => {
    if (!config) return;
    const nextErrors: typeof errors = {};
    const parsedLocalPort = Number(localPort);
    const parsedRemotePort = Number(remotePort);
    if (!Number.isInteger(parsedLocalPort) || parsedLocalPort < 1 || parsedLocalPort > 65535) {
      nextErrors.localPort = tr('请输入 1–65535 之间的端口', 'Enter a port from 1 to 65535');
    }
    if (!Number.isInteger(parsedRemotePort) || parsedRemotePort < 1 || parsedRemotePort > 65535) {
      nextErrors.remotePort = tr('请输入 1–65535 之间的端口', 'Enter a port from 1 to 65535');
    }
    if (!profileId) nextErrors.profile = tr('请先添加 SSH 节点连接', 'Please add an SSH connection first');
    if (Object.keys(nextErrors).length > 0) {
      setErrors(nextErrors);
      return;
    }

    const normalizedLocalHost = localHost.trim() || '127.0.0.1';
    const normalizedRemoteHost = remoteHost.trim() || '127.0.0.1';
    const generatedName = forwardType === 'local'
      ? `${normalizedLocalHost}:${parsedLocalPort} -> ${normalizedRemoteHost}:${parsedRemotePort}`
      : `${normalizedRemoteHost}:${parsedRemotePort} -> ${normalizedLocalHost}:${parsedLocalPort}`;
    const rule: PortForwardRule = {
      id: editingRule?.id ?? crypto.randomUUID(),
      name: name.trim() || generatedName,
      profile_id: profileId,
      forward_type: forwardType,
      local_host: normalizedLocalHost,
      local_port: parsedLocalPort,
      remote_host: normalizedRemoteHost,
      remote_port: parsedRemotePort,
      enabled: editingRule?.enabled ?? true,
    };
    const portForwards = editingRule
      ? rules.map((item) => item.id === editingRule.id ? rule : item)
      : [...rules, rule];
    onSaveConfig({ ...config, port_forwards: portForwards });
    setShowDialog(false);
  };

  const toggleRule = (ruleId: string) => {
    if (!config) return;
    onSaveConfig({
      ...config,
      port_forwards: rules.map((rule) => rule.id === ruleId ? { ...rule, enabled: !rule.enabled } : rule),
    });
  };

  const deleteRule = (ruleId: string) => {
    if (!config) return;
    onSaveConfig({ ...config, port_forwards: rules.filter((rule) => rule.id !== ruleId) });
  };

  const localPortField = (
    <Field label={tr('本地端口', 'Local Port')} error={errors.localPort}>
      <input
        type="number"
        min={1}
        max={65535}
        value={localPort}
        placeholder="8080"
        onChange={(event) => {
          setLocalPort(event.target.value);
          setErrors((current) => ({ ...current, localPort: undefined }));
        }}
      />
    </Field>
  );
  const serverPortField = (
    <Field label={tr('服务器端口', 'Server Port')} error={errors.remotePort}>
      <input
        type="number"
        min={1}
        max={65535}
        value={remotePort}
        placeholder="80"
        onChange={(event) => {
          setRemotePort(event.target.value);
          setErrors((current) => ({ ...current, remotePort: undefined }));
        }}
      />
    </Field>
  );

  return (
    <div className="app-page">
      <PageHeader title={tr('端口转发', 'Port Forwarding')}>
        <IconButton label={tr('添加端口转发', 'Add Forward Rule')} onClick={() => openDialog()}>
          <Plus />
        </IconButton>
      </PageHeader>

      {rules.length === 0 ? (
        <div className="app-page-content is-centered">
          <EmptyState
            icon={<ArrowLeftRight />}
            title={tr('暂无端口转发', 'No Forwarding Rules')}
            description={tr('将本地端口绑定到服务器端口，或将服务器端口绑定到本地端口', 'Map local ports to server ports, or server ports to local ports')}
          >
            <Button variant="suggested" onClick={() => openDialog()}>
              {tr('添加规则', 'Add Rule')}
            </Button>
          </EmptyState>
        </div>
      ) : (
        <div className="app-page-content">
          <div className="card-grid">
            {rules.map((rule) => {
              const isLocal = rule.forward_type === 'local';
              const profile = profiles.find((item) => item.id === rule.profile_id);
              const source = isLocal
                ? `${rule.local_host}:${rule.local_port}`
                : `${rule.remote_host}:${rule.remote_port}`;
              const target = isLocal
                ? `${rule.remote_host}:${rule.remote_port}`
                : `${rule.local_host}:${rule.local_port}`;
              return (
                <Card key={rule.id} className="forward-card">
                  <div className="card-title-row">
                    <StatusDot state={rule.enabled ? 'connected' : 'disconnected'} />
                    <h3 title={rule.name}>{rule.name}</h3>
                    <span className="badge">{isLocal ? tr('本地 → 服务器', 'Local → Server') : tr('服务器 → 本地', 'Server → Local')}</span>
                    <Switch
                      checked={rule.enabled}
                      label={`${rule.name} ${tr('启用状态', 'enabled')}`}
                      onClick={() => toggleRule(rule.id)}
                    />
                    <IconButton label={tr('编辑', 'Edit')} onClick={() => openDialog(rule)}><Edit3 /></IconButton>
                    <IconButton label={tr('删除', 'Delete')} onClick={() => deleteRule(rule.id)}><Trash2 /></IconButton>
                  </div>
                  <div className="forward-topology">
                    <div>
                      <span>{isLocal ? tr('本地端口', 'Local Port') : tr('服务器端口', 'Server Port')}</span>
                      <strong>{source}</strong>
                    </div>
                    <div className="forward-route">
                      <ArrowRight aria-hidden="true" />
                      <span>via {profile?.name ?? 'SSH'}</span>
                    </div>
                    <div className="is-target">
                      <span>{isLocal ? tr('服务器端口', 'Server Port') : tr('本地端口', 'Local Port')}</span>
                      <strong>{target}</strong>
                    </div>
                  </div>
                </Card>
              );
            })}
          </div>
        </div>
      )}

      {showDialog && (
        <Dialog
          title={editingRule ? tr('编辑端口转发', 'Edit Forwarding Rule') : tr('新建端口转发', 'New Forwarding Rule')}
          onClose={() => setShowDialog(false)}
          wide
          footer={(
            <>
              <Button onClick={() => setShowDialog(false)}>{tr('取消', 'Cancel')}</Button>
              <Button variant="suggested" onClick={handleSave}>{tr('保存', 'Save')}</Button>
            </>
          )}
        >
          <div className="forward-form">
            <Field label={tr('SSH 连接', 'SSH Connection')} error={errors.profile}>
              <select
                value={profileId}
                disabled={profiles.length === 0}
                onChange={(event) => {
                  setProfileId(event.target.value);
                  setErrors((current) => ({ ...current, profile: undefined }));
                }}
              >
                {profiles.map((profile) => (
                  <option key={profile.id} value={profile.id}>
                    {profile.name} ({profile.host}:{profile.port})
                  </option>
                ))}
              </select>
            </Field>

            <fieldset className="forward-direction-group">
              <legend>{tr('映射方向', 'Direction')}</legend>
              <div>
                {([
                  ['local', tr('本地 → 服务器', 'Local → Server'), tr('将本地端口绑定到服务器端口', 'Bind a local port to a server port')],
                  ['remote', tr('服务器 → 本地', 'Server → Local'), tr('将服务器端口绑定到本地端口', 'Bind a server port to a local port')],
                ] as const).map(([value, title, description]) => (
                  <button
                    type="button"
                    role="radio"
                    aria-checked={forwardType === value}
                    className={forwardType === value ? 'is-active' : undefined}
                    onClick={() => setForwardType(value)}
                    key={value}
                  >
                    <span><strong>{title}</strong><small>{description}</small></span>
                    {forwardType === value && <Check aria-hidden="true" />}
                  </button>
                ))}
              </div>
            </fieldset>

            <fieldset className="forward-port-group">
              <legend>{tr('端口映射', 'Port Mapping')}</legend>
              <div>
                {forwardType === 'local' ? localPortField : serverPortField}
                <ArrowRight className="forward-port-arrow" aria-hidden="true" />
                {forwardType === 'local' ? serverPortField : localPortField}
              </div>
            </fieldset>

            <details className="forward-advanced">
              <summary>{tr('高级设置', 'Advanced Settings')}</summary>
              <div>
                <Field label={forwardType === 'local' ? tr('本地监听地址', 'Local Listen Address') : tr('本地目标地址', 'Local Target Address')}>
                  <input value={localHost} onChange={(event) => setLocalHost(event.target.value)} />
                </Field>
                <Field label={forwardType === 'local' ? tr('服务器目标地址', 'Server Target Address') : tr('服务器监听地址', 'Server Listen Address')}>
                  <input value={remoteHost} onChange={(event) => setRemoteHost(event.target.value)} />
                </Field>
              </div>
            </details>

            <Field label={tr('名称（可选）', 'Name (Optional)')}>
              <input
                value={name}
                placeholder={tr('留空时自动生成', 'Generated automatically when left blank')}
                onChange={(event) => setName(event.target.value)}
              />
            </Field>
          </div>
        </Dialog>
      )}
    </div>
  );
};
