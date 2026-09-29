import React from 'react';
import clsx from 'clsx';
import { X } from 'lucide-react';
import { useI18n } from '../i18n';

export function PageHeader({ title, children }: React.PropsWithChildren<{ title: string }>) {
  return (
    <header className="adw-headerbar">
      <div className="adw-headerbar-spacer" />
      <h2>{title}</h2>
      <div className="adw-headerbar-actions">{children}</div>
    </header>
  );
}

export function Button({
  variant = 'default',
  className,
  type = 'button',
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'default' | 'suggested' | 'destructive' | 'flat';
}) {
  return (
    <button
      type={type}
      className={clsx('adw-button', `adw-button-${variant}`, className)}
      {...props}
    />
  );
}

export function IconButton({
  label,
  children,
  className,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      type="button"
      className={clsx('adw-icon-button', className)}
      aria-label={label}
      title={label}
      {...props}
    >
      {children}
    </button>
  );
}

export function Card({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={clsx('adw-card', className)} {...props} />;
}

export function StatusDot({ state }: { state: 'connected' | 'connecting' | 'disconnected' }) {
  return <span className={clsx('adw-status-dot', `is-${state}`)} aria-hidden="true" />;
}

export function Switch({
  checked,
  label,
  ...props
}: Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'role'> & {
  checked: boolean;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className="adw-switch"
      {...props}
    >
      <span />
    </button>
  );
}

export function PreferenceGroup({
  title,
  description,
  children,
  className,
}: React.PropsWithChildren<{
  title?: string;
  description?: string;
  className?: string;
}>) {
  return (
    <section className={clsx('adw-preferences-group', className)}>
      {title && <h3>{title}</h3>}
      {description && <p className="adw-group-description">{description}</p>}
      <div className="adw-preferences-list">{children}</div>
    </section>
  );
}

export function ActionRow({
  title,
  subtitle,
  prefix,
  children,
  className,
}: React.PropsWithChildren<{
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  prefix?: React.ReactNode;
  className?: string;
}>) {
  return (
    <div className={clsx('adw-action-row', className)}>
      {prefix && <div className="adw-row-prefix">{prefix}</div>}
      <div className="adw-row-copy">
        <div className="adw-row-title">{title}</div>
        {subtitle && <div className="adw-row-subtitle">{subtitle}</div>}
      </div>
      {children && <div className="adw-row-actions">{children}</div>}
    </div>
  );
}

export function Field({
  label,
  error,
  children,
  className,
}: React.PropsWithChildren<{
  label: string;
  error?: string;
  className?: string;
}>) {
  return (
    <label className={clsx('adw-field', className)}>
      <span>{label}</span>
      {children}
      {error && <small className="adw-field-error">{error}</small>}
    </label>
  );
}

export function Dialog({
  title,
  onClose,
  children,
  footer,
  wide = false,
}: React.PropsWithChildren<{
  title: string;
  onClose: () => void;
  footer: React.ReactNode;
  wide?: boolean;
}>) {
  const { tr } = useI18n();
  return (
    <div className="adw-dialog-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className={clsx('adw-dialog', wide && 'is-wide')}
        role="dialog"
        aria-modal="true"
        aria-labelledby="adw-dialog-title"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="adw-dialog-header">
          <h2 id="adw-dialog-title">{title}</h2>
          <IconButton label={tr('关闭', 'Close')} onClick={onClose}>
            <X />
          </IconButton>
        </div>
        <div className="adw-dialog-body">{children}</div>
        <div className="adw-dialog-footer">{footer}</div>
      </section>
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  description,
  children,
}: React.PropsWithChildren<{
  icon: React.ReactNode;
  title: string;
  description: string;
}>) {
  return (
    <div className="adw-empty-state">
      <div className="adw-empty-icon">{icon}</div>
      <h3>{title}</h3>
      <p>{description}</p>
      {children}
    </div>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: Array<{ value: T; label: string; icon?: React.ReactNode }>;
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div className="adw-segmented" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          type="button"
          role="radio"
          aria-checked={value === option.value}
          className={value === option.value ? 'is-active' : undefined}
          onClick={() => onChange(option.value)}
          key={option.value}
        >
          {option.icon}
          {option.label}
        </button>
      ))}
    </div>
  );
}
