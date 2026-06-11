export function register(ctx) {
  const {
    h,
    nextTick,
    onBeforeUnmount,
    ref,
    watch,
  } = ctx.vue;

  const ThreeModelPreviewPlugin = {
    name: "ThreeModelPreviewPlugin",
    props: {
      entry: {
        type: Object,
        default: null,
      },
      repoId: {
        type: String,
        default: "",
      },
    },
    setup(props) {
      const container = ref(null);
      const canvas = ref(null);
      const state = ref("idle");
      const errorMessage = ref("");
      const modelInfo = ref("");
      const loadProgress = ref({
        value: 6,
        label: "读取模型",
        detail: "准备读取文件",
      });
      let renderer = null;
      let scene = null;
      let camera = null;
      let controls = null;
      let currentVrm = null;
      let frameId = null;
      let resizeObserver = null;
      let loadToken = 0;
      let previousFrameTime = 0;

      watch(
        () => [props.repoId, props.entry?.path],
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
        loadProgress.value = {
          value: 6,
          label: "读取模型",
          detail: "准备读取文件",
        };

        await nextTick();
        if (!canvas.value || !container.value) return;

        teardown();
        await setupRenderer();

        try {
          const extension = (props.entry?.extension ?? "").toLowerCase();
          if (extension === "blend") {
            throw new Error("暂不支持直接预览 .blend 源文件，请先从 Blender 导出为 glb、gltf、fbx、obj、stl 或 3mf。");
          }
          const source = await createModelSourceUrl(ctx, props.repoId, props.entry);
          if (token !== loadToken) return;
          loadProgress.value = {
            value: 48,
            label: "解析模型",
            detail: "读取几何与材质",
          };
          const loaded = await loadObjectByExtension(source, extension, loadProgress);
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
          };
          state.value = "ready";
          void persistCanvasThumbnail(token, props.repoId, props.entry);
        } catch (cause) {
          if (token !== loadToken) return;
          state.value = "error";
          errorMessage.value = cause instanceof Error ? cause.message : String(cause);
        }
      }

      async function persistCanvasThumbnail(token, repoId, entry) {
        await nextTick();
        if (token !== loadToken || !canvas.value || !renderer || !scene || !camera) return;
        renderer.render(scene, camera);
        const blob = await new Promise((resolve) => {
          canvas.value?.toBlob(resolve, "image/jpeg", 0.86);
        });
        if (token !== loadToken || !blob) return;
        await ctx.saveGeneratedThumbnail({
          repoId,
          path: entry.path,
          imageBytes: Array.from(new Uint8Array(await blob.arrayBuffer())),
          mediaType: blob.type || "image/jpeg",
        });
      }

      async function setupRenderer() {
        const three = await import("three");
        const { OrbitControls } = await import("three/examples/jsm/controls/OrbitControls.js");
        if (!canvas.value || !container.value) return;

        renderer = new three.WebGLRenderer({
          canvas: canvas.value,
          antialias: true,
          alpha: true,
          preserveDrawingBuffer: true,
        });
        renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
        renderer.setClearColor(new three.Color(0x000000), 0);

        scene = new three.Scene();
        scene.add(new three.AmbientLight(0xffffff, 1.8));

        const keyLight = new three.DirectionalLight(0xffffff, 2.2);
        keyLight.position.set(4, 6, 5);
        scene.add(keyLight);

        const fillLight = new three.DirectionalLight(0x9fc5ff, 1.1);
        fillLight.position.set(-5, 3, -4);
        scene.add(fillLight);

        const grid = new three.GridHelper(8, 16, 0x6b7280, 0x374151);
        grid.position.y = -0.02;
        scene.add(grid);

        camera = new three.PerspectiveCamera(45, 1, 0.01, 10000);
        camera.position.set(3, 2.2, 4);

        controls = new OrbitControls(camera, renderer.domElement);
        controls.enableDamping = true;
        controls.dampingFactor = 0.08;
        controls.target.set(0, 0.6, 0);

        resizeObserver = new ResizeObserver(resizeRenderer);
        resizeObserver.observe(container.value);
        resizeRenderer();
        previousFrameTime = performance.now();
        animate(previousFrameTime);
      }

      function mountObject(object) {
        if (!scene || !camera || !controls) return;

        scene.add(object);

        import("three").then((three) => {
          const bounds = new three.Box3().setFromObject(object);
          const size = bounds.getSize(new three.Vector3());
          const center = bounds.getCenter(new three.Vector3());
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
        });
      }

      function resizeRenderer() {
        if (!container.value || !renderer || !camera) return;
        const width = Math.max(container.value.clientWidth, 1);
        const height = Math.max(container.value.clientHeight, 1);
        renderer.setSize(width, height, false);
        camera.aspect = width / height;
        camera.updateProjectionMatrix();
      }

      function animate(frameTime = performance.now()) {
        frameId = window.requestAnimationFrame(animate);
        const delta = Math.min(Math.max((frameTime - previousFrameTime) / 1000, 0), 0.1);
        previousFrameTime = frameTime;
        currentVrm?.update?.(delta);
        controls?.update();
        if (scene && camera) {
          renderer?.render(scene, camera);
        }
      }

      function teardown() {
        if (frameId !== null) {
          window.cancelAnimationFrame(frameId);
          frameId = null;
        }
        resizeObserver?.disconnect();
        resizeObserver = null;
        controls?.dispose?.();
        controls = null;
        currentVrm = null;
        scene?.children.forEach((child) => disposeObject(child));
        scene = null;
        renderer?.dispose?.();
        renderer = null;
        camera = null;
      }

      return {
        canvas,
        container,
        errorMessage,
        modelInfo,
        loadProgress,
        state,
        extensionLabel() {
          return (props.entry?.extension ?? "").toUpperCase();
        },
      };
    },
    render() {
      return h("div", { ref: "container", class: "model-preview" }, [
        h("canvas", { ref: "canvas", class: "model-preview__canvas" }),
        this.state === "loading"
          ? h("div", { class: "model-preview__status" }, [
              h("span", this.loadProgress.label),
              h("span", this.loadProgress.detail),
            ])
          : null,
        this.state === "error"
          ? h("div", { class: "model-preview__overlay model-preview__overlay--error" }, [
              h("strong", "无法预览该模型"),
              h("span", this.errorMessage),
            ])
          : null,
        this.state === "ready"
          ? h("div", { class: "model-preview__hud" }, [
              h("span", this.extensionLabel()),
              this.modelInfo ? h("span", this.modelInfo) : null,
            ])
          : null,
      ]);
    },
  };

  ctx.registerPreview({
    supportedExtensions: ["fbx", "obj", "glb", "gltf", "vrm", "stl", "3mf", "blend"],
    component: ThreeModelPreviewPlugin,
  });
}

