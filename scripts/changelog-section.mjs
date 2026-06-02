import { readFileSync } from "node:fs";
import { changelogSection } from "../ui/update.js";

const version = (process.argv[2] || "").replace(/^v/, "");
const path = process.argv[3] || "CHANGELOG.md";

let text = "";
try {
  text = readFileSync(path, "utf8");
} catch {
  process.exit(0);
}

const out = changelogSection(text, version);
if (out) process.stdout.write(out);
