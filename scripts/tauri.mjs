import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, join } from "node:path";
import { createRequire } from "node:module";
import { spawn } from "node:child_process";

// Refresh Cargo discovery for terminals opened before Rust was installed.
const env = { ...process.env };
const cargoBin = join(env.CARGO_HOME || join(homedir(), ".cargo"), "bin");
const pathKey =
  Object.keys(env).find((key) => key.toLowerCase() === "path") || "PATH";
if (existsSync(cargoBin))
  env[pathKey] = `${cargoBin}${delimiter}${env[pathKey] || ""}`;
const require = createRequire(import.meta.url);
const cli = require.resolve("@tauri-apps/cli/tauri.js");
const child = spawn(process.execPath, [cli, ...process.argv.slice(2)], {
  stdio: "inherit",
  env,
});
child.on("error", (error) => {
  console.error(error.message);
  process.exitCode = 1;
});
child.on("exit", (code) => {
  process.exitCode = code ?? 1;
});
