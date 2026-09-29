import { createContext, useContext } from 'react';
import { Settings } from './types';

export type UiLanguage = 'zh' | 'en';

export const I18nContext = createContext<UiLanguage>('zh');

export function resolveLanguage(language: Settings['language'] | undefined): UiLanguage {
  if (language === 'english') return 'en';
  if (language === 'chinese') return 'zh';
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

export function translate(language: UiLanguage, chinese: string, english: string) {
  return language === 'en' ? english : chinese;
}

export function useI18n() {
  const language = useContext(I18nContext);
  return {
    language,
    tr: (chinese: string, english: string) => translate(language, chinese, english),
  };
}
