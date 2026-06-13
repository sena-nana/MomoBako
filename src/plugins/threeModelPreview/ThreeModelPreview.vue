<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch } from "vue";
import {
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
  WebGPURenderer,
} from "three/webgpu";
import { MToonMaterialLoaderPlugin, VRMLoaderPlugin, VRMMetaLoaderPlugin, VRMUtils, type VRM } from "@pixiv/three-vrm";
import { MToonNodeMaterial } from "@pixiv/three-vrm/nodes";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { FBXLoader } from "three/examples/jsm/loaders/FBXLoader.js";
import { GLTFLoader, type GLTFParser } from "three/examples/jsm/loaders/GLTFLoader.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { preparePreviewFileSource } from "../../services/repositoryApi";
import { useRepositoryWorkspace } from "../../composables/useRepositoryWorkspace";
import { useTaskCenter } from "../../composables/useTaskCenter";
import type { FileBrowserEntry } from "../../types/repository";

const props = defineProps<{
  entry: FileBrowserEntry;
  repoId: string;
}>();

const container = ref<HTMLElement | null>(null);
const canvas = ref<HTMLCanvasElement | null>(null);
const state = ref<"idle" | "loading" | "ready" | "error">("idle");
const errorMessage = ref("");
const modelInfo = ref("");
const loadProgress = ref({
  value: 6,
  label: "读取模型",
  detail: "准备读取文件",
  indeterminate: true,
});
const renderer = shallowRef<ModelPreviewRenderer | null>(null);
const { upsertTask, removeTask } = useTaskCenter();
const { saveGeneratedWorkspaceEntryThumbnail } = useRepositoryWorkspace();
let scene: Scene | null = null;
let camera: PerspectiveCamera | null = null;
let controls: OrbitControls | null = null;
let currentVrm: VRM | null = null;
let frameId: number | null = null;
let resizeObserver: ResizeObserver | null = null;
let loadToken = 0;
let previousFrameTime = 0;
let firstFrameRendered = false;
const rendererBackend = ref<"webgpu" | "webgl2" | null>(null);

type ModelPreviewRenderer = WebGPURenderer & {
  backend?: {
    isWebGPUBackend?: boolean;
  };
};

type LoadedModel = {
  object: Object3D;
  vrm?: VRM;
};

const extension = computed(() => props.entry.extension?.toLowerCase() ?? "");

watch(
  [() => props.repoId, () => props.entry.path],
  () => {
    void loadModel();
  },
  { immediate: true },
);

onBeforeUnmount(() => {
  teardown();
});

async function loadModel() {
  const token = ++loadToken;
  state.value = "loading";
  errorMessage.value = "";
  modelInfo.value = "";
  rendererBackend.value = null;
  loadProgress.value = {
    value: 6,
    label: "读取模型",
    detail: "准备读取文件",
    indeterminate: true,
  };
  publishLoadProgress();

  await nextTick();
  if (!canvas.value || !container.value) return;

  teardown();

  try {
    await setupRenderer(token);
    if (token !== loadToken) {
      return;
    }
    const source = await createModelSourceUrl();
    if (token !== loadToken) {
      return;
    }
    loadProgress.value = {
      value: 48,
      label: "解析模型",
      detail: "读取几何与材质",
      indeterminate: true,
    };
    publishLoadProgress();
    const loaded = await loadObjectByExtension(source, extension.value);
    if (token !== loadToken) {
      disposeObject(loaded.object);
      return;
    }
    currentVrm = loaded.vrm ?? null;
    mountObject(loaded.object);
    loadProgress.value = {
      value: 100,
      label: "模型就绪",
      detail: "已完成",
      indeterminate: false,
    };
    removeLoadTaskSoon(token);
    state.value = "ready";
    void persistCanvasThumbnail(token);
  } catch (cause) {
    if (token !== loadToken) return;
    removeLoadTask();
    state.value = "error";
    errorMessage.value = cause instanceof Error ? cause.message : String(cause);
  }
}

async function persistCanvasThumbnail(token: number) {
  await nextTick();
  if (token !== loadToken || !canvas.value || !renderer.value || !scene || !camera) return;
  await waitForFirstFrame(token);
  if (token !== loadToken || !canvas.value || !renderer.value || !scene || !camera) return;
  renderer.value.render(scene, camera);
  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.value?.toBlob(resolve, "image/jpeg", 0.86);
  });
  if (token !== loadToken || !blob) return;
  const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
  await saveGeneratedWorkspaceEntryThumbnail(props.entry.path, bytes, blob.type || "image/jpeg");
}

