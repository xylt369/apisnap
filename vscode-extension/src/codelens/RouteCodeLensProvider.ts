import * as vscode from 'vscode';

export class RouteCodeLensProvider implements vscode.CodeLensProvider {
  private static readonly ROUTE_PATTERNS = [
    // Python (FastAPI / Flask / Django)
    /@(app|router)\.(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']/g,
    // Go (Gin / Chi / Fiber)
    /\.(GET|POST|PUT|DELETE|PATCH)\s*\(\s*["']([^"']+)["']/g,
    // TypeScript / Express / Fastify / Nest
    /\.(get|post|put|delete|patch)\s*\(\s*["']([^"']+)["']/g,
    // Rust (Axum / Actix)
    /\.(route|get|post)\s*\(\s*["']([^"']+)["']/g,
  ];

  public provideCodeLenses(
    document: vscode.TextDocument,
    token: vscode.CancellationToken
  ): vscode.CodeLens[] | Thenable<vscode.CodeLens[]> {
    const codeLenses: vscode.CodeLens[] = [];
    const text = document.getText();

    for (const pattern of RouteCodeLensProvider.ROUTE_PATTERNS) {
      let match: RegExpExecArray | null;
      pattern.lastIndex = 0;

      while ((match = pattern.exec(text)) !== null) {
        const line = document.positionAt(match.index).line;
        const range = new vscode.Range(line, 0, line, 0);

        // CodeLens 1: Status & Quick Test
        codeLenses.push(
          new vscode.CodeLens(range, {
            title: '📸 ApiSnap: Run Test',
            command: 'apisnap.runTest',
            tooltip: 'Run ApiSnap regression test against this endpoint',
          })
        );

        // CodeLens 2: Record / Refresh
        codeLenses.push(
          new vscode.CodeLens(range, {
            title: '⚡ Record Snapshot',
            command: 'apisnap.recordAll',
            tooltip: 'Record fresh snapshot for this route',
          })
        );
      }
    }

    return codeLenses;
  }
}
