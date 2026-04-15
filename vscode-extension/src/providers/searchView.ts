/**
 * Search results tree view in the TestForge sidebar.
 *
 * Displays the last search results with icons for symbol kind,
 * match source badges, and click-to-navigate.
 */

import * as vscode from 'vscode';
import { SearchResult } from '../api/client';

export class SearchResultsProvider implements vscode.TreeDataProvider<SearchResultItem> {
    private _onDidChangeTreeData = new vscode.EventEmitter<SearchResultItem | undefined>();
    readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

    private results: SearchResult[] = [];
    private query: string = '';

    setResults(query: string, results: SearchResult[]): void {
        this.query = query;
        this.results = results;
        this._onDidChangeTreeData.fire(undefined);
    }

    clear(): void {
        this.results = [];
        this.query = '';
        this._onDidChangeTreeData.fire(undefined);
    }

    getTreeItem(element: SearchResultItem): vscode.TreeItem {
        return element;
    }

    getChildren(element?: SearchResultItem): SearchResultItem[] {
        if (element) {
            // Child items: details about the symbol
            return this.getSymbolDetails(element.searchResult);
        }

        if (this.results.length === 0) {
            return [
                new SearchResultItem(
                    'No search results',
                    'Run "TestForge: Search" from the command palette',
                    vscode.TreeItemCollapsibleState.None
                ),
            ];
        }

        return this.results.map((r, i) => {
            const item = new SearchResultItem(
                `${r.symbol.qualified_name}`,
                `${r.symbol.kind} · ${r.match_source} · ${(r.score * 100).toFixed(0)}%`,
                vscode.TreeItemCollapsibleState.Collapsed
            );
            item.searchResult = r;
            item.iconPath = this.getIcon(r.symbol.kind);
            item.contextValue = 'searchResult';

            // Click to navigate
            const workspaceFolders = vscode.workspace.workspaceFolders;
            if (workspaceFolders) {
                const fileUri = vscode.Uri.joinPath(
                    workspaceFolders[0].uri,
                    r.symbol.file_path
                );
                item.command = {
                    title: 'Go to Symbol',
                    command: 'vscode.open',
                    arguments: [
                        fileUri,
                        {
                            selection: new vscode.Range(
                                Math.max(0, r.symbol.start_line - 1), 0,
                                r.symbol.end_line - 1, 0
                            ),
                        },
                    ],
                };
            }

            return item;
        });
    }

    private getSymbolDetails(result: SearchResult): SearchResultItem[] {
        const details: SearchResultItem[] = [];

        details.push(new SearchResultItem(
            `📁 ${result.symbol.file_path}:${result.symbol.start_line}`,
            '',
            vscode.TreeItemCollapsibleState.None
        ));

        if (result.symbol.signature) {
            details.push(new SearchResultItem(
                `📝 ${result.symbol.signature}`,
                '',
                vscode.TreeItemCollapsibleState.None
            ));
        }

        if (result.symbol.docstring) {
            const doc = result.symbol.docstring.length > 80
                ? result.symbol.docstring.substring(0, 77) + '...'
                : result.symbol.docstring;
            details.push(new SearchResultItem(
                `📖 ${doc}`,
                '',
                vscode.TreeItemCollapsibleState.None
            ));
        }

        if (result.symbol.dependencies.length > 0) {
            details.push(new SearchResultItem(
                `🔗 Calls: ${result.symbol.dependencies.join(', ')}`,
                '',
                vscode.TreeItemCollapsibleState.None
            ));
        }

        return details;
    }

    private getIcon(kind: string): vscode.ThemeIcon {
        const icons: Record<string, string> = {
            function: 'symbol-method',
            method: 'symbol-method',
            class: 'symbol-class',
            struct: 'symbol-struct',
            enum: 'symbol-enum',
            trait: 'symbol-interface',
            interface: 'symbol-interface',
            module: 'symbol-namespace',
            constant: 'symbol-constant',
        };
        return new vscode.ThemeIcon(icons[kind] || 'symbol-variable');
    }
}

export class SearchResultItem extends vscode.TreeItem {
    searchResult!: SearchResult;

    constructor(
        label: string,
        description: string,
        collapsibleState: vscode.TreeItemCollapsibleState
    ) {
        super(label, collapsibleState);
        this.description = description;
    }
}