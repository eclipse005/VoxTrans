/**
 * i18next bootstrap.
 *
 * Initialized synchronously with the default locale (zh-CN) so the first
 * paint is already localized. After user preferences load, the app calls
 * `changeAppLanguage()` to switch to the persisted locale.
 *
 * Resources are statically imported (bundled, no network) — appropriate for
 * an offline-capable desktop app.
 */
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import {
  DEFAULT_LOCALE,
  NAMESPACES,
  resources,
  type AppLocale,
} from "./resources";

export type { AppLocale } from "./resources";
export { DEFAULT_LOCALE } from "./resources";

/**
 * Switch the active UI locale at runtime and keep the document lang attr
 * in sync (for accessibility / screen readers). All components using
 * `useTranslation()` re-render automatically.
 */
export async function changeAppLanguage(locale: AppLocale): Promise<void> {
  await i18n.changeLanguage(locale);
  document.documentElement.lang = locale;
}

// Synchronous init — runs before React renders so the first paint is localized.
void i18n.use(initReactI18next).init({
  resources,
  lng: DEFAULT_LOCALE,
  fallbackLng: DEFAULT_LOCALE,
  ns: NAMESPACES,
  defaultNS: "common",
  interpolation: {
    // React already escapes interpolated values, so i18next should not.
    escapeValue: false,
  },
  returnNull: false,
});

document.documentElement.lang = DEFAULT_LOCALE;

export default i18n;
