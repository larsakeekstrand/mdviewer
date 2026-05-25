// Pure file-type detection (no DOM) so it's unit-testable under `node --test`.

export const IMAGE_EXT = /\.(png|jpe?g|gif|webp|avif|bmp|ico|svg)$/i;

export function isImagePath(path) {
  return IMAGE_EXT.test(path || "");
}
