/**
 * Search command — semantic code search from the command palette.
 *
 * Opens an input box, queries the API, and displays results
 * in a QuickPick list. Selecting a result navigates to the symbol.
 */

import * as vscode from 'vscode';
import { ApiClient, SearchResult } from '../api/client';
import { SearchResultsProvider } from '../providers/searchView';

export class SearchCommand {
    constructor(
        private client: ApiClient,
        private resultsProvider: SearchResultsProvider
    ) {}

    async execute(): Promise<void> {
        const query = await vscode.window.showInputBox({
            prompt: 'Search your codebase (natural language or keywords)',
            placeHolder: 'e.g., "authentication logic" or "validate payment"',
        });

        if (!query) {
            return;
        }

        const config = vscode.workspace.getConfiguration('testforge');
        const limit = config.get<number>('searchLimit', 15);

        try {
            const response = await vscode.window.withProgress(
                {
                    location: vscode.ProgressLocation.Notification,
                    title: `TestForge: Searching for "${query}"...`,
                    cancellable: false,
                },
                async () => {
                    return await this.client.search(query, limit);
                }
            );

            if (response.results.length === 0) {
                vscode.window.showInformationMessage(
                    `TestForge: No results for "${query}". Try broader terms.`
                );
                return;
            }

            // Update sidebar tree view
            this.resultsProvider.setResults(query, response.results);

            // Show QuickPick
            const items = response.results.map((r, i) => ({
                label: `$(symbol-${this.kindIcon(r.symbol.kind)}) ${r.symbol.qualified_name}`,
                description: `${r.symbol.kind} • ${r.match_source} • ${(r.score * 100).toFixed(0)}%`,
                detail: `${r.symbol.file_path}:${r.symbol.start_line}` +
                    (r.symbol.docstring ? ` — ${r.symbol.docstring.substring(0, 60)}` : ''),
                result: r,
            }));

            const selected = await vscode.window.showQuickPick(items, {
                title: `TestForge: ${response.total_results} results for "${query}" (${response.search_time_ms}ms)`,
                matchOnDescription: true,
                matchOnDetail: true,
            });

            if (selected) {
                await this.navigateToSymbol(selected.result);
            }
        } catch (err: any) {
            vscode.window.showErrorMessage(
                `TestForge search failed: ${err.message}. Is the server running?`
            );
        }
    }

    private async navigateToSymbol(result: SearchResult): Promise<void> {
        const workspaceFolders = vscode.workspace.workspaceFolders;
        if (!workspaceFolders) {
            return;
        }

        const filePath = vscode.Uri.joinPath(
            workspaceFolders[0].uri,
            result.symbol.file_path
        );

        try {
            const doc = await vscode.workspace.openTextDocument(filePath);
            const editor = await vscode.window.showTextDocument(doc);

            const startLine = Math.max(0, result.symbol.start_line - 1);
            const endLine = result.symbol.end_line - 1;
            const range = new vscode.Range(startLine, 0, endLine, 0);

            editor.selection = new vscode.Selection(range.start, range.start);
            editor.revealRange(range, vscode.TextEditorRevealType.InCenter);

            // Highlight the symbol range
            const decoration = vscode.window.createTextEditorDecorationType({
                backgroundColor: new vscode.ThemeColor('editor.findMatchHighlightBackground'),
                isWholeLine: true,
            });
            editor.setDecorations(decoration, [range]);

            // Remove highlight after 3 seconds
            setTimeout(() => decoration.dispose(), 3000);
        } catch {
            vscode.window.showWarningMessage(
                `Could not open ${result.symbol.file_path}`
            );
        }
    }

    private kindIcon(kind: string): string {
        const icons: Record<string, string> = {
            function: 'method',
            method: 'method',
            class: 'class',
            struct: 'struct',
            enum: 'enum',
            trait: 'interface',
            interface: 'interface',
            module: 'namespace',
            constant: 'constant',
        };
        return icons[kind] || 'variable';
    }
}