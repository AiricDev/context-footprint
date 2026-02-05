import { LspClient, LspClientOptions } from "./lsp-client";
import { SemanticData } from "./types";

export interface ExtractOptions {
  projectRoot: string;
  output?: string;
  configPath?: string;
  exclude?: string[];
  verbose?: boolean;
}

export abstract class ExtractorBase {
  protected client?: LspClient;

  constructor(protected readonly options: ExtractOptions) {}

  protected abstract getLspOptions(): LspClientOptions;
  protected abstract collectSemanticData(): Promise<SemanticData>;

  async run(): Promise<SemanticData> {
    this.client = new LspClient(this.getLspOptions());
    await this.client.start();
    try {
      const data = await this.collectSemanticData();
      return data;
    } finally {
      await this.client.shutdown().catch(() => undefined);
    }
  }
}
