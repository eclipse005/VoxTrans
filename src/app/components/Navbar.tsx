import { BookIcon, SettingsIcon } from "./Icons";
import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

type NavbarProps = {
  onOpenSettings: () => void;
  onOpenTerminology: () => void;
  onOpenHotwords: () => void;
  hasAvailableUpdate: boolean;
  onOpenUpdateDialog: () => void;
};

export default function Navbar({
  onOpenSettings,
  onOpenTerminology,
  onOpenHotwords,
  hasAvailableUpdate,
  onOpenUpdateDialog,
}: NavbarProps) {
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
          VoxTrans
          {hasAvailableUpdate ? (
            <button
              type="button"
              className="app-version-btn"
              onClick={onOpenUpdateDialog}
              title="发现新版本，点击查看详情"
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
            <span>术语</span>
          </button>
          <button className="nav-button" onClick={onOpenHotwords}>
            <BookIcon />
            <span>热词</span>
          </button>
          <button className="nav-button" onClick={onOpenSettings}>
            <SettingsIcon />
            <span>设置</span>
          </button>
        </div>
      </div>
    </nav>
  );
}


