import { BookIcon, SettingsIcon } from "./Icons";

type NavbarProps = {
  onOpenSettings: () => void;
  onOpenTerminology: () => void;
};

export default function Navbar({ onOpenSettings, onOpenTerminology }: NavbarProps) {
  return (
    <nav className="apple-navbar">
      <div className="apple-navbar-content">
        <h1 className="apple-heading-small">
          VoxTrans
          <span className="app-version">v0.1.0</span>
        </h1>
        <div className="nav-buttons">
          <button className="nav-button" onClick={onOpenTerminology}>
            <BookIcon />
            <span>术语</span>
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


