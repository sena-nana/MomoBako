import { render, screen, waitFor } from "@testing-library/vue";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ThreeModelPreview from "../src/plugins/threeModelPreview/ThreeModelPreview.vue";
import type { FileBrowserEntry } from "../src/types/repository";

const mocks = vi.hoisted(() => {
  class Vector3 {
    x: number;
    y: number;
    z: number;

    constructor(x = 0, y = 0, z = 0) {
      this.x = x;
      this.y = y;
      this.z = z;
    }

    set(x: number, y: number, z: number) {
      this.x = x;
      this.y = y;
      this.z = z;
      return this;
    }

    sub(vector: Vector3) {
      this.x -= vector.x;
      this.y -= vector.y;
      this.z -= vector.z;
      return this;
    }
  }

  class Object3D {
    children: Object3D[] = [];
    material: unknown = null;
    position = new Vector3();
    castShadow = true;
    receiveShadow = true;

    add(child: Object3D) {
      this.children.push(child);
      return this;
    }

    traverse(callback: (child: Object3D) => void) {
      callback(this);
      this.children.forEach((child) => child.traverse(callback));
    }
  }

  class Mesh extends Object3D {}

  class Scene extends Object3D {}

  class PerspectiveCamera extends Object3D {
    fov: number;
    aspect: number;
    near: number;
    far: number;

    constructor(fov: number, aspect: number, near: number, far: number) {
      super();
      this.fov = fov;
      this.aspect = aspect;
      this.near = near;
      this.far = far;
    }

    updateProjectionMatrix = vi.fn();
  }

  class Box3 {
    setFromObject() {
      return this;
    }

    getSize(target: Vector3) {
      return target.set(2, 3, 4);
    }

    getCenter(target: Vector3) {
      return target.set(1, 1.5, 2);
    }
  }

  class Color {
    value: number;

    constructor(value: number) {
      this.value = value;
    }
  }

  class AmbientLight extends Object3D {}

  class DirectionalLight extends Object3D {}

  class GridHelper extends Object3D {}

  class MeshStandardMaterial {
    parameters: unknown;

    constructor(parameters: unknown) {
      this.parameters = parameters;
    }
  }

  const rendererInstances: Array<{
    backend: { isWebGPUBackend: boolean };
    domElement: HTMLCanvasElement;
    init: ReturnType<typeof vi.fn>;
    render: ReturnType<typeof vi.fn>;
    dispose: ReturnType<typeof vi.fn>;
    setPixelRatio: ReturnType<typeof vi.fn>;
    setClearColor: ReturnType<typeof vi.fn>;
    setSize: ReturnType<typeof vi.fn>;
  }> = [];
  const gltfLoadResolvers: Array<(object: Object3D) => void> = [];
  const frameCallbacks: FrameRequestCallback[] = [];

  const preparePreviewFileSource = vi.fn(async ({ path }: { path: string }) => ({
    repoId: "repo-main-001",
    path,
    token: "0".repeat(64),
    sourceUrl: `http://127.0.0.1:49152/preview/${path}`,
    mediaType: path.endsWith(".glb") ? "model/gltf-binary" : "application/octet-stream",
    sizeBytes: 1024,
    modifiedAt: "2026-06-05T00:18:00Z",
  }));
  const saveGeneratedWorkspaceEntryThumbnail = vi.fn(async () => null);
  const upsertTask = vi.fn();
  const removeTask = vi.fn();
  const deepDispose = vi.fn();
  const rotateVRM0 = vi.fn();
  const controlsDispose = vi.fn();
  const controlsUpdate = vi.fn();
  const mtoonMaterialLoaderPlugin = vi.fn();
  const vrmLoaderPlugin = vi.fn();
  const vrmMetaLoaderPlugin = vi.fn();
  let rendererBackend: "webgpu" | "webgl2" = "webgpu";
  let delayNextGltfLoad = false;

  return {
    AmbientLight,
    Box3,
    Color,
    DirectionalLight,
    GridHelper,
    Mesh,
    MeshStandardMaterial,
    Object3D,
    PerspectiveCamera,
    Scene,
    Vector3,
    controlsDispose,
    controlsUpdate,
    deepDispose,
    delayNextGltfLoad: () => {
      delayNextGltfLoad = true;
    },
    frameCallbacks,
    gltfLoadResolvers,
    mtoonMaterialLoaderPlugin,
    preparePreviewFileSource,
    removeTask,
    rendererInstances,
    reset: () => {
      preparePreviewFileSource.mockClear();
      saveGeneratedWorkspaceEntryThumbnail.mockClear();
      upsertTask.mockClear();
      removeTask.mockClear();
      deepDispose.mockClear();
      rotateVRM0.mockClear();
      controlsDispose.mockClear();
      controlsUpdate.mockClear();
      mtoonMaterialLoaderPlugin.mockClear();
      vrmLoaderPlugin.mockClear();
      vrmMetaLoaderPlugin.mockClear();
      rendererInstances.length = 0;
      gltfLoadResolvers.length = 0;
      frameCallbacks.length = 0;
      rendererBackend = "webgpu";
      delayNextGltfLoad = false;
    },
    resolveNextGltfLoad: (object = new Object3D()) => {
      const resolve = gltfLoadResolvers.shift();
      resolve?.(object);
    },
    rotateVRM0,
    runAnimationFrames: () => {
      const callbacks = frameCallbacks.splice(0);
      callbacks.forEach((callback) => callback(performance.now()));
    },
    saveGeneratedWorkspaceEntryThumbnail,
    setRendererBackend: (backend: "webgpu" | "webgl2") => {
      rendererBackend = backend;
    },
    upsertTask,
    vrmLoaderPlugin,
    vrmMetaLoaderPlugin,
    WebGPURenderer: class {
      backend = { isWebGPUBackend: rendererBackend === "webgpu" };
      domElement: HTMLCanvasElement;
      init = vi.fn(async () => undefined);
      render = vi.fn();
      dispose = vi.fn();
      setPixelRatio = vi.fn();
      setClearColor = vi.fn();
      setSize = vi.fn();

      constructor({ canvas }: { canvas: HTMLCanvasElement }) {
        this.domElement = canvas;
        rendererInstances.push(this);
      }
    },
    GLTFLoader: class {
      register = vi.fn((factory: (parser: { json: unknown }) => unknown) => {
        factory({ json: { extensions: { VRMC_vrm: { meta: { licenseUrl: "https://example.com/license" } } } } });
      });

      async loadAsync() {
        if (delayNextGltfLoad) {
          delayNextGltfLoad = false;
          return new Promise((resolve) => {
            gltfLoadResolvers.push((object) => resolve({ scene: object, userData: {} }));
          });
        }
        return { scene: new Object3D(), userData: {} };
      }
    },
    FBXLoader: class {
      async loadAsync() {
        return new Object3D();
      }
    },
    OBJLoader: class {
      async loadAsync() {
        return new Object3D();
      }
    },
  };
});

