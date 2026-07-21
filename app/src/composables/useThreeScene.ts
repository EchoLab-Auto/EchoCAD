import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { ref, shallowRef, markRaw, type Ref, type ShallowRef } from "vue";

export interface ThreeContext {
  scene: THREE.Scene;
  camera: THREE.PerspectiveCamera;
  renderer: THREE.WebGLRenderer;
  controls: OrbitControls;
  raycaster: THREE.Raycaster;
  sketchGroup: THREE.Group;
  previewGroup: THREE.Group;
  gridHelper: THREE.GridHelper;
}

export interface UseThreeSceneReturn {
  container: Ref<HTMLDivElement | null>;
  ctx: ShallowRef<ThreeContext | null>;
  wireframe: Ref<boolean>;
  showEdges: Ref<boolean>;
  initScene: () => void;
  setView: (name: ViewName) => void;
  toggleWireframe: () => void;
  toggleEdges: () => void;
  fitView: (meshes?: THREE.Object3D[]) => void;
  worldToScreen: (wx: number, wy: number, wz: number) => { x: number; y: number };
  dispose: () => void;
  animate: () => void;
}

export type ViewName = "front" | "top" | "right" | "iso" | "back" | "left" | "bottom" | "face";

export const VIEW_PRESETS: Record<string, { pos: [number, number, number]; target: [number, number, number] }> = {
  front: { pos: [0, 0, 10], target: [0, 0, 0] },
  back: { pos: [0, 0, -10], target: [0, 0, 0] },
  top: { pos: [0, 10, 0], target: [0, 0, 0] },
  bottom: { pos: [0, -10, 0], target: [0, 0, 0] },
  right: { pos: [10, 0, 0], target: [0, 0, 0] },
  left: { pos: [-10, 0, 0], target: [0, 0, 0] },
  iso: { pos: [5, 4, 6], target: [0, 0, 0] },
};

