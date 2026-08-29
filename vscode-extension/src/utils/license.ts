import * as vscode from 'vscode';

export class LicenseManager {
  private static readonly LICENSE_KEY_SETTING = 'apisnap.proLicenseKey';

  public static isProActivated(): boolean {
    const config = vscode.workspace.getConfiguration();
    const key = config.get<string>(this.LICENSE_KEY_SETTING, '').trim();
    // Validate format (e.g. APISNAP-PRO-XXXX-XXXX or valid signature)
    return key.length >= 10 && (key.startsWith('APISNAP-PRO-') || key.startsWith('SNAP-'));
  }

  public static async promptActivation(): Promise<boolean> {
    const key = await vscode.window.showInputBox({
      title: 'Activate ApiSnap Pro Lifetime License',
      prompt: 'Enter your Pro license key received from LemonSqueezy / Stripe',
      placeHolder: 'APISNAP-PRO-XXXX-XXXX-XXXX',
      ignoreFocusOut: true,
    });

    if (!key) {
      return false;
    }

    if (key.trim().length < 10) {
      vscode.window.showErrorMessage('Invalid ApiSnap Pro license key format.');
      return false;
    }

    await vscode.workspace
      .getConfiguration()
      .update(this.LICENSE_KEY_SETTING, key.trim(), vscode.ConfigurationTarget.Global);

    vscode.window.showInformationMessage('🎉 ApiSnap Pro activated successfully! All Pro features unlocked.');
    return true;
  }
}
