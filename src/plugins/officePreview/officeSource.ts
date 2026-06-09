import JSZip from "jszip";
import { preparePreviewFileSource, readFile } from "../../services/repositoryApi";
import type { FileBrowserEntry } from "../../types/repository";
import { getOfficePreviewKind, isOpenXmlOfficeExtension, type OfficePreviewKind } from "./officeExtensions";

export const OFFICE_XML_PREVIEW_BYTE_LIMIT = 8 * 1024 * 1024;
const TEXT_ITEM_LIMIT = 36;

export type OfficePreviewSource = {
  repoId: string;
  path: string;
  sourceUrl: string;
  mediaType: string;
  sizeBytes: number;
  modifiedAt?: string | null;
};

export type OfficePreviewDocument = {
  kind: OfficePreviewKind;
  title: string;
  subtitle: string;
  sections: OfficePreviewSection[];
  stats: Array<{ label: string; value: string }>;
  unsupported: boolean;
};

export type OfficePreviewSection = {
  title: string;
  rows: string[][];
};

type SharedStringCell = {
  index: number;
  text: string;
};

export async function prepareOfficePreviewSource(repoId: string, path: string): Promise<OfficePreviewSource> {
  const source = await preparePreviewFileSource({ repoId, path });
  if (!source.sourceUrl) {
    throw new Error("文档预览源不可用");
  }
  return {
    repoId: source.repoId,
    path: source.path,
    sourceUrl: source.sourceUrl,
    mediaType: source.mediaType,
    sizeBytes: source.sizeBytes,
    modifiedAt: source.modifiedAt,
  };
}

export async function loadOfficePreviewDocument(
  repoId: string,
  entry: FileBrowserEntry,
): Promise<OfficePreviewDocument> {
  const kind = getOfficePreviewKind(entry.extension);
  if (!isOpenXmlOfficeExtension(entry.extension)) {
    return createUnsupportedOfficeDocument(entry, kind);
  }

  try {
    const bytes = await loadOfficeBytes(repoId, entry.path);
    const zip = await JSZip.loadAsync(bytes);
    if (kind === "word") return parseWordDocument(zip, entry);
    if (kind === "spreadsheet") return parseSpreadsheetDocument(zip, entry);
    if (kind === "presentation") return parsePresentationDocument(zip, entry);
  } catch {
    return createUnsupportedOfficeDocument(entry, kind);
  }
  return createUnsupportedOfficeDocument(entry, kind);
}

async function loadOfficeBytes(repoId: string, path: string) {
  let source: OfficePreviewSource | null = null;
  try {
    source = await prepareOfficePreviewSource(repoId, path);
  } catch {
    const bytes = await readFile({ repoId, path });
    return Uint8Array.from(bytes).buffer;
  }

  if (source.sizeBytes > OFFICE_XML_PREVIEW_BYTE_LIMIT) {
    throw new Error("文档过大，已跳过结构预览");
  }

  try {
    const response = await fetch(source.sourceUrl);
    if (!response.ok) {
      throw new Error(`文档读取失败: ${response.status}`);
    }
    return await response.arrayBuffer();
  } catch {
    const bytes = await readFile({ repoId, path });
    if (bytes.length > OFFICE_XML_PREVIEW_BYTE_LIMIT) {
      throw new Error("文档过大，已跳过结构预览");
    }
    return Uint8Array.from(bytes).buffer;
  }
}

async function parseWordDocument(zip: JSZip, entry: FileBrowserEntry): Promise<OfficePreviewDocument> {
  const xml = await readZipText(zip, "word/document.xml");
  const rows = extractWordParagraphs(xml)
    .slice(0, TEXT_ITEM_LIMIT)
    .map((line) => [line]);

  return {
    kind: "word",
    title: entry.name,
    subtitle: rows.length ? "文档正文预览" : "未读取到正文文本",
    sections: [
      {
        title: "正文",
        rows: rows.length ? rows : [["空白文档或暂不支持的内容结构"]],
      },
    ],
    stats: [
      { label: "段落", value: String(rows.length) },
      { label: "格式", value: "DOCX" },
    ],
    unsupported: false,
  };
}

