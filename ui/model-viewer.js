// 3D model viewer: three.js scene, camera and controls for STL / 3MF / OBJ.
// Loaded by app.js through a dynamic import on the first model open, so none of
// three.js is parsed at startup. One instance per window: its single
// WebGLRenderer (one WebGL context) is reused by every model tab, and the
// canvas is re-attached into #preview on each show().

import * as THREE from "./three/three.module.min.js";
import { OrbitControls } from "./three/OrbitControls.js";
import { STLLoader } from "./three/STLLoader.js";
import { ThreeMFLoader } from "./three/3MFLoader.js";
import { OBJLoader } from "./three/OBJLoader.js";
import { unzipSync, strFromU8 } from "./three/fflate.module.js";
import { formatDimensions, frameCamera } from "./model.js";

const FOV = 45;

const PALETTE = {
  light: { background: 0xf6f8fa, mesh: 0xaab2bb, grid: 0xd0d7de, gridCenter: 0xafb8c1 },
  dark: { background: 0x0d1117, mesh: 0xc4ccd5, grid: 0x30363d, gridCenter: 0x484f58 },
};

// 3MF `<model unit>` → millimetres.
const UNIT_TO_MM = {
  micron: 0.001,
  millimeter: 1,
  centimeter: 10,
  inch: 25.4,
  foot: 304.8,
  meter: 1000,
};

/** three's ThreeMFLoader reads `<model unit>` but never applies it, so the
 *  scale comes from the root model part's header. This inflates that part a
 *  second time (the loader keeps its unzip to itself); only its first few KB
 *  are decoded. Anything unreadable falls back to millimetres — the loader
 *  itself reports a broken archive. */
function threeMfScale(bytes) {
  try {
    const data = new Uint8Array(bytes);
    const rels = unzipSync(data, { filter: (f) => f.name === "_rels/.rels" })["_rels/.rels"];
    const target = rels && /Target="\/?([^"]+\.model)"/i.exec(strFromU8(rels));
    const root = target ? target[1] : "3D/3dmodel.model";
    const part = unzipSync(data, { filter: (f) => f.name === root })[root];
    if (!part) return 1;
    const head = strFromU8(part.subarray(0, 4096));
    const unit = /<model\b[^>]*\bunit\s*=\s*["']([^"']+)["']/i.exec(head);
    return (unit && UNIT_TO_MM[unit[1].toLowerCase()]) || 1;
  } catch {
    return 1;
  }
}

function parse(bytes, format) {
  if (format === "stl") return new THREE.Mesh(new STLLoader().parse(bytes));
  if (format === "obj") return new OBJLoader().parse(new TextDecoder().decode(bytes));
  if (format === "3mf") {
    const group = new ThreeMFLoader().parse(bytes);
    if (!group) throw new Error("the 3MF archive could not be unpacked");
    group.scale.setScalar(threeMfScale(bytes));
    return group;
  }
  throw new Error(`unsupported model format: ${format}`);
}

function disposeMaterial(material) {
  for (const m of Array.isArray(material) ? material : [material]) {
    if (!m) continue;
    for (const value of Object.values(m)) {
      if (value && value.isTexture) value.dispose();
    }
    m.dispose();
  }
}

/** Plain-object copy, so stashed cameras never alias live three.js vectors. */
const vec = (v) => ({ x: v.x, y: v.y, z: v.z });

