import { access, copyFile, mkdir, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const source = join(root, "public", "cepa-icon.svg");
const destination = join(root, "src-tauri", "icons");
const temporary = await mkdtemp(join(tmpdir(), "cepa-icons-"));
const desktopIcons = [
  "32x32.png",
  "128x128.png",
  "128x128@2x.png",
  "icon.png",
  "icon.ico",
  "StoreLogo.png",
  "Square30x30Logo.png",
  "Square44x44Logo.png",
  "Square71x71Logo.png",
  "Square89x89Logo.png",
  "Square107x107Logo.png",
  "Square142x142Logo.png",
  "Square150x150Logo.png",
  "Square284x284Logo.png",
  "Square310x310Logo.png",
] as const;

async function run(command: string[]) {
  const child = Bun.spawn(command, {
    cwd: root,
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await child.exited;
  if (exitCode !== 0) {
    throw new Error(`${command[0]} failed with exit code ${exitCode}.`);
  }
}

try {
  await run(["bun", "run", "tauri", "icon", "-o", temporary, source]);

  let macIcon: string | null = join(temporary, "icon.icns");
  if (process.platform === "darwin") {
    const iconset = join(temporary, "normalized.iconset");
    const normalized = join(temporary, "normalized.icns");
    await run(["iconutil", "-c", "iconset", macIcon, "-o", iconset]);
    await run(["iconutil", "-c", "icns", iconset, "-o", normalized]);
    macIcon = normalized;
  } else {
    try {
      await access(join(destination, "icon.icns"));
      macIcon = null;
    } catch {
      // A fresh non-macOS checkout still receives Tauri's portable ICNS output.
    }
  }

  await mkdir(destination, { recursive: true });
  await Promise.all(
    desktopIcons.map((name) => copyFile(join(temporary, name), join(destination, name))),
  );
  if (macIcon !== null) {
    await copyFile(macIcon, join(destination, "icon.icns"));
  }
  console.log(
    `Updated ${desktopIcons.length + (macIcon === null ? 0 : 1)} desktop icons from public/cepa-icon.svg.`,
  );
  if (macIcon === null) {
    console.log("Preserved icon.icns; run this recipe on macOS to regenerate it deterministically.");
  }
} finally {
  await rm(temporary, { recursive: true, force: true });
}
