import { useCallback } from "react";
import { useData } from "../../state/data";
import { useToast } from "../../state/toast";
import { useT } from "../../i18n";
import { toIpcError } from "../../ipc/bridge";

/** Open / show-in-folder with feedback. */
export function useResultActions() {
  const { api } = useData();
  const toast = useToast();
  const t = useT();
  const run = useCallback(
    async (ref: string, reveal: boolean) => {
      try {
        await api.openResult(ref, reveal);
        toast(t(reveal ? "result.revealedToast" : "result.openedToast"));
      } catch (e) {
        toast(t(`error.${toIpcError(e).code}.title`), true);
      }
    },
    [api, toast, t],
  );
  return {
    onOpen: (ref: string) => run(ref, false),
    onReveal: (ref: string) => run(ref, true),
  };
}