vi.mock("three/webgpu", () => ({
  AmbientLight: mocks.AmbientLight,
  Box3: mocks.Box3,
  Color: mocks.Color,
  DirectionalLight: mocks.DirectionalLight,
  GridHelper: mocks.GridHelper,
  Mesh: mocks.Mesh,
  MeshStandardMaterial: mocks.MeshStandardMaterial,
  Object3D: mocks.Object3D,
  PerspectiveCamera: mocks.PerspectiveCamera,
  Scene: mocks.Scene,
  Vector3: mocks.Vector3,
  WebGPURenderer: mocks.WebGPURenderer,
}));

vi.mock("@pixiv/three-vrm", () => ({
  MToonMaterialLoaderPlugin: mocks.mtoonMaterialLoaderPlugin,
  VRMLoaderPlugin: mocks.vrmLoaderPlugin,
  VRMMetaLoaderPlugin: mocks.vrmMetaLoaderPlugin,
  VRMUtils: {
    deepDispose: mocks.deepDispose,
    rotateVRM0: mocks.rotateVRM0,
  },
}));

vi.mock("@pixiv/three-vrm/nodes", () => ({
  MToonNodeMaterial: class {},
}));

vi.mock("three/examples/jsm/controls/OrbitControls.js", () => ({
  OrbitControls: class {
    enableDamping = false;
    dampingFactor = 0;
    target = new mocks.Vector3();
    dispose = mocks.controlsDispose;
    update = mocks.controlsUpdate;
  },
}));

vi.mock("three/examples/jsm/loaders/FBXLoader.js", () => ({
  FBXLoader: mocks.FBXLoader,
}));

vi.mock("three/examples/jsm/loaders/GLTFLoader.js", () => ({
  GLTFLoader: mocks.GLTFLoader,
}));

vi.mock("three/examples/jsm/loaders/OBJLoader.js", () => ({
  OBJLoader: mocks.OBJLoader,
}));

vi.mock("../src/services/repositoryApi", () => ({
  preparePreviewFileSource: mocks.preparePreviewFileSource,
}));

vi.mock("../src/composables/useRepositoryWorkspace", () => ({
  useRepositoryWorkspace: () => ({
    saveGeneratedWorkspaceEntryThumbnail: mocks.saveGeneratedWorkspaceEntryThumbnail,
  }),
}));

