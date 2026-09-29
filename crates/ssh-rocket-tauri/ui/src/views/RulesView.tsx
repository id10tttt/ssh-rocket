import React, { useMemo, useState } from 'react';
import { Download, Plus, Search, Trash2 } from 'lucide-react';
import { AppConfig, DesktopApp, DomainRule, RuleAction } from '../types';
import {
  Button,
  Dialog,
  Field,
  IconButton,
  PageHeader,
  PreferenceGroup,
  Segmented,
} from '../components/Adwaita';
import { useI18n } from '../i18n';

interface RulesViewProps {
  config: AppConfig | null;
  desktopApps: DesktopApp[];
  onSaveConfig: (cfg: AppConfig) => void;
  onImportRules: (url: string) => Promise<unknown>;
}

type RulesTab = 'apps' | 'custom' | 'imported';
type RuleKind = 'domain' | 'domain-suffix' | 'domain-keyword';

export const RulesView: React.FC<RulesViewProps> = ({
  config,
  desktopApps,
  onSaveConfig,
  onImportRules,
}) => {
  const { tr } = useI18n();
  const actionLabel: Record<RuleAction, string> = {
    direct: tr('直连', 'Direct'),
    proxy: tr('代理', 'Proxy'),
    block: tr('拦截', 'Block'),
  };
  const [tab, setTab] = useState<RulesTab>('apps');
  const [query, setQuery] = useState('');
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [showImportDialog, setShowImportDialog] = useState(false);
  const [ruleType, setRuleType] = useState<'domain' | 'ip'>('domain');
  const [pattern, setPattern] = useState('');
  const [kind, setKind] = useState<RuleKind>('domain-suffix');
  const [action, setAction] = useState<RuleAction>('proxy');
  const [importUrl, setImportUrl] = useState(config?.settings.rule_source_url ?? '');
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState('');

  const settings = config?.settings;
  const normalizedQuery = query.trim().toLowerCase();
  const filteredApps = useMemo(() => desktopApps.filter((app) =>
    `${app.name} ${app.executable}`.toLowerCase().includes(normalizedQuery),
  ), [desktopApps, normalizedQuery]);
  const customRules = [
    ...(settings?.domain_rules ?? []).map((rule, index) => ({ type: 'domain' as const, rule, index })),
    ...(settings?.ip_rules ?? []).map((rule, index) => ({ type: 'ip' as const, rule, index })),
  ].filter(({ rule }) => JSON.stringify(rule).toLowerCase().includes(normalizedQuery));
  const importedRules = [
    ...(settings?.imported_domain_rules ?? []).map((rule) => ({ type: 'domain' as const, rule })),
    ...(settings?.imported_ip_rules ?? []).map((rule) => ({ type: 'ip' as const, rule })),
  ].filter(({ rule }) => JSON.stringify(rule).toLowerCase().includes(normalizedQuery));

  const updateSettings = (patch: Partial<AppConfig['settings']>) => {
    if (!config) return;
    onSaveConfig({ ...config, settings: { ...config.settings, ...patch } });
  };

  const getAppAction = (executable: string): RuleAction =>
    settings?.app_rules.find((rule) => rule.executable === executable)?.action ?? 'direct';

  const setAppAction = (executable: string, nextAction: RuleAction) => {
    const appRules = (settings?.app_rules ?? []).filter((rule) => rule.executable !== executable);
    updateSettings({ app_rules: [...appRules, { executable, action: nextAction }] });
  };

  const addRule = () => {
    if (!settings || !pattern.trim()) return;
    if (ruleType === 'domain') {
      const rule: DomainRule = { pattern: pattern.trim(), kind, action };
      updateSettings({ domain_rules: [...settings.domain_rules, rule] });
    } else {
      updateSettings({ ip_rules: [...settings.ip_rules, { network: pattern.trim(), action }] });
    }
    setPattern('');
    setShowAddDialog(false);
  };

  const deleteRule = (type: 'domain' | 'ip', index: number) => {
    if (!settings) return;
    if (type === 'domain') {
      updateSettings({ domain_rules: settings.domain_rules.filter((_, itemIndex) => itemIndex !== index) });
    } else {
      updateSettings({ ip_rules: settings.ip_rules.filter((_, itemIndex) => itemIndex !== index) });
    }
  };

  const importRules = async () => {
    if (!importUrl.trim()) return;
    setImporting(true);
    setImportError('');
    try {
      await onImportRules(importUrl.trim());
      setShowImportDialog(false);
    } catch (error) {
      setImportError(String(error));
    } finally {
      setImporting(false);
    }
  };

  return (
    <div className="app-page">
      <PageHeader title={tr('分流规则', 'Rules')} />
      <div className="rules-switcher">
        <Segmented
          value={tab}
          onChange={(value) => {
            setTab(value);
            setQuery('');
          }}
          label={tr('规则类别', 'Rule Category')}
          options={[
            { value: 'apps', label: tr('应用分流', 'Applications') },
            { value: 'custom', label: tr('域名与 IP', 'Domains & IPs') },
            { value: 'imported', label: tr('订阅规则', 'Subscription Rules') },
          ]}
        />
      </div>
      <div className="app-page-content rules-content">
        <div className="toolbar-row">
          <label className="search-entry rules-search">
            <Search aria-hidden="true" />
            <input
              className="adw-search"
              value={query}
              placeholder={tab === 'apps' ? tr('搜索已安装应用', 'Search installed apps') : tr('搜索规则', 'Search rules')}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          <div className="toolbar-actions">
            {tab === 'custom' && (
              <Button variant="suggested" onClick={() => setShowAddDialog(true)}><Plus />{tr('添加规则', 'Add Rule')}</Button>
            )}
            {tab === 'imported' && (
              <Button variant="suggested" onClick={() => setShowImportDialog(true)}><Download />{tr('导入订阅', 'Import Subscription')}</Button>
            )}
          </div>
        </div>

        {tab === 'apps' && (
          <PreferenceGroup title={tr('桌面应用程序', 'Desktop Applications')} className="rules-list-group">
            <div className="table-card">
              {filteredApps.length === 0 ? <div className="list-empty">{tr('没有匹配的应用', 'No matching applications')}</div> : filteredApps.map((app) => (
                <div className="table-row rule-row" key={app.executable}>
                  <div className="app-avatar">{app.name.slice(0, 1).toUpperCase()}</div>
                  <div className="rule-copy"><strong>{app.name}</strong><span>{app.executable}</span></div>
                  <select
                    className="row-select"
                    value={getAppAction(app.executable)}
                    aria-label={`${app.name} ${tr('分流动作', 'routing action')}`}
                    onChange={(event) => setAppAction(app.executable, event.target.value as RuleAction)}
                  >
                    <option value="direct">{tr('直连', 'Direct')}</option>
                    <option value="proxy">{tr('代理', 'Proxy')}</option>
                    <option value="block">{tr('拦截', 'Block')}</option>
                  </select>
                </div>
              ))}
            </div>
          </PreferenceGroup>
        )}

        {tab === 'custom' && (
          <PreferenceGroup title={tr('用户自定义规则', 'Custom Rules')} description={`${customRules.length} ${tr('条规则', 'rules')}`} className="rules-list-group">
            <div className="table-card">
              {customRules.length === 0 ? <div className="list-empty">{tr('暂无自定义规则', 'No custom rules')}</div> : customRules.map(({ type, rule, index }) => {
                const value = 'pattern' in rule ? rule.pattern : rule.network;
                const kindLabel = 'kind' in rule ? rule.kind.toUpperCase() : 'IP-CIDR';
                return (
                  <div className="table-row rule-row" key={`${type}-${index}-${value}`}>
                    <span className="badge">{kindLabel}</span>
                    <div className="rule-copy"><strong className="monospace">{value}</strong></div>
                    <span className={`badge ${rule.action}`}>{actionLabel[rule.action]}</span>
                    <IconButton label={tr('删除规则', 'Delete Rule')} onClick={() => deleteRule(type, index)}><Trash2 /></IconButton>
                  </div>
                );
              })}
            </div>
          </PreferenceGroup>
        )}

        {tab === 'imported' && (
          <PreferenceGroup
            title={tr('远程订阅规则', 'Remote Subscription Rules')}
            description={settings?.rule_source_name || settings?.rule_source_url || tr('尚未配置订阅', 'No subscription configured')}
            className="rules-list-group"
          >
            <div className="table-card">
              {importedRules.length === 0 ? <div className="list-empty">{tr('暂无订阅规则', 'No subscription rules')}</div> : importedRules.slice(0, 250).map(({ type, rule }, index) => {
                const value = 'pattern' in rule ? rule.pattern : rule.network;
                const kindLabel = 'kind' in rule ? rule.kind.toUpperCase() : 'IP-CIDR';
                return (
                  <div className="table-row rule-row" key={`${type}-${index}-${value}`}>
                    <span className="badge">{kindLabel}</span>
                    <div className="rule-copy"><strong className="monospace">{value}</strong></div>
                    <span className={`badge ${rule.action}`}>{actionLabel[rule.action]}</span>
                  </div>
                );
              })}
            </div>
          </PreferenceGroup>
        )}
      </div>

      {showAddDialog && (
        <Dialog
          title={tr('添加规则', 'Add Rule')}
          onClose={() => setShowAddDialog(false)}
          footer={(
            <>
              <Button onClick={() => setShowAddDialog(false)}>{tr('取消', 'Cancel')}</Button>
              <Button variant="suggested" onClick={addRule}>{tr('添加', 'Add')}</Button>
            </>
          )}
        >
          <div className="rule-form">
            <Field label={tr('规则类型', 'Rule Type')}>
              <select value={ruleType} onChange={(event) => setRuleType(event.target.value as 'domain' | 'ip')}>
                <option value="domain">{tr('域名', 'Domain')}</option>
                <option value="ip">IP / CIDR</option>
              </select>
            </Field>
            {ruleType === 'domain' && (
              <Field label={tr('匹配方式', 'Match Type')}>
                <select value={kind} onChange={(event) => setKind(event.target.value as RuleKind)}>
                  <option value="domain-suffix">DOMAIN-SUFFIX</option>
                  <option value="domain">DOMAIN</option>
                  <option value="domain-keyword">DOMAIN-KEYWORD</option>
                </select>
              </Field>
            )}
            <Field label={ruleType === 'domain' ? tr('域名或后缀', 'Domain or Suffix') : 'IP / CIDR'}>
              <input className="monospace" value={pattern} autoFocus onChange={(event) => setPattern(event.target.value)} />
            </Field>
            <Field label={tr('动作', 'Action')}>
              <select value={action} onChange={(event) => setAction(event.target.value as RuleAction)}>
                <option value="proxy">{tr('代理', 'Proxy')}</option>
                <option value="direct">{tr('直连', 'Direct')}</option>
                <option value="block">{tr('拦截', 'Block')}</option>
              </select>
            </Field>
          </div>
        </Dialog>
      )}

      {showImportDialog && (
        <Dialog
          title={tr('导入规则订阅', 'Import Rule Subscription')}
          onClose={() => setShowImportDialog(false)}
          wide
          footer={(
            <>
              <Button disabled={importing} onClick={() => setShowImportDialog(false)}>{tr('取消', 'Cancel')}</Button>
              <Button variant="suggested" disabled={importing} onClick={importRules}>
                {importing ? tr('正在导入…', 'Importing…') : tr('导入', 'Import')}
              </Button>
            </>
          )}
        >
          <Field label={tr('订阅地址', 'Subscription URL')} error={importError}>
            <input className="monospace" value={importUrl} autoFocus onChange={(event) => setImportUrl(event.target.value)} />
          </Field>
        </Dialog>
      )}
    </div>
  );
};
