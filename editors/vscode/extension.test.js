const assert = require('assert');
const Module = require('module');

const commands = new Map();
const executions = [];
const output = [];
const terminals = [];
let taskDescription = 'add navigation; echo should-not-run';
let checkChoice = 'Run Checks';

const vscode = {
    StatusBarAlignment: { Right: 1 },
    workspace: {
        workspaceFolders: [{ uri: { fsPath: 'C:\\project' } }]
    },
    commands: {
        registerCommand(name, callback) {
            commands.set(name, callback);
            return { dispose() {} };
        }
    },
    window: {
        createOutputChannel() {
            return {
                appendLine(line) { output.push(line); },
                show() {},
                dispose() {}
            };
        },
        createStatusBarItem() {
            return { show() {}, dispose() {} };
        },
        createTerminal(options) {
            const terminal = {
                options,
                shown: false,
                command: undefined,
                show() { this.shown = true; },
                sendText(text, addNewLine) { this.command = { text, addNewLine }; }
            };
            terminals.push(terminal);
            return terminal;
        },
        async showInputBox() { return taskDescription; },
        async showWarningMessage(_message, _options, action) {
            return action === 'Run Checks' ? checkChoice : undefined;
        },
        async showInformationMessage() {},
        async showErrorMessage() {},
        async showWorkspaceFolderPick() { return undefined; }
    }
};

const originalLoad = Module._load;
Module._load = function (request, parent, isMain) {
    if (request === 'vscode') return vscode;
    if (request === 'child_process') {
        return {
            execFile(file, args, options, callback) {
                executions.push({ file, args, options });
                const stdout = args.length === 1 && args[0] === 'check'
                    ? 'Detected project checks:\n  - Rust tests: cargo test'
                    : 'ok';
                callback(null, stdout, '');
            }
        };
    }
    return originalLoad.call(this, request, parent, isMain);
};

const extension = require('./extension');
Module._load = originalLoad;

async function main() {
    const subscriptions = [];
    extension.activate({ subscriptions });

    await commands.get('centaur.task')();
    assert.deepStrictEqual(executions[0], {
        file: 'centaur',
        args: ['task', taskDescription],
        options: { cwd: 'C:\\project', windowsHide: true }
    });
    assert(!output.some(line => line.includes(taskDescription)), 'task text leaked into output');

    await commands.get('centaur.applyClipboard')();
    assert.deepStrictEqual(terminals[0].options, {
        name: 'Centaur Review',
        cwd: 'C:\\project'
    });
    assert.strictEqual(terminals[0].shown, true);
    assert.deepStrictEqual(terminals[0].command, {
        text: 'centaur --clipboard',
        addNewLine: true
    });
    assert.strictEqual(executions.length, 1, 'apply must stay in one interactive Centaur process');

    await commands.get('centaur.check')();
    assert.deepStrictEqual(executions[1].args, ['check']);
    assert.deepStrictEqual(executions[2].args, ['check', '--run']);

    vscode.workspace.workspaceFolders = [];
    taskDescription = 'should not run';
    await commands.get('centaur.task')();
    assert.strictEqual(executions.length, 3, 'Centaur ran without an open workspace');

    console.log('VS Code extension behavior: valid');
}

main().catch(error => {
    console.error(error);
    process.exitCode = 1;
});
