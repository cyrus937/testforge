/**
 * TestForge VS Code Extension
 *
 * Provides semantic code search and AI-powered test generation
 * directly in the editor via the TestForge API server.
 *
 * Features:
 * - Command Palette search with inline results
 * - Right-click "Generate Tests" for selected symbols
 * - Sidebar with search results and index status
 * - Real-time indexing progress via WebSocket
 */

import * as vscode from 'vscode';
import { SearchCommand } from './commands/search';
import { GenerateTestsCommand } from './commands/generateTests';
import { IndexProjectCommand } from './commands/indexProject';
import { StatusBarProvider } from './providers/statusBar';
import { SearchResultsProvider } from './providers/searchView';
import { ApiClient } from './api/client';

let statusBar: StatusBarProvider;

export function activate(context: vscode.ExtensionContext): void {
    console.log('TestForge extension activated');

    // Initialize API client
    const config = vscode.workspace.getConfiguration('testforge');
    const serverUrl = config.get<string>('serverUrl', 'http://127.0.0.1:7654');
    const client = new ApiClient(serverUrl);

    // Status bar
    statusBar = new StatusBarProvider(client);
    context.subscriptions.push(statusBar);

    // Search results tree view
    const searchProvider = new SearchResultsProvider();
    vscode.window.registerTreeDataProvider('testforgeSearch', searchProvider);

    // Commands
    const searchCmd = new SearchCommand(client, searchProvider);
    const genTestsCmd = new GenerateTestsCommand(client);
    const indexCmd = new IndexProjectCommand(client, statusBar);

    context.subscriptions.push(
        vscode.commands.registerCommand('testforge.search', () => searchCmd.execute()),
        vscode.commands.registerCommand('testforge.generateTests', () => genTestsCmd.execute()),
        vscode.commands.registerCommand(
            'testforge.generateTestsForSymbol',
            () => genTestsCmd.executeForSelection()
        ),
        vscode.commands.registerCommand('testforge.indexProject', () => indexCmd.execute()),
        vscode.commands.registerCommand('testforge.showStatus', () => showStatus(client)),
    );

    // Auto-index on save (if enabled)
    if (config.get<boolean>('autoIndex', false)) {
        const watcher = vscode.workspace.onDidSaveTextDocument((doc) => {
            const ext = doc.fileName.split('.').pop();
            const codeExts = ['py', 'js', 'ts', 'jsx', 'tsx', 'rs', 'java', 'go'];
            if (ext && codeExts.includes(ext)) {
                indexCmd.execute(true); // silent mode
            }
        });
        context.subscriptions.push(watcher);
    }

    // Refresh status on activation
    statusBar.refresh();
}

export function deactivate(): void {
    console.log('TestForge extension deactivated');
}

async function showStatus(client: ApiClient): Promise<void> {
    try {
        const status = await client.getStatus();
        const message = [
            `Files: ${status.file_count}`,
            `Symbols: ${status.symbol_count}`,
            `Vectors: ${status.vector_count}`,
            `Languages: ${status.languages.join(', ') || 'none'}`,
            `Last indexed: ${status.last_indexed || 'never'}`,
        ].join('\n');

        vscode.window.showInformationMessage(
            `TestForge Index Status`,
            { modal: true, detail: message }
        );
    } catch (err: any) {
        vscode.window.showErrorMessage(
            `TestForge: Cannot connect to server. Is it running? (${err.message})`
        );
    }
}