async function parseSpreadsheetDocument(zip: JSZip, entry: FileBrowserEntry): Promise<OfficePreviewDocument> {
  const sharedStrings = await readSharedStrings(zip);
  const workbookRels = await readWorkbookRelationships(zip);
  const sheetNames = await readWorkbookSheetNames(zip);
  const sheetPaths = sheetNames
    .map((sheet) => ({
      name: sheet.name,
      path: workbookRels.get(sheet.relationshipId) ?? `xl/worksheets/sheet${sheet.index + 1}.xml`,
    }))
    .slice(0, 3);
  const sections: OfficePreviewSection[] = [];

  for (const sheet of sheetPaths) {
    const xml = await readZipText(zip, sheet.path);
    const rows = parseWorksheetRows(xml, sharedStrings).slice(0, 12);
    sections.push({
      title: sheet.name,
      rows: rows.length ? rows : [["空工作表"]],
    });
  }

  return {
    kind: "spreadsheet",
    title: entry.name,
    subtitle: sections.length ? "工作表数据预览" : "未读取到工作表",
    sections: sections.length ? sections : [{ title: "工作表", rows: [["空工作簿或暂不支持的内容结构"]] }],
    stats: [
      { label: "工作表", value: String(sections.length) },
      { label: "格式", value: (entry.extension || "xlsx").toUpperCase() },
    ],
    unsupported: false,
  };
}

async function parsePresentationDocument(zip: JSZip, entry: FileBrowserEntry): Promise<OfficePreviewDocument> {
  const slidePaths = Object.keys(zip.files)
    .filter((path) => /^ppt\/slides\/slide\d+\.xml$/.test(path))
    .sort((left, right) => slideIndex(left) - slideIndex(right))
    .slice(0, 8);
  const sections: OfficePreviewSection[] = [];

  for (const path of slidePaths) {
    const xml = await readZipText(zip, path);
    const lines = extractXmlTextRuns(xml).slice(0, 12);
    sections.push({
      title: `幻灯片 ${slideIndex(path)}`,
      rows: lines.length ? lines.map((line) => [line]) : [["空白幻灯片"]],
    });
  }

  return {
    kind: "presentation",
    title: entry.name,
    subtitle: sections.length ? "幻灯片文本预览" : "未读取到幻灯片",
    sections: sections.length ? sections : [{ title: "幻灯片", rows: [["空演示文稿或暂不支持的内容结构"]] }],
    stats: [
      { label: "幻灯片", value: String(sections.length) },
      { label: "格式", value: (entry.extension || "pptx").toUpperCase() },
    ],
    unsupported: false,
  };
}

function createUnsupportedOfficeDocument(entry: FileBrowserEntry, kind: OfficePreviewKind): OfficePreviewDocument {
  const extension = (entry.extension || "office").toUpperCase();
  return {
    kind,
    title: entry.name,
    subtitle: "内容预览暂不可用",
    sections: [
      {
        title: "文件",
        rows: [
          ["名称", entry.name],
          ["类型", extension],
          ["大小", entry.sizeLabel || "未知"],
          ["修改时间", entry.modifiedAt ? new Date(entry.modifiedAt).toLocaleString("zh-CN") : "未记录"],
        ],
      },
    ],
    stats: [
      { label: "格式", value: extension },
      { label: "预览", value: "文件信息" },
    ],
    unsupported: true,
  };
}

async function readZipText(zip: JSZip, path: string) {
  const normalizedPath = path.replace(/^\/+/, "");
  const file = zip.file(normalizedPath);
  if (!file) return "";
  return file.async("text");
}

function extractWordParagraphs(xml: string) {
  const paragraphs = xml.match(/<w:p[\s\S]*?<\/w:p>/g) ?? [];
  return paragraphs
    .map(extractXmlTextRuns)
    .map((runs) => compactText(runs.join("")))
    .filter(Boolean);
}

function extractXmlTextRuns(xml: string) {
  const text: string[] = [];
  const pattern = /<(?:\w+:)?t\b[^>]*>([\s\S]*?)<\/(?:\w+:)?t>/g;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(xml))) {
    const value = compactText(decodeXmlEntities(match[1]));
    if (value) text.push(value);
  }
  return text;
}

async function readSharedStrings(zip: JSZip) {
  const xml = await readZipText(zip, "xl/sharedStrings.xml");
  if (!xml) return [];
  const items = xml.match(/<si[\s\S]*?<\/si>/g) ?? [];
  return items.map((item) => compactText(extractXmlTextRuns(item).join("")));
}

