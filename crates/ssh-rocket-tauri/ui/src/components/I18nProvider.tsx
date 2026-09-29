import React from 'react';
import { I18nContext, UiLanguage } from '../i18n';

export function I18nProvider({
  language,
  children,
}: React.PropsWithChildren<{ language: UiLanguage }>) {
  return <I18nContext.Provider value={language}>{children}</I18nContext.Provider>;
}
