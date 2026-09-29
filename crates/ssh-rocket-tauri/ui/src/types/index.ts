export type RuleAction = 'direct' | 'proxy' | 'block';

export interface Profile {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_type: 'key' | 'password';
  password: string | null;
  identity_file: string | null;
}

export interface DomainRule {
  pattern: string;
  kind: 'domain' | 'domain-suffix' | 'domain-keyword' | 'legacy';
  action: RuleAction;
}

export interface IpRule {
  network: string;
  action: RuleAction;
}

export interface AppRule {
  executable: string;
  action: RuleAction;
}

export interface Settings {
  dns_server: string;
  default_policy: RuleAction;
  custom_overrides: IpRule[];
  rule_source_url: string;
  rule_source_name: string;
  rule_source_updated_at: number;
  ipv6: boolean;
  theme_mode: 'auto' | 'light' | 'dark';
  language: 'auto' | 'chinese' | 'english';
  floating_widget: {
    enabled: boolean;
    idle_opacity: number;
    fade_delay_secs: number;
    speed_decimals: number;
  };
  app_rules: AppRule[];
  domain_rules: DomainRule[];
  ip_rules: IpRule[];
  imported_domain_rules: DomainRule[];
  imported_ip_rules: IpRule[];
}

export type ForwardType = 'local' | 'remote';

export interface PortForwardRule {
  id: string;
  name: string;
  profile_id: string;
  forward_type: ForwardType;
  local_host: string;
  local_port: number;
  remote_host: string;
  remote_port: number;
  enabled: boolean;
}

export interface AppConfig {
  active_profile: string | null;
  profiles: Profile[];
  port_forwards: PortForwardRule[];
  settings: Settings;
}

export interface DesktopApp {
  name: string;
  executable: string;
  icon: string;
}

export interface SpeedDto {
  upload: number;
  download: number;
}

export type ConnectionType = 'Proxy' | 'Direct' | 'Local';

export interface AppTrafficStat {
  id: string;
  name: string;
  icon: string;
  upload: number;
  download: number;
  proxy_upload: number;
  proxy_download: number;
  direct_upload: number;
  direct_download: number;
  local_upload: number;
  local_download: number;
  primary_type: ConnectionType;
}

export interface ActiveConnectionStat {
  proc_name: string;
  icon: string;
  local_addr: string;
  peer_addr: string;
  conn_type: ConnectionType;
  upload: number;
  download: number;
}

export interface RuntimeStatusDto {
  is_running: boolean;
  status_text: string;
}
