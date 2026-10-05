import fs from "node:fs";
import path from "node:path";

// Update-time utility only; Bazel fetches the resulting manifest by SRI.
const root = process.argv[2];
if (!root) throw new Error("Usage: bun localllm/private/lock_opencode.ts <upstream-source>");
const lock = Bun.JSON5.parse(fs.readFileSync(path.join(root, "bun.lock"), "utf8"));
const packages = lock.packages;
const atoms = (key: string) => key.match(/(?:@[^/]+\/)?[^/]+/g) || [];

function resolve(from: string, dependency: string) {
  const parts = atoms(from);
  for (let i = parts.length; i >= 0; i--) {
    const key = [...parts.slice(0, i), dependency].join("/");
    if (packages[key]) return key;
  }
  throw new Error(`Cannot resolve ${dependency} from ${from}`);
}

function supports(input: string | string[] | undefined, value: string) {
  if (!input) return true;
  const constraints = typeof input === "string" ? [input] : input;
  if (constraints.includes(`!${value}`)) return false;
  const positive = constraints.filter(item => !item.startsWith("!"));
  return positive.length === 0 || positive.includes(value);
}

const selected = new Set<string>();
const queue = ["opencode"];
const workspaces = Object.fromEntries(
  Object.entries(lock.workspaces).map(([directory, metadata]: any) => [
    metadata.name, { path: directory, ...metadata },
  ]),
);
while (queue.length) {
  const key = queue.pop()!;
  if (selected.has(key)) continue;
  const entry = packages[key];
  if (!entry) throw new Error(`Missing package ${key}`);
  const workspace = entry[0].includes("@workspace:");
  const metadata = workspace ? workspaces[entry[0].split("@workspace:")[0]] : entry[2];
  if (!supports(metadata.os, "linux") || !supports(metadata.cpu, "x64")) continue;
  selected.add(key);
  const dependencies = {
    ...metadata.dependencies,
    ...metadata.optionalDependencies,
    ...metadata.peerDependencies,
    ...(workspace ? metadata.devDependencies : {}),
  };
  for (const dependency of Object.keys(dependencies)) {
    try {
      queue.push(resolve(key, dependency));
    } catch (error) {
      if (!metadata.optionalPeers?.includes(dependency)) throw error;
    }
  }
}

const entries = [...selected].sort().map(key => {
  const entry = packages[key];
  const dest = atoms(key).map(name => `node_modules/${name}`).join("/");
  if (entry[0].includes("@workspace:")) {
    return { dest, workspace: entry[0].split("@workspace:")[1] };
  }
  const at = entry[0].lastIndexOf("@");
  const name = entry[0].slice(0, at);
  const version = entry[0].slice(at + 1);
  if (entry.length !== 4 || !entry[3].startsWith("sha512-")) {
    throw new Error(`Expected a pinned registry package with SRI: ${key}`);
  }
  return {
    dest,
    url: `https://registry.npmjs.org/${name}/-/${name.split("/").pop()}-${version}.tgz`,
    integrity: entry[3],
  };
});
fs.writeFileSync("localllm/private/opencode-npm.lock.json", JSON.stringify(entries, null, 2) + "\n");
console.log(`${entries.length} packages`);
