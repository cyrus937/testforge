/**
 * Configuration utilities for the TestForge VS Code extension.
 */

import * as vscode from 'vscode';

export interface TestForgeConfig {
    serverUrl: string;
    autoIndex: boolean;
    defaultFramework: string;
    searchLimit: number;
}

/** Read the current extension configuration. */
export function getConfig(): TestForgeConfig {
    const config = vscode.workspace.getConfiguration('testforge');
    return {
        serverUrl: config.get<string>('serverUrl', 'http://127.0.0.1:7654'),
        autoIndex: config.get<boolean>('autoIndex', false),
        defaultFramework: config.get<string>('defaultFramework', 'pytest'),
        searchLimit: config.get<number>('searchLimit', 15),
    };
}

/** Listen for configuration changes. */
export function onConfigChange(callback: (config: TestForgeConfig) => void): vscode.Disposable {
    return vscode.workspace.onDidChangeConfiguration((e) => {
        if (e.affectsConfiguration('testforge')) {
            callback(getConfig());
        }
    });
}
