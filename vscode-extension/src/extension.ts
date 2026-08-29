import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import { SnapshotTreeProvider } from './tree/SnapshotTreeProvider';
import { RouteCodeLensProvider } from './codelens/RouteCodeLensProvider';
import { LicenseManager } from './utils/license';

let terminalInstance: vscode.Terminal | undefined;

export function activate(context: vscode.ExtensionContext) {
  const workspaceRoot =
    vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders.length > 0
      ? vscode.workspace.workspaceFolders[0].uri.fsPath
      : undefined;

  // 1. Initialize Tree Data Provider
  const treeProvider = new SnapshotTreeProvider(workspaceRoot);
  vscode.window.registerTreeDataProvider('apisnap.snapshotList', treeProvider);

  // 2. Register CodeLens Provider for Python, Go, Rust, TypeScript, JavaScript
  const codeLensProvider = new RouteCodeLensProvider();
  context.subscriptions.push(
    vscode.languages.registerCodeLensProvider(
      [
        { language: 'python' },
        { language: 'go' },
        { language: 'rust' },
        { language: 'typescript' },
        { language: 'javascript' },
      ],
      codeLensProvider
    )
  );

  // 3. Pro Status Bar Item
  const statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
  updateStatusBar(statusBar);
  statusBar.show();
  context.subscriptions.push(statusBar);

  // 4. Command: Run Test
  context.subscriptions.push(
    vscode.commands.registerCommand('apisnap.runTest', async () => {
      runInTerminal('cargo run -- test');
    })
  );

  // 5. Command: Record Snapshots
  context.subscriptions.push(
    vscode.commands.registerCommand('apisnap.recordAll', async () => {
      runInTerminal('cargo run -- record');
      setTimeout(() => treeProvider.refresh(), 2000);
    })
  );

  // 6. Command: Interactive Review in Terminal
  context.subscriptions.push(
    vscode.commands.registerCommand('apisnap.reviewDiffs', async () => {
      runInTerminal('cargo run -- review');
    })
  );

  // 7. Command: Refresh Tree
  context.subscriptions.push(
    vscode.commands.registerCommand('apisnap.refreshTree', () => {
      treeProvider.refresh();
      vscode.window.showInformationMessage('ApiSnap snapshots refreshed.');
    })
  );

  // 8. Command: Open Snapshot File
  context.subscriptions.push(
    vscode.commands.registerCommand('apisnap.openSnapshot', async (filePath: string) => {
      if (fs.existsSync(filePath)) {
        const doc = await vscode.workspace.openTextDocument(vscode.Uri.file(filePath));
        await vscode.window.showTextDocument(doc, { preview: false });
      }
    })
  );

  // 9. Command: Activate Pro License
  context.subscriptions.push(
    vscode.commands.registerCommand('apisnap.activatePro', async () => {
      const success = await LicenseManager.promptActivation();
      if (success) {
        updateStatusBar(statusBar);
      }
    })
  );
}

function updateStatusBar(statusBar: vscode.StatusBarItem) {
  const isPro = LicenseManager.isProActivated();
  if (isPro) {
    statusBar.text = '$(check) ApiSnap Pro [Active]';
    statusBar.tooltip = 'ApiSnap Pro Lifetime License Active';
    statusBar.color = new vscode.ThemeColor('charts.green');
    statusBar.command = 'apisnap.runTest';
  } else {
    statusBar.text = '$(sparkle) ApiSnap [Free]';
    statusBar.tooltip = 'Click to activate ApiSnap Pro Lifetime License ($19)';
    statusBar.color = new vscode.ThemeColor('charts.yellow');
    statusBar.command = 'apisnap.activatePro';
  }
}

function runInTerminal(command: string) {
  if (!terminalInstance || terminalInstance.exitStatus !== undefined) {
    terminalInstance = vscode.window.createTerminal({
      name: 'ApiSnap Terminal',
      iconPath: new vscode.ThemeIcon('camera'),
    });
  }
  terminalInstance.show();
  terminalInstance.sendText(command);
}

export function deactivate() {
  if (terminalInstance) {
    terminalInstance.dispose();
  }
}
