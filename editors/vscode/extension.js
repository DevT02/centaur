const vscode = require('vscode');
const { execFile } = require('child_process');

function activate(context) {
    const outputChannel = vscode.window.createOutputChannel('Centaur');

    const statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    statusBarItem.command = 'centaur.task';
    statusBarItem.text = '$(zap) Centaur';
    statusBarItem.tooltip = 'Start an AI-assisted project task';
    statusBarItem.show();
    context.subscriptions.push(statusBarItem);

    async function chooseWorkspace() {
        const workspaceFolders = vscode.workspace.workspaceFolders;
        if (!workspaceFolders || workspaceFolders.length === 0) {
            vscode.window.showErrorMessage('Open a project folder before running Centaur.');
            return undefined;
        }
        if (workspaceFolders.length === 1) return workspaceFolders[0].uri.fsPath;

        const selected = await vscode.window.showWorkspaceFolderPick({
            placeHolder: 'Choose the project Centaur should use'
        });
        return selected?.uri.fsPath;
    }

    function runCentaurCommand(args, cwd) {
        outputChannel.appendLine(`Running Centaur in ${cwd}`);
        return new Promise((resolve, reject) => {
            execFile('centaur', args, { cwd, windowsHide: true }, (error, stdout, stderr) => {
                if (stdout) outputChannel.appendLine(stdout.trimEnd());
                if (stderr) outputChannel.appendLine(stderr.trimEnd());

                if (error) {
                    vscode.window.showErrorMessage('Centaur could not complete that action. Check the Centaur output for details.');
                    outputChannel.show(true);
                    reject(error);
                    return;
                }
                resolve(stdout);
            });
        });
    }

    const taskSub = vscode.commands.registerCommand('centaur.task', async () => {
        const cwd = await chooseWorkspace();
        if (!cwd) return;
        const description = await vscode.window.showInputBox({
            title: 'Start an AI task with Centaur',
            prompt: 'What should the AI change?',
            placeHolder: 'Add keyboard navigation to the command menu',
            ignoreFocusOut: true,
            validateInput: value => value.trim() ? undefined : 'Describe the change first.'
        });
        if (!description) return;

        try {
            await runCentaurCommand(['task', description.trim()], cwd);
            vscode.window.showInformationMessage('Centaur prepared the project context and AI prompt.');
        } catch (_) {}
    });

    const applySub = vscode.commands.registerCommand('centaur.applyClipboard', async () => {
        const cwd = await chooseWorkspace();
        if (!cwd) return;
        const terminal = vscode.window.createTerminal({ name: 'Centaur Review', cwd });
        terminal.show();
        terminal.sendText('centaur --clipboard', true);
    });

    const checkSub = vscode.commands.registerCommand('centaur.check', async () => {
        const cwd = await chooseWorkspace();
        if (!cwd) return;
        try {
            const plan = await runCentaurCommand(['check'], cwd);
            outputChannel.show(true);
            if (!plan.includes('Detected project checks:')) {
                vscode.window.showInformationMessage('Centaur did not find a known project check.');
                return;
            }
            const choice = await vscode.window.showWarningMessage(
                'Review the detected commands in the Centaur output. Repository commands may execute arbitrary code.',
                { modal: true },
                'Run Checks'
            );
            if (choice === 'Run Checks') {
                await runCentaurCommand(['check', '--run'], cwd);
            }
        } catch (_) {}
    });

    const undoSub = vscode.commands.registerCommand('centaur.undo', async () => {
        const cwd = await chooseWorkspace();
        if (!cwd) return;
        try {
            await runCentaurCommand(['undo', 'latest'], cwd);
            vscode.window.showInformationMessage('Centaur restored the project state before the last change.');
        } catch (_) {}
    });

    const exportSub = vscode.commands.registerCommand('centaur.export', async () => {
        const cwd = await chooseWorkspace();
        if (!cwd) return;
        try {
            await runCentaurCommand(['--export', '--mode', 'changed'], cwd);
            vscode.window.showInformationMessage('Centaur exported the changed project context.');
        } catch (_) {}
    });

    context.subscriptions.push(taskSub, applySub, checkSub, undoSub, exportSub, outputChannel);
}

function deactivate() {}

module.exports = {
    activate,
    deactivate
};
