/**
 * Test helper for managing LSP lifecycle
 * 
 * To speed up tests, we can either:
 * 1. Use a single LSP instance for all tests (faster but tests may interfere)
 * 2. Start LSP once per test file (balanced)
 * 3. Start LSP per test (current behavior, slow)
 */

import { PythonExtractor } from "../src/languages/python/extractor";
import { SemanticData } from "../src/core/types";

// Global cache for LSP results to avoid re-running extraction
const extractionCache = new Map<string, SemanticData>();

/**
 * Run extraction with caching
 */
export async function runExtractionWithCache(
  projectRoot: string
): Promise<SemanticData> {
  const cacheKey = projectRoot;
  
  if (extractionCache.has(cacheKey)) {
    console.log(`[TestHelper] Using cached result for ${projectRoot}`);
    return extractionCache.get(cacheKey)!;
  }

  console.log(`[TestHelper] Running extraction for ${projectRoot}...`);
  const startTime = Date.now();
  
  const extractor = new PythonExtractor({
    projectRoot,
    verbose: process.env.DEBUG === "1"
  });

  const data = await extractor.run();
  
  const duration = Date.now() - startTime;
  console.log(`[TestHelper] Extraction completed in ${duration}ms`);
  
  extractionCache.set(cacheKey, data);
  return data;
}

/**
 * Clear extraction cache
 */
export function clearExtractionCache(): void {
  extractionCache.clear();
}

/**
 * Get cache size
 */
export function getCacheSize(): number {
  return extractionCache.size;
}
