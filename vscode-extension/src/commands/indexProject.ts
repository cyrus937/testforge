/**
 * Index Project command — triggers re-indexing and shows progress.
 */

import * as vscode from 'vscode';
import { ApiClient } from '../api/client';
import { StatusBarProvider } from '../providers/statusBar';

export class IndexProjectCommand {
    constructor(
        private client: ApiClient,
        private statusBar: StatusBarProvider
    ) {}

    async execute(silent: boolean = false): Promise<void> {
        try {
            const job = await this.client.triggerIndex(false);

            if (silent) {
                // Background indexing — just update status bar when done
                this.pollUntilDone(job.job_id);
                return;
            }

            await vscode.window.withProgress(
                {
                    location: vscode.ProgressLocation.Notification,
                    title: 'TestForge: Indexing project...',
                    cancellable: false,
                },
                async (progress) => {
                    progress.report({ increment: 10, message: 'Parsing source files...' });

                    const maxAttempts = 120;
                    for (let i = 0; i < maxAttempts; i++) {
                        await sleep(1000);
                        progress.report({
                            increment: 80 / maxAttempts,
                            message: 'Building search index...',
                        });

                        // Check if job is still running
                        try {
                            const health = await this.client.getHealth();
                            if (i > 5) {
                                // Give it at least 5 seconds
                                const status = await this.client.getStatus();
                                if (status.file_count > 0) {
                                    progress.report({ increment: 100, message: 'Complete!' });
                                    break;
                                }
                            }
                        } catch {
                            // Server might be busy
                        }
                    }
                }
            );

            // Refresh status bar
            this.statusBar.refresh();

            const status = await this.client.getStatus();
            vscode.window.showInformationMessage(
                `TestForge: Indexed ${status.file_count} files, ${status.symbol_count} symbols`
            );
        } catch (err: any) {
            vscode.window.showErrorMessage(
                `TestForge: Indexing failed — ${err.message}. Is the server running?`
            );
        }
    }

    private async pollUntilDone(jobId: string): Promise<void> {
        for (let i = 0; i < 60; i++) {
            await sleep(2000);
            try {
                await this.client.getHealth();
                break;
            } catch {
                // Still indexing
            }
        }
        this.statusBar.refresh();
    }
}

function sleep(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
}