export function createModelViewer({ getTheme }) {
  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setPixelRatio(window.devicePixelRatio || 1);

  const stage = document.createElement("div");
  stage.className = "model-stage";
  const dims = document.createElement("div");
  dims.className = "model-dims";
  const resetBtn = document.createElement("button");
  resetBtn.type = "button";
  resetBtn.className = "model-reset";
  resetBtn.textContent = "Reset view";
  resetBtn.title = "Re-frame the model (or double-click it)";
  stage.append(renderer.domElement, dims, resetBtn);

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(FOV, 1, 0.1, 1000);
  scene.add(camera);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x8d8d8d, 1.6));
  // Parented to the camera so whatever side faces the viewer is lit.
  const key = new THREE.DirectionalLight(0xffffff, 1.8);
  key.position.set(0.5, 0.6, 0);
  key.target.position.set(0, 0, -1);
  camera.add(key, key.target);

  const material = new THREE.MeshStandardMaterial({
    roughness: 0.85,
    metalness: 0,
    flatShading: true,
    side: THREE.DoubleSide,
  });

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = false;
  controls.addEventListener("change", render);

  let model = null;
  let grid = null;
  // World-space bounds of the shown model (after the Z-up → Y-up rotation).
  let bounds = null;
  let sphere = null;
  // Camera work that has to wait for a non-zero viewport: frameCamera needs a
  // real aspect. `{ frame: true }` or `{ camera: {position, target} }`.
  let pending = null;
  let observed = null;

  const resizeObserver = new ResizeObserver(() => syncSize());

  function sized() {
    return stage.isConnected && stage.clientWidth > 0 && stage.clientHeight > 0;
  }

  function updateClipping() {
    const dist = camera.position.distanceTo(controls.target);
    const radius = sphere ? sphere.radius : 1;
    camera.near = Math.max(dist / 100, 1e-4);
    camera.far = Math.max(dist * 100, dist + radius * 4);
    camera.updateProjectionMatrix();
  }

  function render() {
    if (!model || !sized()) return;
    updateClipping();
    renderer.render(scene, camera);
  }

  function setCamera(position, target) {
    camera.position.set(position.x, position.y, position.z);
    controls.target.set(target.x, target.y, target.z);
    controls.update();
  }

  function frame() {
    const f = frameCamera(sphere.center, sphere.radius, camera.fov, camera.aspect);
    setCamera(f.position, f.target);
  }

  function applyPending() {
    if (!pending) return;
    if (pending.camera) setCamera(pending.camera.position, pending.camera.target);
    else frame();
    pending = null;
  }

  function syncSize() {
    if (!sized()) return;
    const w = stage.clientWidth;
    const h = stage.clientHeight;
    renderer.setPixelRatio(window.devicePixelRatio || 1);
    renderer.setSize(w, h);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    applyPending();
    render();
  }

  function rebuildGrid() {
    if (grid) {
      scene.remove(grid);
      grid.geometry.dispose();
      grid.material.dispose();
      grid = null;
    }
    if (!bounds) return;
    const colors = PALETTE[getTheme()] || PALETTE.light;
    const size = bounds.getSize(new THREE.Vector3());
    const extent = Math.max(size.x, size.z, 1e-3) * 1.4;
    const step = Math.pow(10, Math.floor(Math.log10(extent / 4)));
    const divisions = Math.max(1, Math.ceil(extent / step));
    grid = new THREE.GridHelper(divisions * step, divisions, colors.gridCenter, colors.grid);
    const center = bounds.getCenter(new THREE.Vector3());
    grid.position.set(center.x, bounds.min.y, center.z);
    scene.add(grid);
  }

  function applyColors() {
    const colors = PALETTE[getTheme()] || PALETTE.light;
    scene.background = new THREE.Color(colors.background);
    material.color.set(colors.mesh);
    rebuildGrid();
  }

  function clearModel() {
    if (!model) return;
    scene.remove(model);
    model.traverse((node) => {
      if (node.geometry) node.geometry.dispose();
    });
    model = null;
  }

  /** Swap each loader-made material for the shared matte one. Returns whether
   *  any mesh carries vertices. */
  function adopt(object) {
    let hasVertices = false;
    object.traverse((node) => {
      if (!node.isMesh) return;
      if (node.material !== material) disposeMaterial(node.material);
      node.material = material;
      const pos = node.geometry && node.geometry.getAttribute("position");
      if (pos && pos.count > 0) hasVertices = true;
    });
    return hasVertices;
  }

  function attach(container) {
    if (stage.parentNode !== container) container.append(stage);
    if (observed !== container) {
      if (observed) resizeObserver.unobserve(observed);
      resizeObserver.observe(container);
      observed = container;
    }
  }

  function show(container, bytes, format, { camera: saved = null } = {}) {
    let object;
    try {
      object = parse(bytes, format);
    } catch (e) {
      throw new Error(e && e.message ? e.message : String(e));
    }
    const fileBox = adopt(object) ? new THREE.Box3().setFromObject(object) : null;
    if (!fileBox || fileBox.isEmpty()) {
      object.traverse((node) => node.geometry && node.geometry.dispose());
      throw new Error("the file contains no geometry");
    }
    // Measured before the rotation, so X/Y/Z are the file's (the slicer's)
    // axes rather than three's Y-up world.
    dims.textContent = formatDimensions(fileBox.min, fileBox.max);
    object.rotation.x = -Math.PI / 2;
    object.updateMatrixWorld(true);

    clearModel();
    model = object;
    scene.add(model);
    bounds = new THREE.Box3().setFromObject(model);
    sphere = bounds.getBoundingSphere(new THREE.Sphere());
    applyColors();

    pending = saved
      ? { camera: { position: { ...saved.position }, target: { ...saved.target } } }
      : { frame: true };
    attach(container);
    syncSize();
  }

  function currentCamera() {
    if (pending && pending.camera) {
      return { position: { ...pending.camera.position }, target: { ...pending.camera.target } };
    }
    // Never sized, so never framed: there is no view worth restoring.
    if (pending) return null;
    return { position: vec(camera.position), target: vec(controls.target) };
  }

  function resetView() {
    if (!model) return;
    pending = { frame: true };
    syncSize();
  }

  function applyTheme() {
    applyColors();
    render();
  }

  renderer.domElement.addEventListener("dblclick", resetView);
  resetBtn.addEventListener("click", resetView);

  return { show, camera: currentCamera, applyTheme, resetView };
}

export default createModelViewer;