async function createModelSourceUrl(ctx, repoId, entry) {
  const response = await ctx.preparePreviewFileSource({
    repoId,
    path: entry.path,
  });
  if (!response.sourceUrl) {
    throw new Error("模型预览源不可用");
  }
  return response.sourceUrl;
}

async function loadObjectByExtension(source, fileExtension, loadProgress) {
  const three = await import("three");
  const onProgress = (event) => {
    if (!event.lengthComputable || event.total <= 0) {
      loadProgress.value = {
        value: Math.max(loadProgress.value.value, 58),
        label: "解析模型",
        detail: "解析模型结构",
      };
      return;
    }
    const loadedPercent = Math.round((event.loaded / event.total) * 42);
    loadProgress.value = {
      value: Math.min(94, 48 + loadedPercent),
      label: "解析模型",
      detail: "读取几何与材质",
    };
  };

  if (fileExtension === "fbx") {
    const { FBXLoader } = await import("three/examples/jsm/loaders/FBXLoader.js");
    return { object: await new FBXLoader().loadAsync(source, onProgress) };
  }
  if (fileExtension === "obj") {
    const { OBJLoader } = await import("three/examples/jsm/loaders/OBJLoader.js");
    return { object: await new OBJLoader().loadAsync(source, onProgress) };
  }
  if (fileExtension === "stl") {
    const { STLLoader } = await import("three/examples/jsm/loaders/STLLoader.js");
    const geometry = await new STLLoader().loadAsync(source, onProgress);
    if (!geometry.getAttribute("normal")) {
      geometry.computeVertexNormals();
    }
    return {
      object: new three.Mesh(
        geometry,
        new three.MeshStandardMaterial({
          color: 0xb8c2cc,
          roughness: 0.72,
          metalness: 0.08,
        }),
      ),
    };
  }
  if (fileExtension === "3mf") {
    const { ThreeMFLoader } = await import("three/examples/jsm/loaders/3MFLoader.js");
    return { object: await new ThreeMFLoader().loadAsync(source, onProgress) };
  }
  if (fileExtension === "glb" || fileExtension === "gltf" || fileExtension === "vrm") {
    const { GLTFLoader } = await import("three/examples/jsm/loaders/GLTFLoader.js");
    const loader = new GLTFLoader();
    if (fileExtension === "vrm") {
      const { VRMLoaderPlugin, VRMMetaLoaderPlugin, VRMUtils } = await import("@pixiv/three-vrm");
      loader.register((parser) => new VRMLoaderPlugin(parser, {
        metaPlugin: new VRMMetaLoaderPlugin(parser),
      }));
      const result = await loader.loadAsync(source, onProgress);
      const vrm = result.userData.vrm;
      if (vrm) {
        VRMUtils.rotateVRM0(vrm);
        return { object: vrm.scene, vrm };
      }
      return { object: result.scene };
    }
    const result = await loader.loadAsync(source, onProgress);
    return { object: result.scene };
  }
  throw new Error(`暂不支持 .${fileExtension || "unknown"} 模型预览`);
}

function disposeObject(object) {
  import("@pixiv/three-vrm").then(({ VRMUtils }) => {
    VRMUtils.deepDispose(object);
  }).catch(() => {
    object?.traverse?.((child) => {
      child.geometry?.dispose?.();
      if (Array.isArray(child.material)) {
        child.material.forEach((material) => material?.dispose?.());
      } else {
        child.material?.dispose?.();
      }
    });
  });
}
