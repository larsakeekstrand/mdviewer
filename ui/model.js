// Pure helpers for the 3D model viewer (no DOM, no three.js) so they run under `node --test`.

export function modelFormat(path) {
  const m = /\.(stl|3mf|obj)$/i.exec(path || "");
  return m ? m[1].toLowerCase() : null;
}

const fmt = (n) => (Math.round(n * 10) / 10 + 0).toFixed(1).replace(/^-0\.0$/, "0.0");

export function formatDimensions(min, max, unit = "mm") {
  const parts = [max.x - min.x, max.y - min.y, max.z - min.z].map(fmt);
  return `${parts.join(" × ")} ${unit}`;
}

export function frameCamera(center, radius, fovDeg, aspect) {
  const r = Math.max(radius, 1e-3);
  const vHalf = (fovDeg * Math.PI) / 360;
  const hHalf = Math.atan(Math.tan(vHalf) * aspect);
  const half = Math.min(vHalf, hHalf);
  const distance = (r * 1.25) / Math.sin(half);
  const len = Math.hypot(1, 0.8, 1);
  const dir = { x: 1 / len, y: 0.8 / len, z: 1 / len };
  return {
    position: {
      x: center.x + dir.x * distance,
      y: center.y + dir.y * distance,
      z: center.z + dir.z * distance,
    },
    target: { x: center.x, y: center.y, z: center.z },
    near: distance / 100,
    far: distance * 100,
  };
}

export function keepCamera(prevPath, nextPath, reason) {
  return reason === "reload" && prevPath === nextPath;
}
