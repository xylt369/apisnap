import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';

export interface SnapshotItemData {
  endpointName: string;
  filePath: string;
  statusCode: number;
  recordedAt: string;
  durationMs: number;
}

export class SnapshotTreeProvider implements vscode.TreeDataProvider<SnapshotTreeItem> {
  private _onDidChangeTreeData: vscode.EventEmitter<SnapshotTreeItem | undefined | null | void> =
    new vscode.EventEmitter<SnapshotTreeItem | undefined | null | void>();
  readonly onDidChangeTreeData: vscode.Event<SnapshotTreeItem | undefined | null | void> =
    this._onDidChangeTreeData.event;

  constructor(private workspaceRoot: string | undefined) {}

  refresh(): void {
    this._onDidChangeTreeData.fire();
  }

  getTreeItem(element: SnapshotTreeItem): vscode.TreeItem {
    return element;
  }

  getChildren(element?: SnapshotTreeItem): Thenable<SnapshotTreeItem[]> {
    if (!this.workspaceRoot) {
      return Promise.resolve([]);
    }

    if (element) {
      return Promise.resolve([]);
    }

    const snapshotDir = path.join(this.workspaceRoot, '__snapshots__');
    if (!fs.existsSync(snapshotDir)) {
      return Promise.resolve([]);
    }

    try {
      const files = fs.readdirSync(snapshotDir);
      const items: SnapshotTreeItem[] = [];

      for (const file of files) {
        if (file.endsWith('.snap.json')) {
          const fullPath = path.join(snapshotDir, file);
          const content = fs.readFileSync(fullPath, 'utf8');
          try {
            const parsed = JSON.parse(content);
            const data: SnapshotItemData = {
              endpointName: parsed.endpoint_name || file.replace('.snap.json', ''),
              filePath: fullPath,
              statusCode: parsed.metadata?.status_code || 200,
              recordedAt: parsed.metadata?.recorded_at || '',
              durationMs: parsed.metadata?.duration_ms || 0,
            };
            items.push(new SnapshotTreeItem(data));
          } catch {
            // Ignore corrupted JSON
          }
        }
      }

      return Promise.resolve(items);
    } catch {
      return Promise.resolve([]);
    }
  }
}

export class SnapshotTreeItem extends vscode.TreeItem {
  constructor(public readonly data: SnapshotItemData) {
    super(data.endpointName, vscode.TreeItemCollapsibleState.None);

    this.tooltip = `Endpoint: ${data.endpointName}\nStatus: ${data.statusCode} OK\nDuration: ${data.durationMs}ms\nFile: ${data.filePath}`;
    this.description = `HTTP ${data.statusCode} (${data.durationMs}ms)`;
    this.iconPath = new vscode.ThemeIcon('file-code', new vscode.ThemeColor('charts.green'));

    this.command = {
      command: 'apisnap.openSnapshot',
      title: 'Open Snapshot',
      arguments: [data.filePath],
    };

    this.contextValue = 'snapshotItem';
  }
}
