/**
 * Generate Tests command — AI-powered test generation from the editor.
 *
 * Two modes:
 * 1. Command Palette: prompts for a symbol name
 * 2. Context Menu: uses the selected text as the symbol name
 *
 * Shows a progress notification, polls for completion, and opens
 * the generated test file in a new editor tab.
 */

import * as vscode from 'vscode';
import { ApiClient } from '../api/client';

export class GenerateTestsCommand {
    constructor(private client: ApiClient) {}

    /** Execute from Command Palette — prompts for symbol name. */
    async execute(): Promise<void> {
        const target = await vscode.window.showInputBox({
            prompt: 'Symbol to generate tests for',
            placeHolder: 'e.g., UserService.create_user or src/auth.py',
        });

        if (!target) {
            return;
        }

        await this.generateAndShow(target);
    }

    /** Execute from right-click context menu — uses selection. */
    async executeForSelection(): Promise<void> {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            vscode.window.showWarningMessage('No active editor');
            return;
        }

        const selection = editor.document.getText(editor.selection).trim();
        if (!selection) {
            vscode.window.showWarningMessage('Select a symbol name first');
            return;
        }

        // Clean up selection (remove def/fn/class keywords)
        const cleaned = selection
            .replace(/^(def|fn|class|function|pub fn|pub\(crate\) fn|async def|async fn)\s+/, '')
            .replace(/\(.*$/, '')
            .trim();

        await this.generateAndShow(cleaned);
    }

    private async generateAndShow(target: string): Promise<void> {
        const config = vscode.workspace.getConfiguration('testforge');
        const framework = config.get<string>('defaultFramework', 'pytest');

        try {
            // Start generation
            const job = await this.client.generateTests(target, {
                framework,
                include_edge_cases: true,
                include_mocks: true,
            });

            // Poll for result with progress
            const result = await vscode.window.withProgress(
                {
                    location: vscode.ProgressLocation.Notification,
                    title: `TestForge: Generating tests for "${target}"...`,
                    cancellable: false,
                },
                async (progress) => {
                    progress.report({ increment: 20 });

                    // Poll every 2 seconds
                    const maxAttempts = 60; // 2 minutes max
                    for (let i = 0; i < maxAttempts; i++) {
                        await sleep(2000);
                        progress.report({
                            increment: 60 / maxAttempts,
                            message: `Waiting for AI response...`,
                        });

                        const status = await this.client.getGenerationResult(job.job_id);

                        if (status.status === 'complete' && status.result) {
                            return status.result;
                        }

                        if (status.status === 'failed') {
                            throw new Error(status.error || 'Generation failed');
                        }
                    }

                    throw new Error('Generation timed out');
                }
            );

            // Show the generated tests
            const doc = await vscode.workspace.openTextDocument({
                content: result.source,
                language: this.languageId(framework),
            });

            await vscode.window.showTextDocument(doc, {
                preview: false,
                viewColumn: vscode.ViewColumn.Beside,
            });

            // Show summary
            const warnings = result.warnings.length > 0
                ? ` (${result.warnings.length} warnings)`
                : '';

            const saveAction = 'Save to File';
            const action = await vscode.window.showInformationMessage(
                `TestForge: Generated ${result.test_count} tests for ${result.target_symbol}${warnings}`,
                saveAction
            );

            if (action === saveAction) {
                await this.saveTestFile(result.source, result.file_name);
            }
        } catch (err: any) {
            if (err.response?.status === 404) {
                vscode.window.showErrorMessage(
                    `TestForge: Symbol "${target}" not found. Run "TestForge: Index Project" first.`
                );
            } else {
                vscode.window.showErrorMessage(
                    `TestForge: Test generation failed — ${err.message}`
                );
            }
        }
    }

    private async saveTestFile(source: string, fileName: string): Promise<void> {
        const workspaceFolders = vscode.workspace.workspaceFolders;
        if (!workspaceFolders) {
            return;
        }

        const defaultUri = vscode.Uri.joinPath(
            workspaceFolders[0].uri,
            'tests',
            'generated',
            fileName
        );

        const uri = await vscode.window.showSaveDialog({
            defaultUri,
            filters: {
                'Python': ['py'],
                'JavaScript': ['js', 'ts'],
                'Rust': ['rs'],
                'All Files': ['*'],
            },
        });

        if (uri) {
            await vscode.workspace.fs.writeFile(uri, Buffer.from(source, 'utf-8'));
            vscode.window.showInformationMessage(`Test file saved: ${uri.fsPath}`);

            // Open the saved file
            const doc = await vscode.workspace.openTextDocument(uri);
            await vscode.window.showTextDocument(doc);
        }
    }

    private languageId(framework: string): string {
        const map: Record<string, string> = {
            'pytest': 'python',
            'unittest': 'python',
            'jest': 'typescript',
            'mocha': 'javascript',
            'vitest': 'typescript',
            'cargo-test': 'rust',
            'junit': 'java',
        };
        return map[framework] || 'plaintext';
    }
}

function sleep(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
}