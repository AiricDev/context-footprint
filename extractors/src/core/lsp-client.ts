import { ChildProcessWithoutNullStreams, spawn } from "child_process";
import { EventEmitter } from "events";
import { randomUUID } from "crypto";

interface JsonRpcRequest {
  jsonrpc: "2.0";
  id: string;
  method: string;
  params?: any;
}

interface JsonRpcResponse<T = any> {
  jsonrpc: "2.0";
  id: string;
  result?: T;
  error?: {
    code: number;
    message: string;
    data?: any;
  };
}

export interface LspClientOptions {
  command: string;
  args?: string[];
  cwd?: string;
  env?: NodeJS.ProcessEnv;
  initializationOptions?: any;
  rootUri?: string;
}

export class LspClient extends EventEmitter {
  private proc?: ChildProcessWithoutNullStreams;
  private pending = new Map<string, (response: JsonRpcResponse) => void>();
  private stdoutBuffer: Buffer = Buffer.alloc(0);
  private sequence = 0;

  constructor(private readonly options: LspClientOptions) {
    super();
  }

  async start(): Promise<void> {
    if (this.proc) return;

    this.proc = spawn(this.options.command, this.options.args ?? ["--stdio"], {
      cwd: this.options.cwd,
      env: { ...process.env, ...this.options.env },
      stdio: "pipe"
    });

    this.proc.stderr?.on("data", (chunk) => {
      this.emit("stderr", chunk.toString());
    });

    this.proc.stdout.on("data", (chunk) => this.handleStdoutChunk(chunk));

    await this.sendRequest("initialize", {
      processId: process.pid,
      capabilities: {},
      rootUri: this.options.rootUri ?? null,
      workspaceFolders: null,
      initializationOptions: this.options.initializationOptions ?? {}
    });

    await this.sendNotification("initialized", {});
  }

  async shutdown(): Promise<void> {
    if (!this.proc) return;
    await this.sendRequest("shutdown", null);
    await this.sendNotification("exit", null);
    this.proc.kill();
    this.proc = undefined;
  }

  async sendRequest<T>(method: string, params: any): Promise<T> {
    const id = this.nextId();
    const payload: JsonRpcRequest = { jsonrpc: "2.0", id, method, params };
    const promise = new Promise<JsonRpcResponse<T>>((resolve, reject) => {
      this.pending.set(id, (response) => {
        if (response.error) {
          reject(new Error(`LSP error ${response.error.code}: ${response.error.message}`));
        } else {
          resolve(response as JsonRpcResponse<T>);
        }
      });
    });
    this.write(payload);
    const response = await promise;
    return response.result as T;
  }

  async sendNotification(method: string, params: any): Promise<void> {
    const payload = { jsonrpc: "2.0", method, params };
    this.write(payload);
  }

  private write(message: object) {
    if (!this.proc) throw new Error("LSP process not started");
    const json = JSON.stringify(message);
    const data = `Content-Length: ${Buffer.byteLength(json, "utf8")}\r\n\r\n${json}`;
    this.proc.stdin.write(data, "utf8");
  }

  private handleStdoutChunk(chunk: Buffer) {
    this.stdoutBuffer = Buffer.concat([this.stdoutBuffer, chunk]);
    while (true) {
      const separatorIndex = this.stdoutBuffer.indexOf(Buffer.from("\r\n\r\n"));
      if (separatorIndex === -1) return;
      const header = this.stdoutBuffer.slice(0, separatorIndex).toString("utf8");
      const match = header.match(/Content-Length:\s*(\d+)/i);
      if (!match) {
        throw new Error(`Invalid LSP header: ${header}`);
      }
      const length = Number(match[1]);
      const totalLength = separatorIndex + 4 + length;
      if (this.stdoutBuffer.length < totalLength) return;
      const body = this.stdoutBuffer.slice(separatorIndex + 4, totalLength).toString("utf8");
      this.stdoutBuffer = this.stdoutBuffer.slice(totalLength);
      const message = JSON.parse(body);
      this.routeMessage(message);
    }
  }

  private routeMessage(message: any) {
    if (message.id && this.pending.has(message.id)) {
      const handler = this.pending.get(message.id)!;
      this.pending.delete(message.id);
      handler(message);
      return;
    }

    if (message.method) {
      this.emit(message.method, message.params);
    }
  }

  private nextId(): string {
    return `${Date.now()}-${this.sequence++}-${randomUUID()}`;
  }
}
