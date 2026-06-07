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
  WebGLRenderer,
} from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { FBXLoader } from "three/examples/jsm/loaders/FBXLoader.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { readFile } from "../../services/repositoryApi";
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
const renderer = shallowRef<WebGLRenderer | null>(null);
let scene: Scene | null = null;
let camera: PerspectiveCamera | null = null;
let controls: OrbitControls | null = null;
let frameId: number | null = null;
let resizeObserver: ResizeObserver | null = null;
let objectUrl: string | null = null;
let loadToken = 0;

const extension = computed(() => props.entry.extension?.toLowerCase() ?? "");

watch(
  () => [props.repoId, props.entry.path],
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

  await nextTick();
  if (!canvas.value || !container.value) return;

  teardown();
  setupRenderer();

  try {
    const source = await createModelObjectUrl();
    if (token !== loadToken) {
      URL.revokeObjectURL(source);
      return;
    }
    objectUrl = source;
    const object = await loadObjectByExtension(source, extension.value);
    if (token !== loadToken) {
      if (objectUrl === source) {
        URL.revokeObjectURL(source);
        objectUrl = null;
      }
      disposeObject(object);
      return;
    }
    mountObject(object);
    state.value = "ready";
  } catch (cause) {
    if (token !== loadToken) return;
    state.value = "error";
    errorMessage.value = cause instanceof Error ? cause.message : String(cause);
  }
}

async function createModelObjectUrl() {
  const bytes = await readFile({
    repoId: props.repoId,
    path: props.entry.path,
  });
  const data = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  return URL.createObjectURL(new Blob([data], { type: mimeTypeForExtension(extension.value) }));
}

function mimeTypeForExtension(fileExtension: string) {
  if (fileExtension === "glb") return "model/gltf-binary";
  if (fileExtension === "gltf") return "model/gltf+json";
  if (fileExtension === "obj") return "text/plain";
  return "application/octet-stream";
}

function setupRenderer() {
  if (!canvas.value || !container.value) return;

  const nextRenderer = new WebGLRenderer({
    canvas: canvas.value,
    antialias: true,
    alpha: true,
  });
  nextRenderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  nextRenderer.setClearColor(new Color(0x000000), 0);
  renderer.value = nextRenderer;

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
  animate();
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
  if (fileExtension === "fbx") {
    return new FBXLoader().loadAsync(source);
  }
  if (fileExtension === "obj") {
    return new OBJLoader().loadAsync(source);
  }
  if (fileExtension === "glb" || fileExtension === "gltf") {
    const result = await new GLTFLoader().loadAsync(source);
    return result.scene;
  }
  throw new Error(`暂不支持 .${fileExtension || "unknown"} 模型预览`);
}

function resizeRenderer() {
  if (!container.value || !renderer.value || !camera) return;
  const width = Math.max(container.value.clientWidth, 1);
  const height = Math.max(container.value.clientHeight, 1);
  renderer.value.setSize(width, height, false);
  camera.aspect = width / height;
  camera.updateProjectionMatrix();
}

function animate() {
  frameId = window.requestAnimationFrame(animate);
  controls?.update();
  if (scene && camera) {
    renderer.value?.render(scene, camera);
  }
}

function teardown() {
  if (objectUrl) {
    URL.revokeObjectURL(objectUrl);
    objectUrl = null;
  }
  if (frameId !== null) {
    window.cancelAnimationFrame(frameId);
    frameId = null;
  }
  resizeObserver?.disconnect();
  resizeObserver = null;
  controls?.dispose();
  controls = null;
  scene?.children.forEach((child) => disposeObject(child));
  scene = null;
  renderer.value?.dispose();
  renderer.value = null;
  camera = null;
}

function disposeObject(object: Object3D) {
  object.traverse((child) => {
    if (!(child instanceof Mesh)) return;
    child.geometry?.dispose();
    const materials = Array.isArray(child.material) ? child.material : [child.material];
    materials.forEach((material) => material?.dispose());
  });
}
</script>

<template>
  <div ref="container" class="model-preview">
    <canvas ref="canvas" class="model-preview__canvas" />

    <div v-if="state === 'loading'" class="model-preview__overlay">
      <span>正在加载 3D 模型</span>
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
