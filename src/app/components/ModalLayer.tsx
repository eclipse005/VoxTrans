import { lazy, Suspense, useState } from "react";
import type { RefObject } from "react";
import type { ExportSrtItem } from "../api/transcribe";
import type { UpdateCheckResult } from "../api/updater";
import type { SettingsForm } from "../hooks/useSettingsController";
import type { AppAction } from "../state/appReducer";
import type { ToastState } from "../types";
import SubtitleExportModal from "./SubtitleExportModal";
import Toast from "./Toast";

// Code-split heavy modals out of the startup chunk. Each mounts lazily on
// first open and then stays mounted, so state kept while closed behaves
// exactly as with eager imports.
const LogsModal = lazy(() => import("./LogsModal"));
const SettingsModal = lazy(() => import("./SettingsModal"));
const TerminologyModal = lazy(() => import("./TerminologyModal"));
const UpdateModal = lazy(() => import("./UpdateModal"));

function useMountedOnceVisible(visible: boolean): boolean {
  const [mounted, setMounted] = useState(visible);
  // Render-phase state adjustment (React docs pattern): once the modal has
  // been visible it stays mounted so its preserved state behaves as before.
  if (visible && !mounted) {
    setMounted(true);
  }
  return mounted || visible;
}

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
  updateAnchorRef: RefObject<HTMLElement | null>;
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
  updateAnchorRef,
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
  const settingsMounted = useMountedOnceVisible(showSettings);
  const logsMounted = useMountedOnceVisible(showLogs);
  const terminologyMounted = useMountedOnceVisible(showTerminologyModal);
  const updateMounted = useMountedOnceVisible(showUpdateDialog);
  return (
    <>
      {settingsMounted ? (
        <Suspense fallback={null}>
          <SettingsModal
            visible={showSettings}
            onClose={() => dispatch({ type: "set_ui", payload: { showSettings: false } })}
          />
        </Suspense>
      ) : null}

      {logsMounted ? (
        <Suspense fallback={null}>
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
        </Suspense>
      ) : null}

      {terminologyMounted ? (
        <Suspense fallback={null}>
          <TerminologyModal
            visible={showTerminologyModal}
            groups={form.terminologyGroups}
            activeGroupId={form.activeTerminologyGroupId}
            onClose={() => setShowTerminologyModal(false)}
            onChange={(value) => setForm((prev) => ({ ...prev, terminologyGroups: value }))}
            onChangeActiveGroupId={(groupId) =>
              setForm((prev) => ({ ...prev, activeTerminologyGroupId: groupId }))
            }
            onSave={saveTerminologyGroups}
          />
        </Suspense>
      ) : null}

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

      {updateMounted ? (
        <Suspense fallback={null}>
          <UpdateModal
            visible={showUpdateDialog}
            update={availableUpdate}
            installing={installing}
            installProgress={installProgress}
            anchorRef={updateAnchorRef}
            onClose={closeUpdateDialog}
            onInstall={installUpdate}
            onCancelInstall={cancelInstall}
            onSkipVersion={skipVersion}
          />
        </Suspense>
      ) : null}

      <Toast toast={toast} />
    </>
  );
}
