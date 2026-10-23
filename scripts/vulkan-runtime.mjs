// Windows only: puts the Vulkan loader (vulkan-1.dll, signed by LunarG) and
// its licence in src-tauri/vendor/, bundled next to the exe. Without it the
// app does not start on a PC with no GPU driver; with it whisper.cpp falls
// back to the CPU. Runs before every `npm run tauri ...`; no-op once done.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, copyFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const VERSION = "1.4.363.0";
const SOURCE = `https://sdk.lunarg.com/sdk/download/${VERSION}/windows/VulkanRT-X64-${VERSION}-Components.zip`;
const ZIP_SHA256 = "a25a927aa8b9f0371048f1861cf88ac3b9bc9b1fb332c42d897c8ab32695769a";
const DLL_SHA256 = "e1fcfc9489beefa6a6d9d11d6c7517f6a2b7f16e0d3fb8f4103bee0210f314a3";

if (process.platform !== "win32") process.exit(0);

const vendor = new URL("../src-tauri/vendor/", import.meta.url);
const dll = new URL("vulkan-1.dll", vendor);
const sha256 = (data) => createHash("sha256").update(data).digest("hex");

if (existsSync(dll) && sha256(readFileSync(dll)) === DLL_SHA256) process.exit(0);

console.log(`Downloading the Vulkan runtime ${VERSION}...`);
const res = await fetch(SOURCE);
if (!res.ok) throw new Error(`${SOURCE}: HTTP ${res.status}`);
const zip = Buffer.from(await res.arrayBuffer());
if (sha256(zip) !== ZIP_SHA256) throw new Error(`${SOURCE}: unexpected SHA-256`);

const tmp = mkdtempSync(join(tmpdir(), "vulkan-rt-"));
try {
  writeFileSync(join(tmp, "rt.zip"), zip);
  // The tar of Windows (bsdtar) reads zip files; Git's GNU tar does not.
  const tar = join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe");
  execFileSync(tar, ["-xf", "rt.zip"], { cwd: tmp });
  const dir = join(tmp, `VulkanRT-X64-${VERSION}-Components`);
  if (sha256(readFileSync(join(dir, "x64", "vulkan-1.dll"))) !== DLL_SHA256) {
    throw new Error("vulkan-1.dll: unexpected SHA-256");
  }
  mkdirSync(vendor, { recursive: true });
  copyFileSync(join(dir, "x64", "vulkan-1.dll"), dll);
  copyFileSync(join(dir, "VulkanRT-License.txt"), new URL("VulkanRT-License.txt", vendor));
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
console.log("Vulkan runtime ready in src-tauri/vendor/");
