import fs from 'node:fs';
import path from 'node:path';
const root = path.resolve(import.meta.dirname, '..');
const release = path.join(root, 'src-tauri', 'target', 'release');
if (!fs.existsSync(path.join(release, 'lyte-mastering.exe'))) throw new Error('Build the release executable first.');
// NSIS takes its files directly from source. Keep the standalone .exe equally usable.
fs.cpSync(path.join(root, 'bin'), path.join(release, 'bin'), { recursive: true });
fs.cpSync(path.join(root, 'resources', 'phaselimiter'), path.join(release, 'resources', 'phaselimiter'), { recursive: true });
console.log('Standalone release runtime copied beside lyte-mastering.exe.');
