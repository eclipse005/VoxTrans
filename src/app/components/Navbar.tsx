import { BookIcon, LogsIcon, SettingsIcon } from "./Icons";

type NavbarProps = {
  termsCount: number;
  onOpenTerms: () => void;
  onOpenSettings: () => void;
  onOpenLogs: () => void;
};

export default function Navbar({ termsCount, onOpenTerms, onOpenSettings, onOpenLogs }: NavbarProps) {
  return (
    <nav className="apple-navbar">
      <div className="apple-navbar-content">
        <h1 className="apple-heading-small">VoxTrans</h1>
        <div className="nav-buttons">
          <button className="nav-button" onClick={onOpenTerms}>
            <BookIcon />
            <span>术语</span>
            {termsCount > 0 ? <span className="badge show">{termsCount}</span> : null}
          </button>
          <button className="nav-button" onClick={onOpenLogs}>
            <LogsIcon />
            <span>日志</span>
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


