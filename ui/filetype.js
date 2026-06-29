// Pure file-type detection (no DOM) so it's unit-testable under `node --test`.

export const IMAGE_EXT = /\.(png|jpe?g|gif|webp|avif|bmp|ico|svg)$/i;
export const MARKDOWN_EXT = /\.(md|markdown|mdown|mkd|mkdn)$/i;

export function isImagePath(path) {
  return IMAGE_EXT.test(path || "");
}

export function isMarkdownPath(path) {
  return MARKDOWN_EXT.test(path || "");
}

// A "code view" tab is any file that isn't markdown and isn't an image —
// it renders as syntax-highlighted, line-numbered text.
export function isCodeView(path) {
  return !isImagePath(path) && !isMarkdownPath(path);
}