export function useThreeScene(): UseThreeSceneReturn {
  const container = ref<HTMLDivElement | null>(null);
  // Use shallowRef so Vue does not deep-proxy the Three.js objects (Proxying
  // breaks WebGLRenderer, PerspectiveCamera, etc.).
  const ctx = shallowRef<ThreeContext | null>(null);
  const wireframe = ref(false);
  const showEdges = ref(true);
  let animationId: number | null = null;
  let resizeObserver: ResizeObserver | null = null;

  function initScene() {
    if (!container.value) return;
    const w = container.value.clientWidth || 800;
    const h = container.value.clientHeight || 600;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x1e1e1e);
    const raycaster = new THREE.Raycaster();

    const camera = new THREE.PerspectiveCamera(45, w / h, 0.01, 1000);
    camera.position.set(5, 4, 6);
    camera.lookAt(0, 0, 0);

    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({ antialias: true });
    } catch (err) {
      console.error("[EchoCAD] WebGL renderer init failed:", err);
      return;
    }
    renderer.setSize(w, h);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    container.value.appendChild(renderer.domElement);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;

    scene.add(new THREE.AmbientLight(0xffffff, 0.55));
    const d1 = new THREE.DirectionalLight(0xffffff, 0.75);
    d1.position.set(5, 10, 7);
    scene.add(d1);
    const d2 = new THREE.DirectionalLight(0xffffff, 0.25);
    d2.position.set(-3, -1, -5);
    scene.add(d2);

    const gridHelper = new THREE.GridHelper(10, 10, 0x333333, 0x222222);
    scene.add(gridHelper);
    scene.add(new THREE.AxesHelper(1));

    const sketchGroup = new THREE.Group();
    scene.add(sketchGroup);
    const previewGroup = new THREE.Group();
    previewGroup.name = "preview";
    scene.add(previewGroup);

    ctx.value = markRaw({ scene, camera, renderer, controls, raycaster, sketchGroup, previewGroup, gridHelper });

    resizeObserver = new ResizeObserver(() => {
      if (!container.value) return;
      const w = container.value.clientWidth;
      const h = container.value.clientHeight;
      if (w > 0 && h > 0) {
        camera.aspect = w / h;
        camera.updateProjectionMatrix();
        renderer.setSize(w, h);
        // Update pixel ratio when the window moves to a different-DPI monitor.
        renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
      }
    });
    resizeObserver.observe(container.value);
  }

  function setView(name: ViewName) {
    if (!ctx.value) return;
    const { camera, controls } = ctx.value;
    const v = VIEW_PRESETS[name];
    if (!v) return;
    const d = 5;
    camera.position.set(v.pos[0] * d, v.pos[1] * d, v.pos[2] * d);
    controls.target.set(v.target[0], v.target[1], v.target[2]);
    controls.update();
  }

  function toggleWireframe() {
    wireframe.value = !wireframe.value;
  }

  function toggleEdges() {
    showEdges.value = !showEdges.value;
  }

  function fitView(meshes?: THREE.Object3D[]) {
    if (!ctx.value) return;
    const objs = meshes ?? ctx.value.scene.children.filter(c => c.type === "Mesh");
    if (objs.length === 0) return;
    const { camera, controls } = ctx.value;
    const box = new THREE.Box3();
    for (const m of objs) box.expandByObject(m);
    if (box.isEmpty()) return;
    const c = box.getCenter(new THREE.Vector3());
    const s = box.getSize(new THREE.Vector3());
    const d = Math.max(s.x, s.y, s.z) * 1.5;
    controls.target.copy(c);
    camera.position.set(c.x + d, c.y + d * 0.7, c.z + d);
    controls.update();
  }

  function worldToScreen(wx: number, wy: number, wz: number): { x: number; y: number } {
    if (!ctx.value) return { x: 0, y: 0 };
    const v = new THREE.Vector3(wx, wy, wz);
    v.project(ctx.value.camera);
    const el = ctx.value.renderer.domElement;
    return {
      x: (v.x * 0.5 + 0.5) * el.clientWidth,
      y: (-v.y * 0.5 + 0.5) * el.clientHeight,
    };
  }

  function animate() {
    animationId = requestAnimationFrame(animate);
    if (!ctx.value) return;
    ctx.value.controls.update();
    ctx.value.renderer.render(ctx.value.scene, ctx.value.camera);
  }

  function dispose() {
    resizeObserver?.disconnect();
    if (animationId !== null) cancelAnimationFrame(animationId);
    if (ctx.value) {
      ctx.value.controls.dispose();
      // Recursively dispose geometries, materials, and textures on every
      // object in the scene so that GPU resources are released.
      ctx.value.scene.traverse((obj) => {
        if (obj instanceof THREE.Mesh || obj instanceof THREE.Line || obj instanceof THREE.LineSegments || obj instanceof THREE.Points) {
          obj.geometry?.dispose();
          const mat = obj.material;
          if (mat) {
            if (Array.isArray(mat)) {
              for (const m of mat) disposeMaterial(m);
            } else {
              disposeMaterial(mat);
            }
          }
        }
      });
      // Remove the renderer's canvas from the DOM so it does not leak.
      const canvas = ctx.value.renderer.domElement;
      if (canvas.parentElement) {
        canvas.parentElement.removeChild(canvas);
      }
      ctx.value.renderer.dispose();
    }
    ctx.value = null;
  }

  function disposeMaterial(mat: THREE.Material) {
    // Dispose textures referenced by the material before disposing the material.
    for (const key of Object.keys(mat)) {
      const v = (mat as any)[key];
      if (v && v.isTexture) {
        v.dispose();
      }
    }
    mat.dispose();
  }

  return { container, ctx, wireframe, showEdges, initScene, setView, toggleWireframe, toggleEdges, fitView, worldToScreen, dispose, animate };
}

/// Map a string plane name to a { origin, normal, uDir, vDir } frame in 3D.
export function planeFrame(planeName: string): {
  origin: THREE.Vector3;
  normal: THREE.Vector3;
  uDir: THREE.Vector3;
  vDir: THREE.Vector3;
} {
  switch (planeName) {
    case "yz":
      return {
        origin: new THREE.Vector3(0, 0, 0),
        normal: new THREE.Vector3(1, 0, 0),
        uDir: new THREE.Vector3(0, 1, 0),
        vDir: new THREE.Vector3(0, 0, 1),
      };
    case "zx":
      return {
        origin: new THREE.Vector3(0, 0, 0),
        normal: new THREE.Vector3(0, 1, 0),
        uDir: new THREE.Vector3(0, 0, 1),
        vDir: new THREE.Vector3(1, 0, 0),
      };
    case "xy":
    default:
      return {
        origin: new THREE.Vector3(0, 0, 0),
        normal: new THREE.Vector3(0, 0, 1),
        uDir: new THREE.Vector3(1, 0, 0),
        vDir: new THREE.Vector3(0, 1, 0),
      };
  }
}

export function sketchToWorld3D(sx: number, sy: number, planeName: string): THREE.Vector3 {
  const { origin, uDir, vDir } = planeFrame(planeName);
  return origin.clone().add(uDir.clone().multiplyScalar(sx)).add(vDir.clone().multiplyScalar(sy));
}

export function worldToSketch2D(world: THREE.Vector3, planeName: string): { x: number; y: number } {
  const { origin, uDir, vDir } = planeFrame(planeName);
  const rel = world.clone().sub(origin);
  return { x: rel.dot(uDir), y: rel.dot(vDir) };
}
