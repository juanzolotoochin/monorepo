import fs from "node:fs";
import path from "node:path";

// Bun's upstream build mutates its source directory. Materialize a private tree
// in this action's declared output; never write into an external repository.
const source = path.resolve(path.dirname(process.argv[2]), "../..");
const output = path.resolve(process.argv[3]);
const work = path.join(output, "work");
const home = path.join(output, "home");
fs.mkdirSync(output, { recursive: true });
fs.mkdirSync(home, { recursive: true });
process.env.HOME = home;
process.env.OPENCODE_DISABLE_AUTOUPDATE = "1";
process.env.OPENCODE_DISABLE_MODELS_FETCH = "1";
fs.cpSync(source, work, { recursive: true, dereference: true, filter: p => !/\/node_modules\/opencode(?:\/|$)/.test(p) });
// Workspace packages resolve paths relative to their original packages/* tree.
// Restore these relative links after materializing Bazel's input symlinks.
const manifest = JSON.parse(fs.readFileSync(process.argv[4], "utf8"));
for (const entry of manifest) {
  if (!entry.workspace) continue;
  const link = path.join(work, entry.dest);
  fs.rmSync(link, { recursive: true, force: true });
  fs.mkdirSync(path.dirname(link), { recursive: true });
  fs.symlinkSync(path.relative(path.dirname(link), path.join(work, entry.workspace)), link);
}
process.env.OPENCODE_VERSION = "1.18.34-local";
process.env.OPENCODE_CHANNEL = "local";
delete process.env.OPENCODE_RELEASE;
process.env.MODELS_DEV_API_JSON = path.join(output, "models.json");
fs.writeFileSync(process.env.MODELS_DEV_API_JSON, "{}");
// Bun otherwise embeds sandbox-specific CommonJS paths in compiled chunks.
const buildScript = path.join(work, "packages/opencode/script/build.ts");
const buildSource = fs.readFileSync(buildScript, "utf8");
if (!buildSource.includes("    define: {")) throw new Error("Upstream build define block changed");
fs.writeFileSync(buildScript, buildSource.replace("    define: {", `    define: {
      __dirname: JSON.stringify("/$bunfs/root"),
      __filename: JSON.stringify("/$bunfs/root/opencode"),`));
process.argv = [process.execPath, "build.ts", "--single", "--skip-install", "--skip-embed-web-ui"];
await import(path.join(work, "packages/opencode/script/build.ts"));
fs.copyFileSync(path.join(work, "packages/opencode/dist/opencode-linux-x64/bin/opencode"), path.join(output, "opencode"));
fs.chmodSync(path.join(output, "opencode"), 0o755);
fs.rmSync(work, { recursive: true });
fs.rmSync(home, { recursive: true });
