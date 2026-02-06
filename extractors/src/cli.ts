#!/usr/bin/env node
import { Command } from "commander";
import fs from "fs";
import path from "path";
import { PythonExtractor } from "./languages/python/extractor";
import { ExtractOptions } from "./core/extractor-base";

const program = new Command();

program
  .name("extract-semantics")
  .description("Extract SemanticData via LSP")
  .argument("<language>", "Language to extract (e.g., python)")
  .argument("<projectRoot>", "Path to project root")
  .option("-o, --output <file>", "Output file (defaults to stdout)")
  .option("-c, --config <file>", "Language server config path")
  .option("-e, --exclude <pattern...>", "Glob patterns to exclude")
  .option("-v, --verbose", "Verbose logging")
  .action(async (language: string, projectRoot: string, options: any) => {
    try {
      const resolvedRoot = path.resolve(projectRoot);
      const extractOptions: ExtractOptions = {
        projectRoot: resolvedRoot,
        output: options.output,
        configPath: options.config,
        exclude: options.exclude,
        verbose: options.verbose
      };

      const extractor = createExtractor(language, extractOptions);
      if (options.verbose) {
        console.error(`Starting extraction for ${language} at ${resolvedRoot}`);
      }
      const data = await extractor.run();
      const json = JSON.stringify(data, null, 2);

      if (options.output) {
        fs.writeFileSync(path.resolve(options.output), json, "utf8");
      } else {
        process.stdout.write(json);
      }

      const mem = process.memoryUsage();
      const toMB = (bytes: number) => (bytes / 1024 / 1024).toFixed(1);
      console.error(
        `\nMemory usage: RSS=${toMB(mem.rss)}MB, Heap Used=${toMB(mem.heapUsed)}MB, Heap Total=${toMB(mem.heapTotal)}MB, External=${toMB(mem.external)}MB`
      );
    } catch (err) {
      if (err instanceof Error) {
        console.error(options?.verbose ? err.stack ?? err.message : err.message);
      } else {
        console.error(String(err));
      }
      process.exitCode = 1;
    }
  });

program.parse(process.argv);

function createExtractor(language: string, options: ExtractOptions) {
  switch (language.toLowerCase()) {
    case "python":
      return new PythonExtractor(options);
    default:
      throw new Error(`Unsupported language: ${language}`);
  }
}