async function createModelSourceUrl() {
  loadProgress.value = {
    value: 18,
    label: "读取模型",
    detail: props.entry.sizeLabel ? `读取 ${props.entry.sizeLabel}` : "读取模型文件",
    indeterminate: true,
  };
  publishLoadProgress();
  const response = await preparePreviewFileSource({
    repoId: props.repoId,
    path: props.entry.path,
  });
  if (!response.sourceUrl) {
    throw new Error("模型预览源不可用");
  }
  loadProgress.value = {
    value: 42,
    label: "读取模型",
    detail: "建立预览流",
    indeterminate: false,
  };
  publishLoadProgress();
  return response.sourceUrl;
}

async function setupRenderer(token: number) {
  if (!canvas.value || !container.value) return;

  const nextRenderer = new WebGPURenderer({
    canvas: canvas.value,
    antialias: true,
    alpha: true,
  }) as ModelPreviewRenderer;
  nextRenderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  nextRenderer.setClearColor(new Color(0x000000), 0);
  await nextRenderer.init();
  if (token !== loadToken) {
    nextRenderer.dispose();
    return;
  }
  renderer.value = nextRenderer;
  rendererBackend.value = nextRenderer.backend?.isWebGPUBackend ? "webgpu" : "webgl2";

  scene = new Scene();
  scene.add(new AmbientLight(0xffffff, 1.8));

  const keyLight = new DirectionalLight(0xffffff, 2.2);
  keyLight.position.set(4, 6, 5);
  scene.add(keyLight);

  const fillLight = new DirectionalLight(0x9fc5ff, 1.1);
  fillLight.position.set(-5, 3, -4);
  scene.add(fillLight);

  const grid = new GridHelper(8, 16, 0x6b7280, 0x374151);
  grid.position.y = -0.02;
  scene.add(grid);

  camera = new PerspectiveCamera(45, 1, 0.01, 10000);
  camera.position.set(3, 2.2, 4);

  controls = new OrbitControls(camera, nextRenderer.domElement);
  controls.enableDamping = true;
  controls.dampingFactor = 0.08;
  controls.target.set(0, 0.6, 0);

  resizeObserver = new ResizeObserver(resizeRenderer);
  resizeObserver.observe(container.value);
  resizeRenderer();
  previousFrameTime = performance.now();
  firstFrameRendered = false;
  animate(previousFrameTime);
}

function mountObject(object: Object3D) {
  if (!scene || !camera || !controls) return;

  normalizeMaterials(object);
  scene.add(object);

  const bounds = new Box3().setFromObject(object);
  const size = bounds.getSize(new Vector3());
  const center = bounds.getCenter(new Vector3());
  const maxSize = Math.max(size.x, size.y, size.z) || 1;

  object.position.sub(center);
  object.position.y += size.y / 2;

  const distance = maxSize / (2 * Math.tan((camera.fov * Math.PI) / 360));
  camera.near = Math.max(distance / 100, 0.01);
  camera.far = Math.max(distance * 100, 1000);
  camera.position.set(distance * 0.95, distance * 0.72, distance * 1.25);
  camera.updateProjectionMatrix();

  controls.target.set(0, size.y / 2, 0);
  controls.update();

  modelInfo.value = `${Math.max(size.x, 0.01).toFixed(2)} x ${Math.max(size.y, 0.01).toFixed(2)} x ${Math.max(size.z, 0.01).toFixed(2)}`;
}

function normalizeMaterials(object: Object3D) {
  object.traverse((child) => {
    if (!(child instanceof Mesh)) return;
    child.castShadow = false;
    child.receiveShadow = false;
    if (!child.material) {
      child.material = new MeshStandardMaterial({ color: 0xb8c2cc, roughness: 0.72, metalness: 0.08 });
    }
  });
}

async function loadObjectByExtension(source: string, fileExtension: string) {
  const onProgress = (event: ProgressEvent<EventTarget>) => {
    if (!event.lengthComputable || event.total <= 0) {
      loadProgress.value = {
        value: Math.max(loadProgress.value.value, 58),
        label: "解析模型",
        detail: "解析模型结构",
        indeterminate: true,
      };
      publishLoadProgress();
      return;
    }
    const loadedPercent = Math.round((event.loaded / event.total) * 42);
    loadProgress.value = {
      value: Math.min(94, 48 + loadedPercent),
      label: "解析模型",
      detail: "读取几何与材质",
      indeterminate: false,
    };
    publishLoadProgress();
  };

  if (fileExtension === "fbx") {
    return { object: await new FBXLoader().loadAsync(source, onProgress) } satisfies LoadedModel;
  }
  if (fileExtension === "obj") {
    return { object: await new OBJLoader().loadAsync(source, onProgress) } satisfies LoadedModel;
  }
  if (fileExtension === "glb" || fileExtension === "gltf" || fileExtension === "vrm") {
    const loader = new GLTFLoader();
    if (fileExtension === "vrm") {
      loader.register(createVrmLoaderPlugin);
    }
    const result = await loader.loadAsync(source, onProgress);
    const vrm = result.userData.vrm as VRM | undefined;
    if (vrm) {
      VRMUtils.rotateVRM0(vrm);
      return { object: vrm.scene, vrm } satisfies LoadedModel;
    }
    return { object: result.scene };
  }
  throw new Error(`暂不支持 .${fileExtension || "unknown"} 模型预览`);
}