async function readWorkbookRelationships(zip: JSZip) {
  const xml = await readZipText(zip, "xl/_rels/workbook.xml.rels");
  const relationships = new Map<string, string>();
  const pattern = /<Relationship\b([^>]*)\/?>/g;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(xml))) {
    const attrs = parseXmlAttributes(match[1]);
    const id = attrs.get("Id");
    const target = attrs.get("Target");
    if (!id || !target) continue;
    relationships.set(id, normalizeWorkbookTarget(target));
  }
  return relationships;
}

async function readWorkbookSheetNames(zip: JSZip) {
  const xml = await readZipText(zip, "xl/workbook.xml");
  const pattern = /<sheet\b([^>]*)\/?>/g;
  const sheets: Array<{ name: string; relationshipId: string; index: number }> = [];
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(xml))) {
    const attrs = parseXmlAttributes(match[1]);
    const relationshipId = attrs.get("r:id") ?? attrs.get("id") ?? "";
    sheets.push({
      name: attrs.get("name") || `Sheet ${sheets.length + 1}`,
      relationshipId,
      index: sheets.length,
    });
  }
  return sheets;
}

function parseWorksheetRows(xml: string, sharedStrings: string[]) {
  const rowXmlItems = xml.match(/<row\b[\s\S]*?<\/row>/g) ?? [];
  return rowXmlItems
    .map((rowXml) => {
      const cells = parseWorksheetCells(rowXml, sharedStrings);
      if (!cells.length) return [];
      const lastIndex = Math.min(Math.max(...cells.map((cell) => cell.index)), 7);
      return Array.from({ length: lastIndex + 1 }, (_, index) => cells.find((cell) => cell.index === index)?.text ?? "");
    })
    .filter((row) => row.some(Boolean));
}

function parseWorksheetCells(rowXml: string, sharedStrings: string[]): SharedStringCell[] {
  const cells: SharedStringCell[] = [];
  const cellPattern = /<c\b([^>]*)>([\s\S]*?)<\/c>/g;
  let match: RegExpExecArray | null;
  while ((match = cellPattern.exec(rowXml))) {
    const attrs = parseXmlAttributes(match[1]);
    const reference = attrs.get("r") ?? "";
    const type = attrs.get("t") ?? "";
    const rawValue = firstXmlTagContent(match[2], "v") || extractXmlTextRuns(match[2]).join("");
    const text = compactText(resolveCellValue(rawValue, type, sharedStrings));
    if (!text) continue;
    cells.push({
      index: columnIndexFromCellReference(reference),
      text,
    });
  }
  return cells;
}

function resolveCellValue(rawValue: string, type: string, sharedStrings: string[]) {
  const value = compactText(decodeXmlEntities(rawValue));
  if (type === "s") {
    const index = Number.parseInt(value, 10);
    return Number.isFinite(index) ? sharedStrings[index] ?? "" : "";
  }
  if (type === "b") return value === "1" ? "TRUE" : "FALSE";
  return value;
}

function firstXmlTagContent(xml: string, localName: string) {
  const pattern = new RegExp(`<(?:\\w+:)?${localName}\\b[^>]*>([\\s\\S]*?)<\\/(?:\\w+:)?${localName}>`);
  return pattern.exec(xml)?.[1] ?? "";
}

function parseXmlAttributes(value: string) {
  const attrs = new Map<string, string>();
  const pattern = /([\w:-]+)="([^"]*)"/g;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(value))) {
    attrs.set(match[1], decodeXmlEntities(match[2]));
  }
  return attrs;
}

function normalizeWorkbookTarget(target: string) {
  const cleanTarget = target.replace(/^\/+/, "");
  if (cleanTarget.startsWith("xl/")) return cleanTarget;
  return `xl/${cleanTarget}`;
}

function columnIndexFromCellReference(reference: string) {
  const letters = (reference.match(/[A-Z]+/i)?.[0] ?? "A").toUpperCase();
  let index = 0;
  for (const letter of letters) {
    index = index * 26 + letter.charCodeAt(0) - 64;
  }
  return Math.max(index - 1, 0);
}

function slideIndex(path: string) {
  return Number.parseInt(path.match(/slide(\d+)\.xml$/)?.[1] ?? "0", 10);
}

function compactText(value: string) {
  return value.replace(/\s+/g, " ").trim();
}

function decodeXmlEntities(value: string) {
  return value
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, "\"")
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, "&");
}
