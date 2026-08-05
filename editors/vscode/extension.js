const vscode = require('vscode');
const { exec } = require('child_process');
const path = require('path');

function activate(context) {
    const outputChannel = vscode.window.createOutputChannel('Centaur');

    // Create Status Bar Item
    const statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    statusBarItem.command = 'centaur.applyClipboard';
    statusBarItem.text = '$(zap) Centaur';
    statusBarItem.tooltip = 'Click to apply Search/Replace patch from clipboard (Ctrl+Alt+V)';
    statusBarItem.show();
    context.subscriptions.push(statusBarItem);

    function runCentaurCommand(args, successMsg) {
        const workspaceFolders = vscode.workspace.workspaceFolders;
        const cwd = workspaceFolders ? workspaceFolders[0].uri.fsPath : process.cwd();

        const cmd = `centaur ${args.join(' ')}`;
        outputChannel.appendLine(`Executing: ${cmd} (in ${cwd})`);

        exec(cmd, { cwd }, (error, stdout, stderr) => {
            if (stdout) outputChannel.appendLine(stdout);
            if (stderr) outputChannel.appendLine(stderr);

            if (error) {
                vscode.window.showErrorMessage(`Centaur Error: ${stderr || error.message}`);
            } else {
                vscode.window.showInformationMessage(successMsg || 'Centaur action completed successfully!');
            }
        });
    }

    const applySub = vscode.commands.registerCommand('centaur.applyClipboard', () => {
        runCentaurCommand(['--clipboard'], '✨ Centaur Search/Replace patch applied!');
    });

    const undoSub = vscode.commands.registerCommand('centaur.undo', () => {
        runCentaurCommand(['undo'], '⏪ Centaur patch reverted to previous snapshot.');
    });

    const exportSub = vscode.commands.registerCommand('centaur.export', () => {
        runCentaurCommand(['--export', '--mode', 'changed'], '📋 Centaur context exported & prompt copied to clipboard!');
    });

    context.subscriptions.push(applySub, undoSub, exportSub, outputChannel);
}

function deactivate() {}

module.exports = {
    activate,
    deactivate
};
