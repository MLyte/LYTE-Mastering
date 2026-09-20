import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const configPath = path.join(root, "src-tauri", "tauri.conf.json");
const config = fs.readFileSync(configPath, "utf8");
const time = new Intl.DateTimeFormat("fr-BE", {
  timeZone: "Europe/Brussels",
  hour: "2-digit",
  minute: "2-digit",
  hour12: false,
}).format(new Date());
const titlePattern = /"title": "LYTE Mastering v[^"]+"/;
if (!titlePattern.test(config)) throw new Error("Could not find the LYTE window title to stamp.");
const stamped = config.replace(titlePattern, title => `${title.replace(/ · build \d{2}:\d{2}"$/, "")} · build ${time}"`);
fs.writeFileSync(configPath, stamped);
console.log(`Build time stamped: ${time}`);
