import { invoke } from "~/api/invoke";
import type { ImportScan, ImportSource } from "~/api/types";

export const importsApi = {
  /** Counts conversations `kind` saved on disk that are not threads yet. */
  scanImports: (kind: ImportSource) => invoke<ImportScan>("scan_imports", { kind }),
  /** Imports those conversations as threads; returns how many were added. */
  importThreads: (kinds: ImportSource[]) => invoke<number>("import_threads", { kinds }),
};
