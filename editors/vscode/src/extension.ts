import { workspace, ExtensionContext } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient;

export function activate(context: ExtensionContext) {
  const config = workspace.getConfiguration("axis");
  const serverPath = config.get<string>("server.path", "axis");

  const serverOptions: ServerOptions = {
    command: serverPath,
    args: ["--lsp"],
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "axis" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.axis"),
    },
  };

  client = new LanguageClient(
    "axis-ls",
    "Axis Language Server",
    serverOptions,
    clientOptions
  );

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}
