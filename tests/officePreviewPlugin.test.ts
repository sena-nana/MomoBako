/**
 * Office 预览插件测试。
 *
 * 验证 PDF 直读链路与 Office 转 PDF 后预览链路，
 * 确保前端统一走 PDF 预览与缩略图生成逻辑。
 */
import { describe, expect, it, vi, afterEach, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/vue";
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
  const originalDevicePixelRatio = window.devicePixelRatio;

  beforeEach(() => {
    pdfjsState.reset();
    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: 1,
    });
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
    Object.defineProperty(window, "devicePixelRatio", {
      configurable: true,
      value: originalDevicePixelRatio,
    });
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

  it("renders pdf preview pages and allows page navigation for direct pdf files", async () => {
    let component: unknown;
    const getPageCalls: number[] = [];
    const renderCalls: number[] = [];
    class MockPdfDocument {
      #pagePromises: number[] = [];
      numPages = 2;

      async getPage(pageNumber: number) {
        this.#pagePromises.push(pageNumber);
        getPageCalls.push(pageNumber);
        return {
          getViewport: ({ scale }: { scale: number }) => ({
            width: 480 * scale,
            height: 640 * scale,
          }),
          render: () => {
            renderCalls.push(pageNumber);
            return {
              promise: Promise.resolve(),
            };
          },
        };
      }

      async destroy() {
        return undefined;
      }
    }
    pdfjsState.getDocument.mockImplementation(() => ({
      promise: Promise.resolve(new MockPdfDocument()),
    }));
    const preparePreviewFileSource = vi.fn(async () => ({
      repoId: "repo-main-001",
      path: "Docs/demo.pdf",
      token: "2".repeat(64),
      sourceUrl: "http://127.0.0.1:49152/preview/pdf-view-token",
      mediaType: "application/pdf",
      sizeBytes: 8192,
      modifiedAt: "2026-07-01T08:00:00Z",
    }));
    const callPlugin = vi.fn();
    const prepareRepositoryCacheFilePreviewSource = vi.fn();
    const saveGeneratedThumbnail = vi.fn();

    const { register } = await import("../External/Plugins/office-preview/src/register.js");
    register({
      preparePreviewFileSource,
      callPlugin,
      prepareRepositoryCacheFilePreviewSource,
      saveGeneratedThumbnail,
      registerPreview: (definition: { component?: unknown }) => {
        component = definition.component;
        return definition;
      },
      pdfRuntime: pdfjsState,
      vue: await import("vue"),
    });

    render(component as never, {
      props: {
        repoId: "repo-main-001",
        entry: fileEntry("pdf"),
      },
    });

    expect(await screen.findByText("2 页 PDF")).toBeInTheDocument();
    expect(await screen.findByText("1 / 2")).toBeInTheDocument();
    await waitFor(() => {
      expect(renderCalls).toContain(1);
    });
    expect(screen.getByRole("button", { name: "上一页" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "下一页" })).toBeEnabled();

    await screen.getByRole("button", { name: "下一页" }).click();

    expect(await screen.findByText("2 / 2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "下一页" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "上一页" })).toBeEnabled();
    expect(getPageCalls).toContain(1);
    expect(getPageCalls).toContain(2);
    expect(callPlugin).not.toHaveBeenCalled();
    expect(prepareRepositoryCacheFilePreviewSource).not.toHaveBeenCalled();
  });

  it("renders converted office preview pages after officeConvert resolves a cached pdf", async () => {
    let component: unknown;
    const getPageCalls: number[] = [];
    pdfjsState.getDocument.mockImplementation(() => ({
      promise: Promise.resolve({
        numPages: 3,
        getPage: async (pageNumber: number) => {
          getPageCalls.push(pageNumber);
          return {
            getViewport: ({ scale }: { scale: number }) => ({
              width: 480 * scale,
              height: 640 * scale,
            }),
            render: () => ({
              promise: Promise.resolve(),
            }),
          };
        },
        destroy: async () => undefined,
      }),
    }));
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
      token: "3".repeat(64),
      sourceUrl: "http://127.0.0.1:49152/preview/office-preview-token",
      mediaType: "application/pdf",
      sizeBytes: 4096,
      modifiedAt: "2026-07-01T08:00:00Z",
    }));
    const saveGeneratedThumbnail = vi.fn();

    const { register } = await import("../External/Plugins/office-preview/src/register.js");
    register({
      preparePreviewFileSource,
      callPlugin,
      prepareRepositoryCacheFilePreviewSource,
      saveGeneratedThumbnail,
      registerPreview: (definition: { component?: unknown }) => {
        component = definition.component;
        return definition;
      },
      pdfRuntime: pdfjsState,
      vue: await import("vue"),
    });

    render(component as never, {
      props: {
        repoId: "repo-main-001",
        entry: fileEntry("pptx"),
      },
    });

    expect(await screen.findByText("3 页 PDF")).toBeInTheDocument();
    expect(await screen.findByText("1 / 3")).toBeInTheDocument();
    expect(callPlugin).toHaveBeenCalledWith({
      pluginId: "momobako.service.office-convert",
      method: "officeConvert.ensurePreviewPdf",
      payload: {
        repoId: "repo-main-001",
        entryPath: "Docs/demo.pptx",
        extension: "pptx",
        sourcePath: "C:/Mock/Repo/Docs/demo.pptx",
        sourceModifiedAt: "2026-07-01T08:00:00Z",
        sourceSizeBytes: 8192,
      },
    });
    expect(prepareRepositoryCacheFilePreviewSource).toHaveBeenCalledWith({
      repoId: "repo-main-001",
      path: "C:/Mock/Repo/.momo/cache/office-preview/demo.pdf",
      mediaType: "application/pdf",
    });

    await screen.getByRole("button", { name: "下一页" }).click();

    expect(await screen.findByText("2 / 3")).toBeInTheDocument();
    expect(getPageCalls).toContain(1);
    expect(getPageCalls).toContain(2);
    expect(preparePreviewFileSource).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(saveGeneratedThumbnail).toHaveBeenCalledWith({
        repoId: "repo-main-001",
        path: "Docs/demo.pptx",
        imageBytes: expect.any(Array),
        mediaType: "image/jpeg",
      });
    });
  });

  it("renders a clear error when office conversion fails", async () => {
    let component: unknown;
    const preparePreviewFileSource = vi.fn();
    const callPlugin = vi.fn(async () => {
      throw new Error("LibreOffice 转换失败：守护进程不可用");
    });
    const prepareRepositoryCacheFilePreviewSource = vi.fn();

    const { register } = await import("../External/Plugins/office-preview/src/register.js");
    register({
      preparePreviewFileSource,
      callPlugin,
      prepareRepositoryCacheFilePreviewSource,
      registerPreview: (definition: { component?: unknown }) => {
        component = definition.component;
        return definition;
      },
      pdfRuntime: pdfjsState,
      vue: await import("vue"),
    });

    render(component as never, {
      props: {
        repoId: "repo-main-001",
        entry: fileEntry("pptx"),
      },
    });

    expect(await screen.findByText("无法预览该文档")).toBeInTheDocument();
    expect(await screen.findByText("LibreOffice 转换失败：守护进程不可用")).toBeInTheDocument();
    expect(callPlugin).toHaveBeenCalledWith({
      pluginId: "momobako.service.office-convert",
      method: "officeConvert.ensurePreviewPdf",
      payload: {
        repoId: "repo-main-001",
        entryPath: "Docs/demo.pptx",
        extension: "pptx",
        sourcePath: "C:/Mock/Repo/Docs/demo.pptx",
        sourceModifiedAt: "2026-07-01T08:00:00Z",
        sourceSizeBytes: 8192,
      },
    });
    expect(prepareRepositoryCacheFilePreviewSource).not.toHaveBeenCalled();
  });

  it("renders a clear error when cached pdf preview source is unavailable", async () => {
    let component: unknown;
    const preparePreviewFileSource = vi.fn();
    const callPlugin = vi.fn(async () => ({
      payload: {
        pdfPath: "C:/Mock/Repo/.momo/cache/office-preview/demo.pdf",
        cached: false,
        converter: "microsoft-office",
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
      sourceUrl: null,
      mediaType: "application/pdf",
      sizeBytes: 4096,
      modifiedAt: "2026-07-01T08:00:00Z",
    }));

    const { register } = await import("../External/Plugins/office-preview/src/register.js");
    register({
      preparePreviewFileSource,
      callPlugin,
      prepareRepositoryCacheFilePreviewSource,
      registerPreview: (definition: { component?: unknown }) => {
        component = definition.component;
        return definition;
      },
      pdfRuntime: pdfjsState,
      vue: await import("vue"),
    });

    render(component as never, {
      props: {
        repoId: "repo-main-001",
        entry: fileEntry("xlsx"),
      },
    });

    expect(await screen.findByText("无法预览该文档")).toBeInTheDocument();
    expect(await screen.findByText("转换后的 PDF 预览源不可用")).toBeInTheDocument();
    expect(prepareRepositoryCacheFilePreviewSource).toHaveBeenCalledWith({
      repoId: "repo-main-001",
      path: "C:/Mock/Repo/.momo/cache/office-preview/demo.pdf",
      mediaType: "application/pdf",
    });
    expect(pdfjsState.getDocument).not.toHaveBeenCalled();
  });
});
