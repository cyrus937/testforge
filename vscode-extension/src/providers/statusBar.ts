/**
 * Status bar item showing TestForge index status.
 *
 * Displays symbol count, server connectivity, and provides
 * a click action to show full status details.
 */

import * as vscode from 'vscode';
import { ApiClient } from '../api/client';

export class StatusBarProvider implements vscode.Disposable {
    private item: vscode.StatusBarItem;
    private client: ApiClient;
    private refreshInterval: NodeJS.Timeout | undefined;

    constructor(client: ApiClient) {
        this.client = client;
        this.item = vscode.window.createStatusBarItem(
            vscode.StatusBarAlignment.Left,
            50
        );
        this.item.command = 'testforge.showStatus';
        this.item.show();

        // Refresh every 30 seconds
        this.refreshInterval = setInterval(() => this.refresh(), 30000);

        // Initial state
        this.setOffline();
    }

    async refresh(): Promise<void> {
        try {
            const status = await this.client.getStatus();
            this.item.text = `$(beaker) TF: ${status.symbol_count} symbols`;
            this.item.tooltip = [
                `TestForge — Connected`,
                `Files: ${status.file_count}`,
                `Symbols: ${status.symbol_count}`,
                `Vectors: ${status.embedding_count}`,
                `Languages: ${status.languages.join(', ') || 'none'}`,
            ].join('\n');
            this.item.backgroundColor = undefined;
        } catch {
            this.setOffline();
        }
    }

    setIndexing(): void {
        this.item.text = '$(sync~spin) TF: Indexing...';
        this.item.tooltip = 'TestForge is indexing your project';
    }

    private setOffline(): void {
        this.item.text = '$(circle-slash) TF: Offline';
        this.item.tooltip = 'TestForge server not running.\nRun: testforge serve';
        this.item.backgroundColor = new vscode.ThemeColor(
            'statusBarItem.warningBackground'
        );
    }

    dispose(): void {
        if (this.refreshInterval) {
            clearInterval(this.refreshInterval);
        }
        this.item.dispose();
    }
}