function createVrmLoaderPlugin(parser: GLTFParser) {
  const licenseUrl = getVrmLicenseUrl(parser.json);
  const mtoonMaterialPlugin = new MToonMaterialLoaderPlugin(parser, {
    materialType: MToonNodeMaterial,
  });
  return new VRMLoaderPlugin(parser, {
    mtoonMaterialPlugin,
    metaPlugin: new VRMMetaLoaderPlugin(parser, {
      acceptLicenseUrls: licenseUrl ? [licenseUrl] : undefined,
    }),
  });
}

function getVrmLicenseUrl(json: unknown) {
  if (!json || typeof json !== "object") return null;
  const extensions = (json as { extensions?: unknown }).extensions;
  if (!extensions || typeof extensions !== "object") return null;
  const vrmExtension = (extensions as { VRMC_vrm?: unknown }).VRMC_vrm;
  if (!vrmExtension || typeof vrmExtension !== "object") return null;
  const meta = (vrmExtension as { meta?: unknown }).meta;
  if (!meta || typeof meta !== "object") return null;
  const licenseUrl = (meta as { licenseUrl?: unknown }).licenseUrl;
  return typeof licenseUrl === "string" && licenseUrl ? licenseUrl : null;
}

function resizeRenderer() {
  if (!container.value || !renderer.value || !camera) return;
  const width = Math.max(container.value.clientWidth, 1);
  const height = Math.max(container.value.clientHeight, 1);
  renderer.value.setSize(width, height, false);
  camera.aspect = width / height;
  camera.updateProjectionMatrix();
}

function animate(frameTime = performance.now()) {
  frameId = window.requestAnimationFrame(animate);
  const delta = Math.min(Math.max((frameTime - previousFrameTime) / 1000, 0), 0.1);
  previousFrameTime = frameTime;
  currentVrm?.update(delta);
  controls?.update();
  if (scene && camera) {
    renderer.value?.render(scene, camera);
    firstFrameRendered = true;
  }
}

async function waitForFirstFrame(token: number) {
  if (firstFrameRendered) return;
  await new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => resolve());
  });
  if (token === loadToken && renderer.value && scene && camera && !firstFrameRendered) {
    renderer.value.render(scene, camera);
    firstFrameRendered = true;
  }
}

function loadTaskId() {
  return `preview-model:${props.repoId}:${props.entry.path}`;
}

function publishLoadProgress() {
  upsertTask({
    id: loadTaskId(),
    label: loadProgress.value.label,
    detail: loadProgress.value.detail,
    value: loadProgress.value.value,
    indeterminate: loadProgress.value.indeterminate,
    source: "模型预览",
  });
}

function removeLoadTask() {
  removeTask(loadTaskId());
}

function removeLoadTaskSoon(token: number) {
  window.setTimeout(() => {
    if (token === loadToken) {
      removeLoadTask();
    }
  }, 300);
}

function teardown() {
  removeLoadTask();
  if (frameId !== null) {
    window.cancelAnimationFrame(frameId);
    frameId = null;
  }
  resizeObserver?.disconnect();
  resizeObserver = null;
  controls?.dispose();
  controls = null;
  currentVrm = null;
  scene?.children.forEach((child) => disposeObject(child));
  scene = null;
  renderer.value?.dispose();
  renderer.value = null;
  camera = null;
}

function disposeObject(object: Object3D) {
  VRMUtils.deepDispose(object);
}
</script>

<template>
  <div
    ref="container"
    class="model-preview"
    :data-renderer-backend="rendererBackend"
  >
    <canvas ref="canvas" class="model-preview__canvas" />

    <div v-if="state === 'loading'" class="model-preview__status">
      <span>{{ loadProgress.label }}</span>
      <span>{{ loadProgress.detail }}</span>
    </div>

    <div v-else-if="state === 'error'" class="model-preview__overlay model-preview__overlay--error">
      <strong>无法预览该模型</strong>
      <span>{{ errorMessage }}</span>
    </div>

    <div v-if="state === 'ready'" class="model-preview__hud">
      <span>{{ extension.toUpperCase() }}</span>
      <span v-if="modelInfo">{{ modelInfo }}</span>
    </div>
  </div>
</template>
