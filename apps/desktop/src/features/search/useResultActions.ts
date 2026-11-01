import { useCallback } from "react";
import { useData } from "../../state/data";
import { useToast } from "../../state/toast";
import { useT } from "../../i18n";
import { toIpcError } from "../../ipc/bridge";

/** Open / show-in-folder with honest feedback (mock mode says so). */
export function useResultActions() {
  const { api } = useData();
  const toast = useToast();
  const t = useT();
  const run = useCallback(
    async (ref: string, reveal: boolean) => {
      try {
        await api.openResult(ref, reveal);
        const msg = t(reveal ? "result.revealedToast" : "result.openedToast");
        toast(api.mode === "mock" ? `${msg} ${t("mock.suffix")}` : msg);
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
