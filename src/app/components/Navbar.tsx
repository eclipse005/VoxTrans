import { BookIcon, SettingsIcon } from "./Icons";
import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { useTranslation } from "react-i18next";
import type { AppLocale } from "../../i18n";

type NavbarProps = {
  onOpenSettings: () => void;
  onOpenTerminology: () => void;
  hasAvailableUpdate: boolean;
  onOpenUpdateDialog: () => void;
  currentLocale: AppLocale;
  onToggleLocale: () => void;
};

export default function Navbar({
  onOpenSettings,
  onOpenTerminology,
  hasAvailableUpdate,
  onOpenUpdateDialog,
  currentLocale,
  onToggleLocale,
}: NavbarProps) {
  const { t } = useTranslation(["common"]);
  const [version, setVersion] = useState(import.meta.env.VITE_APP_VERSION ?? "0.0.0");

  useEffect(() => {
    let mounted = true;

    const loadVersion = async () => {
      try {
        const appVersion = await getVersion();
        if (mounted && appVersion) {
          setVersion(appVersion);
        }
      } catch {
        // Ignore failures in non-Tauri contexts.
      }
    };

    void loadVersion();

    return () => {
      mounted = false;
    };
  }, []);

  return (
    <nav className="apple-navbar">
      <div className="apple-navbar-content">
        <h1 className="apple-heading-small">
          {t("common:appName")}
          {hasAvailableUpdate ? (
            <button
              type="button"
              className="app-version-btn"
              onClick={onOpenUpdateDialog}
              title={t("common:nav.newVersionAvailable")}
            >
              <span className="app-version has-update">
                v{version}
                <span className="app-version-dot" aria-hidden="true" />
              </span>
            </button>
          ) : (
            <span className="app-version">v{version}</span>
          )}
        </h1>
        <div className="nav-buttons">
          <button className="nav-button" onClick={onOpenTerminology}>
            <BookIcon />
            <span>{t("common:nav.terminology")}</span>
          </button>
          <button className="nav-button" onClick={onOpenSettings}>
            <SettingsIcon />
            <span>{t("common:nav.settings")}</span>
          </button>
          <button
            type="button"
            className="nav-lang-button"
            onClick={onToggleLocale}
            aria-label={t("common:nav.toggleLanguage")}
            title={t("common:nav.toggleLanguage")}
          >
            {currentLocale === "zh-CN" ? "中" : "EN"}
          </button>
        </div>
      </div>
    </nav>
  );
}
