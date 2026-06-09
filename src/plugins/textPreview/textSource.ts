import { preparePreviewFileSource, readFile } from "../../services/repositoryApi";

export const TEXT_PREVIEW_BYTE_LIMIT = 768 * 1024;
export const TEXT_THUMBNAIL_BYTE_LIMIT = 24 * 1024;

export type TextPreviewContent = {
  text: string;
  truncated: boolean;
  sizeBytes: number;
  bytesRead: number;
  mediaType: string;
  modifiedAt?: string | null;
};

export async function loadTextPreviewContent(
  repoId: string,
  path: string,
  byteLimit = TEXT_PREVIEW_BYTE_LIMIT,
): Promise<TextPreviewContent> {
  try {
    const source = await preparePreviewFileSource({ repoId, path });
    if (source.sizeBytes <= 0) {
      return {
        text: "",
        truncated: false,
        sizeBytes: source.sizeBytes,
        bytesRead: 0,
        mediaType: source.mediaType,
        modifiedAt: source.modifiedAt,
      };
    }
    if (!source.sourceUrl) {
      throw new Error("文本预览源不可用");
    }

    const bytes = await fetchPreviewBytes(source.sourceUrl, source.sizeBytes, byteLimit);
    return {
      text: decodeTextBytes(bytes),
      truncated: source.sizeBytes > bytes.byteLength,
      sizeBytes: source.sizeBytes,
      bytesRead: bytes.byteLength,
      mediaType: source.mediaType,
      modifiedAt: source.modifiedAt,
    };
  } catch {
    const bytes = await readFile({ repoId, path });
    const previewBytes = Uint8Array.from(bytes.slice(0, byteLimit));
    return {
      text: decodeTextBytes(previewBytes),
      truncated: bytes.length > previewBytes.byteLength,
      sizeBytes: bytes.length,
      bytesRead: previewBytes.byteLength,
      mediaType: "text/plain",
      modifiedAt: null,
    };
  }
}

async function fetchPreviewBytes(sourceUrl: string, sizeBytes: number, byteLimit: number) {
  const end = Math.max(Math.min(sizeBytes, byteLimit) - 1, 0);
  const response = await fetch(sourceUrl, {
    headers: {
      Range: `bytes=0-${end}`,
    },
  });
  if (!response.ok) {
    throw new Error(`文本预览读取失败: ${response.status}`);
  }
  return new Uint8Array(await response.arrayBuffer()).slice(0, byteLimit);
}

function decodeTextBytes(bytes: Uint8Array) {
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    return new TextDecoder("utf-8").decode(bytes.slice(3));
  }
  if (bytes.length >= 2 && bytes[0] === 0xff && bytes[1] === 0xfe) {
    return new TextDecoder("utf-16le").decode(bytes.slice(2));
  }
  if (bytes.length >= 2 && bytes[0] === 0xfe && bytes[1] === 0xff) {
    return new TextDecoder("utf-16be").decode(bytes.slice(2));
  }
  return new TextDecoder("utf-8").decode(bytes);
}
