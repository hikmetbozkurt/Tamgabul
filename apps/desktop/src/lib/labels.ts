import type { FileKind } from "../bindings/FileKind";
import type { ModelCategory } from "../bindings/ModelCategory";
import type { ModelStatus } from "../bindings/ModelStatus";
import type { IconName } from "../ui/icons";

/** Optional + missing reads "Optional", required + missing reads "Not installed". */
export function statusKey(status: ModelStatus, required: boolean): string {
  if (status === "MISSING") return required ? "status.MISSING" : "status.OPTIONAL";
  return `status.${status}`;
}

export const CATEGORY_ICON: Record<ModelCategory, IconName> = {
  embedding: "layers",
  llm: "chip",
  stt: "speaker",
  tts: "speakerWaves",
};

export const FILE_ICON: Record<FileKind, IconName> = {
  markdown: "file",
  text: "fileText",
  pdf: "fileText",
  image: "image",
  other: "file",
};

export const FILE_KIND_KEY: Record<FileKind, string> = {
  markdown: "file.markdown",
  text: "file.text",
  pdf: "file.pdf",
  image: "file.image",
  other: "file.other",
};
