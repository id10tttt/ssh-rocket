import React, { useState } from 'react';
import { Edit3, Eye, EyeOff, KeyRound, Plus, Rocket, Trash2 } from 'lucide-react';
import { AppConfig, Profile } from '../types';
import {
  Button,
  Card,
  Dialog,
  EmptyState,
  Field,
  IconButton,
  PageHeader,
  StatusDot,
} from '../components/Adwaita';
import { useI18n } from '../i18n';

interface ConnectViewProps {
  config: AppConfig | null;
  isRunning: boolean;
  statusText: string;
  onStart: (profileId?: string) => void;
  onStop: () => void;
  onSaveConfig: (cfg: AppConfig) => void;
}

type AuthKind = 'password' | 'private_key';

const emptyProfile = (): Profile => ({
  id: crypto.randomUUID(),
  name: '',
  host: '',
  port: 22,
  username: '',
  auth_type: 'key',
  password: null,
  identity_file: '~/.ssh/id_ed25519',
});

export const ConnectView: React.FC<ConnectViewProps> = ({
  config,
  isRunning,
  statusText,
  onStart,
  onStop,
  onSaveConfig,
}) => {
  const { tr } = useI18n();
  const [draft, setDraft] = useState<Profile | null>(null);
  const [showSecret, setShowSecret] = useState(false);
  const [error, setError] = useState('');

  const profiles = config?.profiles ?? [];
  const activeId = config?.active_profile ?? profiles[0]?.id ?? null;

  const getAuthKind = (profile: Profile): AuthKind =>
    profile.auth_type === 'password' ? 'password' : 'private_key';

  const getAuthValue = (profile: Profile) => {
    if (getAuthKind(profile) === 'password') return profile.password ?? '';
    return profile.identity_file ?? '';
  };

  const setAuth = (kind: AuthKind, value: string) => {
    setDraft((current) => current ? {
      ...current,
      auth_type: kind === 'password' ? 'password' : 'key',
      password: kind === 'password' ? value : null,
      identity_file: kind === 'private_key' ? value : null,
    } : current);
  };

  const saveProfile = () => {
    if (!config || !draft) return;
    if (!draft.host.trim()) {
      setError(tr('服务器地址不能为空', 'Server address cannot be empty'));
      return;
    }
    const exists = profiles.some((profile) => profile.id === draft.id);
    const nextProfiles = exists
      ? profiles.map((profile) => profile.id === draft.id ? draft : profile)
      : [...profiles, draft];
    onSaveConfig({
      ...config,
      profiles: nextProfiles,
      active_profile: config.active_profile ?? draft.id,
    });
    setDraft(null);
  };

  const removeProfile = (profileId: string) => {
    if (!config) return;
    const nextProfiles = profiles.filter((profile) => profile.id !== profileId);
    onSaveConfig({
      ...config,
      profiles: nextProfiles,
      active_profile: activeId === profileId ? nextProfiles[0]?.id ?? null : activeId,
    });
  };

  const selectProfile = (profileId: string) => {
    if (!config) return;
    onSaveConfig({ ...config, active_profile: profileId });
  };

  return (
    <div className="app-page">
      <PageHeader title={tr('节点连接', 'Connections')}>
        <IconButton
          label={tr('添加连接', 'Add Connection')}
          onClick={() => {
            setError('');
            setDraft(emptyProfile());
          }}
        >
          <Plus />
        </IconButton>
      </PageHeader>

      {profiles.length === 0 ? (
        <div className="app-page-content is-centered">
          <EmptyState
            icon={<Rocket />}
            title={tr('暂无节点配置', 'No Nodes Configured')}
            description={tr('添加 SSH 节点服务器以开启透明代理', 'Add an SSH server to enable transparent proxy')}
          >
            <Button variant="suggested" onClick={() => setDraft(emptyProfile())}>
              {tr('添加连接', 'Add Connection')}
            </Button>
          </EmptyState>
        </div>
      ) : (
        <div className="app-page-content">
          <div className="card-grid connection-grid">
            {profiles.map((profile) => {
              const isActive = profile.id === activeId;
              const isThisRunning = isActive && isRunning;
              return (
                <Card key={profile.id} className={isActive ? 'connection-card is-active' : 'connection-card'}>
                  <div className="card-title-row">
                    <StatusDot state={isThisRunning ? 'connected' : 'disconnected'} />
                    <h3>{profile.name || tr('未命名', 'Untitled')}</h3>
                    {isActive && <span className="badge proxy">{tr('默认', 'Default')}</span>}
                    <IconButton
                      label={tr('编辑', 'Edit')}
                      disabled={isThisRunning}
                      onClick={() => {
                        setError('');
                        setDraft({ ...profile });
                      }}
                    >
                      <Edit3 />
                    </IconButton>
                    <IconButton
                      label={tr('删除', 'Delete')}
                      disabled={isThisRunning}
                      onClick={() => removeProfile(profile.id)}
                    >
                      <Trash2 />
                    </IconButton>
                  </div>
                  <dl className="connection-details">
                    <div><dt>{tr('服务器', 'Server')}</dt><dd>{profile.host}:{profile.port}</dd></div>
                    <div><dt>{tr('用户名', 'Username')}</dt><dd>{profile.username || '—'}</dd></div>
                    <div><dt>{tr('认证', 'Authentication')}</dt><dd>{getAuthKind(profile) === 'password' ? tr('密码认证', 'Password') : tr('私钥认证', 'Private Key')}</dd></div>
                  </dl>
                  <div className="connection-actions">
                    {!isActive && (
                      <Button variant="flat" onClick={() => selectProfile(profile.id)}>{tr('设为默认', 'Set as Default')}</Button>
                    )}
                    <span className="dim-label">{isThisRunning ? statusText : tr('未连接', 'Disconnected')}</span>
                    {isThisRunning ? (
                      <Button variant="destructive" onClick={onStop}>{tr('断开连接', 'Disconnect')}</Button>
                    ) : (
                      <Button variant="suggested" onClick={() => onStart(profile.id)}>{tr('连接', 'Connect')}</Button>
                    )}
                  </div>
                </Card>
              );
            })}
          </div>
        </div>
      )}

      {draft && (
        <Dialog
          title={profiles.some((profile) => profile.id === draft.id) ? tr('编辑连接', 'Edit Connection') : tr('新建连接', 'New Connection')}
          onClose={() => setDraft(null)}
          footer={(
            <>
              <Button onClick={() => setDraft(null)}>{tr('取消', 'Cancel')}</Button>
              <Button variant="suggested" onClick={saveProfile}>{tr('保存', 'Save')}</Button>
            </>
          )}
        >
          <div className="connection-form">
            <Field label={tr('名称', 'Name')}>
              <input
                value={draft.name}
                onChange={(event) => setDraft({ ...draft, name: event.target.value })}
              />
            </Field>
            <div className="form-grid-host">
              <Field label={tr('服务器地址', 'Server Address')} error={error}>
                <input
                  value={draft.host}
                  autoFocus
                  onChange={(event) => {
                    setError('');
                    setDraft({ ...draft, host: event.target.value });
                  }}
                />
              </Field>
              <Field label={tr('端口', 'Port')}>
                <input
                  type="number"
                  min={1}
                  max={65535}
                  value={draft.port}
                  onChange={(event) => setDraft({ ...draft, port: Number(event.target.value) || 22 })}
                />
              </Field>
            </div>
            <Field label={tr('用户名', 'Username')}>
              <input value={draft.username} onChange={(event) => setDraft({ ...draft, username: event.target.value })} />
            </Field>
            <Field label={tr('认证方式', 'Authentication')}>
              <select
                value={getAuthKind(draft)}
                onChange={(event) => setAuth(event.target.value as AuthKind, '')}
              >
                <option value="private_key">{tr('私钥认证', 'Private Key')}</option>
                <option value="password">{tr('密码认证', 'Password')}</option>
              </select>
            </Field>
            <Field label={getAuthKind(draft) === 'password' ? tr('SSH 密码', 'SSH Password') : tr('私钥', 'Private Key')}>
              <div className="secret-entry">
                <input
                  type={getAuthKind(draft) === 'password' && !showSecret ? 'password' : 'text'}
                  value={getAuthValue(draft)}
                  onChange={(event) => setAuth(getAuthKind(draft), event.target.value)}
                />
                {getAuthKind(draft) === 'password' ? (
                  <IconButton label={showSecret ? tr('隐藏密码', 'Hide Password') : tr('显示密码', 'Show Password')} onClick={() => setShowSecret(!showSecret)}>
                    {showSecret ? <EyeOff /> : <Eye />}
                  </IconButton>
                ) : <KeyRound aria-hidden="true" />}
              </div>
            </Field>
          </div>
        </Dialog>
      )}
    </div>
  );
};
