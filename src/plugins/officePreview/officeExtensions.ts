export const pdfPreviewExtensions = ["pdf"] as const;

export const wordPreviewExtensions = ["docx", "docm", "doc", "dotx", "dotm", "dot"] as const;

export const spreadsheetPreviewExtensions = ["xlsx", "xlsm", "xlsb", "xls", "xltx", "xltm", "xlt"] as const;

export const presentationPreviewExtensions = ["pptx", "pptm", "ppt", "ppsx", "ppsm", "pps", "potx", "potm", "pot"] as const;

export const officePreviewExtensions = [
  ...pdfPreviewExtensions,
  ...wordPreviewExtensions,
  ...spreadsheetPreviewExtensions,
  ...presentationPreviewExtensions,
];

export const vueOfficePreviewExtensions = ["docx", "xlsx", "pdf"] as const;
export const pptxPreviewExtensions = ["pptx"] as const;

export type OfficePreviewKind = "pdf" | "word" | "spreadsheet" | "presentation" | "office";

export function getOfficePreviewKind(extension?: string | null): OfficePreviewKind {
  const normalized = extension?.toLowerCase() ?? "";
  if (pdfPreviewExtensions.includes(normalized as typeof pdfPreviewExtensions[number])) return "pdf";
  if (wordPreviewExtensions.includes(normalized as typeof wordPreviewExtensions[number])) return "word";
  if (spreadsheetPreviewExtensions.includes(normalized as typeof spreadsheetPreviewExtensions[number])) return "spreadsheet";
  if (presentationPreviewExtensions.includes(normalized as typeof presentationPreviewExtensions[number])) return "presentation";
  return "office";
}

export function isVueOfficePreviewExtension(extension?: string | null) {
  const normalized = extension?.toLowerCase() ?? "";
  return vueOfficePreviewExtensions.includes(normalized as typeof vueOfficePreviewExtensions[number]);
}

export function isPptxPreviewExtension(extension?: string | null) {
  const normalized = extension?.toLowerCase() ?? "";
  return pptxPreviewExtensions.includes(normalized as typeof pptxPreviewExtensions[number]);
}

export function isOpenXmlOfficeExtension(extension?: string | null) {
  const normalized = extension?.toLowerCase() ?? "";
  return [
    "docx",
    "docm",
    "dotx",
    "dotm",
    "xlsx",
    "xlsm",
    "xltx",
    "xltm",
    "pptx",
    "pptm",
    "ppsx",
    "ppsm",
    "potx",
    "potm",
  ].includes(normalized);
}

export function officeKindLabel(kind: OfficePreviewKind) {
  if (kind === "pdf") return "PDF";
  if (kind === "word") return "Word";
  if (kind === "spreadsheet") return "Excel";
  if (kind === "presentation") return "PowerPoint";
  return "Office";
}