vi.mock("../src/composables/useTaskCenter", () => ({
  useTaskCenter: () => ({
    removeTask: mocks.removeTask,
    upsertTask: mocks.upsertTask,
  }),
}));

class MockResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
}

function fileEntry(path: string, extension: string): FileBrowserEntry {
  return {
    path,
    name: path.split("/").at(-1) ?? path,
    kind: "file",
    extension,
    sizeBytes: 1024,
    sizeLabel: "1 KB",
    modifiedAt: "2026-06-05T00:18:00Z",
    assetId: `asset-${extension}`,
    status: "synced",
    thumbnailPath: null,
    thumbnailCustom: false,
    metadata: {},
  };
}

async function flushPreview() {
  await Promise.resolve();
  mocks.runAnimationFrames();
  await Promise.resolve();
  mocks.runAnimationFrames();
  await Promise.resolve();
}

describe("ThreeModelPreview", () => {
  beforeEach(() => {
    mocks.reset();
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      mocks.frameCallbacks.push(callback);
      return mocks.frameCallbacks.length;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    Object.defineProperty(HTMLCanvasElement.prototype, "toBlob", {
      configurable: true,
      value: (callback: BlobCallback) => callback(new Blob([new Uint8Array([1, 2, 3])], { type: "image/jpeg" })),
    });
  });

  it("renders through the WebGPU backend and persists a generated thumbnail", async () => {
    const wrapper = render(ThreeModelPreview, {
      props: {
        entry: fileEntry("Characters/avatar.glb", "glb"),
        repoId: "repo-main-001",
      },
    });

    await waitFor(() => expect(screen.getByText("GLB")).toBeInTheDocument());
    await flushPreview();

    await waitFor(() => {
      expect(mocks.saveGeneratedWorkspaceEntryThumbnail).toHaveBeenCalledWith(
        "Characters/avatar.glb",
        [1, 2, 3],
        "image/jpeg",
      );
    });
    expect(mocks.rendererInstances[0].init).toHaveBeenCalledTimes(1);
    expect(wrapper.container.querySelector(".model-preview__hud")).toHaveTextContent("2.00 x 3.00 x 4.00");
    expect(wrapper.container.querySelector(".model-preview")).toHaveAttribute("data-renderer-backend", "webgpu");
  });

  it("records the WebGL2 fallback backend when WebGPU is unavailable", async () => {
    mocks.setRendererBackend("webgl2");
    const wrapper = render(ThreeModelPreview, {
      props: {
        entry: fileEntry("Characters/fallback.glb", "glb"),
        repoId: "repo-main-001",
      },
    });

    await waitFor(() => expect(screen.getByText("GLB")).toBeInTheDocument());

    expect(wrapper.container.querySelector(".model-preview")).toHaveAttribute("data-renderer-backend", "webgl2");
  });

  it("uses the WebGPU-compatible MToon material plugin for VRM files", async () => {
    render(ThreeModelPreview, {
      props: {
        entry: fileEntry("Characters/avatar.vrm", "vrm"),
        repoId: "repo-main-001",
      },
    });

    await waitFor(() => expect(screen.getByText("VRM")).toBeInTheDocument());

    expect(mocks.mtoonMaterialLoaderPlugin).toHaveBeenCalledTimes(1);
    expect(mocks.vrmLoaderPlugin).toHaveBeenCalledTimes(1);
  });

  it("disposes stale model loads and does not save thumbnails for expired tokens", async () => {
    mocks.delayNextGltfLoad();
    const { rerender } = render(ThreeModelPreview, {
      props: {
        entry: fileEntry("Characters/old.glb", "glb"),
        repoId: "repo-main-001",
      },
    });
    await waitFor(() => expect(mocks.gltfLoadResolvers).toHaveLength(1));

    await rerender({
      entry: fileEntry("Characters/new.glb", "glb"),
      repoId: "repo-main-001",
    });
    await waitFor(() => expect(screen.getByText("GLB")).toBeInTheDocument());
    const staleObject = new mocks.Object3D();
    mocks.resolveNextGltfLoad(staleObject);
    await flushPreview();

    await waitFor(() => {
      expect(mocks.saveGeneratedWorkspaceEntryThumbnail).toHaveBeenCalledWith(
        "Characters/new.glb",
        [1, 2, 3],
        "image/jpeg",
      );
    });
    expect(mocks.saveGeneratedWorkspaceEntryThumbnail).not.toHaveBeenCalledWith(
      "Characters/old.glb",
      expect.anything(),
      expect.anything(),
    );
    expect(mocks.deepDispose).toHaveBeenCalledWith(staleObject);
  });
});
