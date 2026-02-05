import { glob } from "glob";
import path from "path";

export interface DiscoverOptions {
  cwd: string;
  patterns: string[];
  ignore?: string[];
}

export async function discoverFiles(options: DiscoverOptions): Promise<string[]> {
  const results = await glob(options.patterns, {
    cwd: options.cwd,
    ignore: options.ignore,
    nodir: true,
    absolute: true
  });
  return results.map((p) => path.resolve(p));
}

export function toUri(filePath: string): string {
  const resolved = path.resolve(filePath);
  const prefix = process.platform === "win32" ? "file:///" : "file://";
  return prefix + resolved.replace(/\\/g, "/");
}

export function relativePath(root: string, filePath: string): string {
  return path.relative(root, filePath).replace(/\\/g, "/");
}
