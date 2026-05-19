type DeleteRemoteWithLocalPreparationArgs<TPrepared> = {
  prepareLocal: () => TPrepared;
  deleteRemote: () => Promise<void>;
  commitLocal: (prepared: TPrepared) => void | Promise<void>;
  rollbackLocal?: (prepared: TPrepared, error: unknown) => void | Promise<void>;
};

export async function deleteRemoteWithLocalPreparation<TPrepared>({
  prepareLocal,
  deleteRemote,
  commitLocal,
  rollbackLocal,
}: DeleteRemoteWithLocalPreparationArgs<TPrepared>): Promise<void> {
  const prepared = prepareLocal();
  try {
    await deleteRemote();
  } catch (error) {
    await rollbackLocal?.(prepared, error);
    throw error;
  }
  await commitLocal(prepared);
}

export async function deleteRemoteBeforeLocalMutation(
  deleteRemote: () => Promise<void>,
  mutateLocal: () => void,
): Promise<void> {
  await deleteRemoteWithLocalPreparation({
    prepareLocal: () => undefined,
    deleteRemote,
    commitLocal: mutateLocal,
  });
}
