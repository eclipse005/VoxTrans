/**
 * Aggregated i18n resources.
 *
 * Each language bundles all 8 namespaces as JSON modules. Keeping the
 * aggregation in one place lets `index.ts` stay focused on i18next config,
 * and makes it obvious where to add a new language or namespace.
 *
 * Namespace layout:
 *  - common    : shared buttons, status, navigation, generic fallbacks
 *  - settings  : settings modal (model / translation / subtitle / locale)
 *  - tasks     : workspace, task list, queue, upload panel
 *  - subtitles : subtitle editor, subtitle export, glossary
 *  - models    : model download card & progress
 *  - errors    : backend error-code → localized message mapping
 *  - toasts    : toast messages emitted from hooks
 *  - updater   : update modal, version, relative time
 */
import common_zhCN from "./locales/zh-CN/common.json";
import settings_zhCN from "./locales/zh-CN/settings.json";
import tasks_zhCN from "./locales/zh-CN/tasks.json";
import subtitles_zhCN from "./locales/zh-CN/subtitles.json";
import models_zhCN from "./locales/zh-CN/models.json";
import errors_zhCN from "./locales/zh-CN/errors.json";
import toasts_zhCN from "./locales/zh-CN/toasts.json";
import updater_zhCN from "./locales/zh-CN/updater.json";

import common_en from "./locales/en/common.json";
import settings_en from "./locales/en/settings.json";
import tasks_en from "./locales/en/tasks.json";
import subtitles_en from "./locales/en/subtitles.json";
import models_en from "./locales/en/models.json";
import errors_en from "./locales/en/errors.json";
import toasts_en from "./locales/en/toasts.json";
import updater_en from "./locales/en/updater.json";

export const NAMESPACES = [
  "common",
  "settings",
  "tasks",
  "subtitles",
  "models",
  "errors",
  "toasts",
  "updater",
] as const;

export type AppLocale = "zh-CN" | "en";

export const DEFAULT_LOCALE: AppLocale = "zh-CN";

export const resources = {
  "zh-CN": {
    common: common_zhCN,
    settings: settings_zhCN,
    tasks: tasks_zhCN,
    subtitles: subtitles_zhCN,
    models: models_zhCN,
    errors: errors_zhCN,
    toasts: toasts_zhCN,
    updater: updater_zhCN,
  },
  en: {
    common: common_en,
    settings: settings_en,
    tasks: tasks_en,
    subtitles: subtitles_en,
    models: models_en,
    errors: errors_en,
    toasts: toasts_en,
    updater: updater_en,
  },
} as const;
