const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client = null;
let outputChannel = null;

function getServerCommand() {
    const configured = vscode.workspace.getConfiguration().get("cool.lsp.serverCommand");
    if (Array.isArray(configured) && configured.length > 0 && configured.every((item) => typeof item === "string")) {
        const filtered = configured.map((item) => item.trim()).filter((item) => item.length > 0);
        if (filtered.length > 0) {
            return filtered;
        }
    }
    return ["cool", "lsp"];
}

async function startClient(context) {
    const serverCommand = getServerCommand();
    const watchers = [
        vscode.workspace.createFileSystemWatcher("**/*.cool"),
        vscode.workspace.createFileSystemWatcher("**/cool.toml")
    ];

    for (const watcher of watchers) {
        context.subscriptions.push(watcher);
    }

    const clientOptions = {
        documentSelector: [
            { scheme: "file", language: "cool" },
            { scheme: "untitled", language: "cool" }
        ],
        synchronize: {
            fileEvents: watchers
        },
        outputChannel,
        traceOutputChannel: outputChannel
    };

    client = new LanguageClient(
        "coolLanguageServer",
        "Cool Language Server",
        {
            command: serverCommand[0],
            args: serverCommand.slice(1)
        },
        clientOptions
    );

    try {
        await client.start();
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        vscode.window.showErrorMessage(
            `Cool: failed to start language server with ${JSON.stringify(serverCommand)}: ${message}`
        );
        throw error;
    }
}

async function restartClient(context) {
    if (client) {
        await client.stop();
        client = null;
    }
    await startClient(context);
}

async function activate(context) {
    outputChannel = vscode.window.createOutputChannel("Cool Language Server");
    context.subscriptions.push(outputChannel);

    context.subscriptions.push(
        vscode.workspace.onDidChangeConfiguration(async (event) => {
            if (!event.affectsConfiguration("cool.lsp.serverCommand")) {
                return;
            }
            try {
                await restartClient(context);
                vscode.window.showInformationMessage("Cool: restarted language server after configuration change.");
            } catch (_) {
            }
        })
    );

    await startClient(context);
}

async function deactivate() {
    if (client) {
        await client.stop();
        client = null;
    }
}

module.exports = {
    activate,
    deactivate
};
