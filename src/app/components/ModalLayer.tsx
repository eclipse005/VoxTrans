import type { ExportSrtItem } from "../api/transcribe";
import type { UpdateCheckResult } from "../api/updater";
import type { SettingsForm } from "../hooks/useSettingsController";
import type { AppAction } from "../state/appReducer";
import type { ToastState } from "../types";
import LogsModal from "./LogsModal";
import SettingsModal from "./SettingsModal";
import SubtitleExportModal from "./SubtitleExportModal";
import TerminologyModal from "./TerminologyModal";
import Toast from "./Toast";
import UpdateModal from "./UpdateModal";

type LogChannel = "main" | "llm";

type ModalLayerProps = {
  showSettings: boolean;
  showLogs: boolean;
  showTerminologyModal: boolean;
  showSubtitleExportModal: boolean;
  showUpdateDialog: boolean;
  canExportTranslated: boolean;
  savedExportItems: ExportSrtItem[];
  form: SettingsForm;
  toast: ToastState | null;
  logTaskName: string;
  logContent: string;
  logChannel: LogChannel;
  loadingLogs: boolean;
  totalTokens: number;
  availableUpdate: UpdateCheckResult | null;
  installing: boolean;
  installProgress: number | null;
  dispatch: (action: AppAction) => void;
  setForm: React.Dispatch<React.SetStateAction<SettingsForm>>;
  setShowTerminologyModal: (visible: boolean) => void;
  setShowSubtitleExportModal: (visible: boolean) => void;
  setSavedExportItems: (items: ExportSrtItem[]) => void;
  saveExportItems: (items: ExportSrtItem[]) => void;
  saveTerminologyGroups: (groups: SettingsForm["terminologyGroups"]) => Promise<void>;
  exportSubtitleSrt: (items: ExportSrtItem[]) => Promise<void>;
  loadLogs: () => void | Promise<void>;
  setLogChannel: (channel: LogChannel) => void;
  clearLogs: () => void | Promise<void>;
  openLogDir: () => void | Promise<void>;
  closeUpdateDialog: () => void;
  installUpdate: () => void | Promise<void>;
  cancelInstall: () => void | Promise<void>;
  skipVersion: () => void | Promise<void>;
};

export function ModalLayer({
  showSettings,
  showLogs,
  showTerminologyModal,
  showSubtitleExportModal,
  showUpdateDialog,
  canExportTranslated,
  savedExportItems,
  form,
  toast,
  logTaskName,
  logContent,
  logChannel,
  loadingLogs,
  totalTokens,
  availableUpdate,
  installing,
  installProgress,
  dispatch,
  setForm,
  setShowTerminologyModal,
  setShowSubtitleExportModal,
  setSavedExportItems,
  saveExportItems,
  saveTerminologyGroups,
  exportSubtitleSrt,
  loadLogs,
  setLogChannel,
  clearLogs,
  openLogDir,
  closeUpdateDialog,
  installUpdate,
  cancelInstall,
  skipVersion,
}: ModalLayerProps) {
  return (
    <>
      <SettingsModal
        visible={showSettings}
        onClose={() => dispatch({ type: "set_ui", payload: { showSettings: false } })}
      />

      <LogsModal
        visible={showLogs}
        loading={loadingLogs}
        totalTokens={totalTokens}
        taskName={logTaskName}
        content={logContent}
        channel={logChannel}
        onChannelChange={setLogChannel}
        onClose={() => dispatch({ type: "set_ui", payload: { showLogs: false } })}
        onRefresh={loadLogs}
        onClear={clearLogs}
        onOpenDir={openLogDir}
      />

      <TerminologyModal
        visible={showTerminologyModal}
        groups={form.terminologyGroups}
        onClose={() => setShowTerminologyModal(false)}
        onChange={(value) => setForm((prev) => ({ ...prev, terminologyGroups: value }))}
        onSave={saveTerminologyGroups}
      />

      {showSubtitleExportModal ? (
        <SubtitleExportModal
          canExportTranslated={canExportTranslated}
          initialSelectedItems={savedExportItems}
          onClose={() => setShowSubtitleExportModal(false)}
          onConfirm={async (items) => {
            setSavedExportItems(items);
            saveExportItems(items);
            await exportSubtitleSrt(items);
            setShowSubtitleExportModal(false);
          }}
        />
      ) : null}

      <UpdateModal
        visible={showUpdateDialog}
        update={availableUpdate}
        installing={installing}
        installProgress={installProgress}
        onClose={closeUpdateDialog}
        onInstall={installUpdate}
        onCancelInstall={cancelInstall}
        onSkipVersion={skipVersion}
      />

      <Toast toast={toast} />
    </>
  );
}
