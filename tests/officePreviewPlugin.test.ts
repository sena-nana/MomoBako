/**
 * Office 预览插件测试。
 *
 * 验证 PDF 直读链路与 Office 转 PDF 后预览链路，
 * 确保前端统一走 PDF 预览与缩略图生成逻辑。
 */
import { describe, expect, it, vi, afterEach, beforeEach } from "vitest";
import type { FileBrowserEntry } from "../src/types/repository";
const pdfjsState = vi.hoisted(() => {
  const getDocument = vi.fn((url: string) => ({
    promise: Promise.resolve({
      numPages: 2,
      getPage: async () => ({
        getViewport: ({ scale }: { scale: number }) => ({
          width: 480 * scale,
          height: 640 * scale,
        }),
        render: () => ({
          promise: Promise.resolve(),
        }),
      }),
      destroy: async () => undefined,
    }),
  }));
  return {
    getDocument,
    reset: () => {
      getDocument.mockClear();
    },
  };
});

function fileEntry(extension: string): FileBrowserEntry {
  return {
    path: `Docs/demo.${extension}`,
    name: `demo.${extension}`,
    kind: "file",
    extension,
    sizeBytes: 8192,
    sizeLabel: "8 KB",
    modifiedAt: "2026-07-01T08:00:00Z",
    assetId: "asset-office",
    status: "synced",
    thumbnailPath: null,
    thumbnailCustom: false,
    metadata: {},
    localAbsolutePath: `C:/Mock/Repo/Docs/demo.${extension}`,
  };
}

describe("office preview plugin", () => {
  const originalGetContext = HTMLCanvasElement.prototype.getContext;
  const originalToBlob = HTMLCanvasElement.prototype.toBlob;

  beforeEach(() => {
    pdfjsState.reset();
    HTMLCanvasElement.prototype.getContext = vi.fn(() => ({
      setTransform: vi.fn(),
    })) as typeof HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.toBlob = vi.fn((callback: BlobCallback) => {
      callback(new Blob(["thumbnail"], { type: "image/jpeg" }));
    }) as typeof HTMLCanvasElement.prototype.toBlob;
  });

  afterEach(() => {
    HTMLCanvasElement.prototype.getContext = originalGetContext;
    HTMLCanvasElement.prototype.toBlob = originalToBlob;
  });

  it("uses repository preview source directly for pdf thumbnails", async () => {
    let generateThumbnail:
      | ((context: { repoId: string; entry: FileBrowserEntry }) => Promise<{ bytes: number[]; mediaType: string } | null>)
      | undefined;
    const preparePreviewFileSource = vi.fn(async () => ({
      repoId: "repo-main-001",
      path: "Docs/demo.pdf",
      token: "0".repeat(64),
      sourceUrl: "http://127.0.0.1:49152/preview/pdf-token",
      mediaType: "application/pdf",
      sizeBytes: 8192,
      modifiedAt: "2026-07-01T08:00:00Z",
    }));
    const callPlugin = vi.fn();
    const prepareRepositoryCacheFilePreviewSource = vi.fn();

    const { register } = await import("../External/Plugins/office-preview/src/register.js");
    register({
      preparePreviewFileSource,
      callPlugin,
      prepareRepositoryCacheFilePreviewSource,
      registerPreview: (definition: {
        generateThumbnail?: (context: { repoId: string; entry: FileBrowserEntry }) => Promise<{ bytes: number[]; mediaType: string } | null>;
      }) => {
        generateThumbnail = definition.generateThumbnail;
        return definition;
      },
      pdfRuntime: pdfjsState,
      vue: await import("vue"),
    });

    const result = await generateThumbnail?.({
      repoId: "repo-main-001",
      entry: fileEntry("pdf"),
    });

    expect(result?.mediaType).toBe("image/jpeg");
    expect(result?.bytes.length).toBeGreaterThan(0);
    expect(preparePreviewFileSource).toHaveBeenCalledWith({
      repoId: "repo-main-001",
      path: "Docs/demo.pdf",
    });
    expect(callPlugin).not.toHaveBeenCalled();
    expect(prepareRepositoryCacheFilePreviewSource).not.toHaveBeenCalled();
    expect(pdfjsState.getDocument).toHaveBeenCalledWith("http://127.0.0.1:49152/preview/pdf-token");
  });

  it("converts office files to cached pdf before generating thumbnails", async () => {
    let generateThumbnail:
      | ((context: { repoId: string; entry: FileBrowserEntry }) => Promise<{ bytes: number[]; mediaType: string } | null>)
      | undefined;
    const preparePreviewFileSource = vi.fn();
    const callPlugin = vi.fn(async () => ({
      payload: {
        pdfPath: "C:/Mock/Repo/.momo/cache/office-preview/demo.pdf",
        cached: true,
        converter: "libreoffice",
        cacheKey: "office-preview-cache",
        mediaType: "application/pdf",
        sizeBytes: 4096,
        modifiedAt: "2026-07-01T08:00:00Z",
      },
    }));
    const prepareRepositoryCacheFilePreviewSource = vi.fn(async () => ({
      repoId: "repo-main-001",
      path: "C:/Mock/Repo/.momo/cache/office-preview/demo.pdf",
      token: "1".repeat(64),
      sourceUrl: "http://127.0.0.1:49152/preview/office-cache-token",
      mediaType: "application/pdf",
      sizeBytes: 4096,
      modifiedAt: "2026-07-01T08:00:00Z",
    }));

    const { register } = await import("../External/Plugins/office-preview/src/register.js");
    register({
      preparePreviewFileSource,
      callPlugin,
      prepareRepositoryCacheFilePreviewSource,
      registerPreview: (definition: {
        generateThumbnail?: (context: { repoId: string; entry: FileBrowserEntry }) => Promise<{ bytes: number[]; mediaType: string } | null>;
      }) => {
        generateThumbnail = definition.generateThumbnail;
        return definition;
      },
      pdfRuntime: pdfjsState,
      vue: await import("vue"),
    });

    const result = await generateThumbnail?.({
      repoId: "repo-main-001",
      entry: fileEntry("docx"),
    });

    expect(result?.mediaType).toBe("image/jpeg");
    expect(preparePreviewFileSource).not.toHaveBeenCalled();
    expect(callPlugin).toHaveBeenCalledWith({
      pluginId: "momobako.service.office-convert",
      method: "officeConvert.ensurePreviewPdf",
      payload: {
        repoId: "repo-main-001",
        entryPath: "Docs/demo.docx",
        extension: "docx",
        sourcePath: "C:/Mock/Repo/Docs/demo.docx",
        sourceModifiedAt: "2026-07-01T08:00:00Z",
        sourceSizeBytes: 8192,
      },
    });
    expect(prepareRepositoryCacheFilePreviewSource).toHaveBeenCalledWith({
      repoId: "repo-main-001",
      path: "C:/Mock/Repo/.momo/cache/office-preview/demo.pdf",
      mediaType: "application/pdf",
    });
    expect(pdfjsState.getDocument).toHaveBeenCalledWith("http://127.0.0.1:49152/preview/office-cache-token");
  });
});
