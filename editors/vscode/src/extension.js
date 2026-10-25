// extension.js — Aven Language Extension for VS Code / OpenVSCode / Antigravity
const vscode = require('vscode');
const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');

/**
 * Finds the avenc executable binary.
 * Prioritizes workspace release/debug binaries, then PATH.
 */
function findAvencBinary() {
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (workspaceFolders && workspaceFolders.length > 0) {
        for (const folder of workspaceFolders) {
            const root = folder.uri.fsPath;
            const releaseExe = path.join(root, 'target', 'release', process.platform === 'win32' ? 'avenc.exe' : 'avenc');
            if (fs.existsSync(releaseExe)) {
                return releaseExe;
            }
            const debugExe = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'avenc.exe' : 'avenc');
            if (fs.existsSync(debugExe)) {
                return debugExe;
            }
            const binExe = path.join(root, 'bin', process.platform === 'win32' ? 'avenc.exe' : 'avenc');
            if (fs.existsSync(binExe)) {
                return binExe;
            }
        }
    }
    return process.platform === 'win32' ? 'avenc.exe' : 'avenc';
}

/**
 * Parses avenc check/run output and applies editor squiggles (diagnostics).
 */
function updateDiagnostics(document, diagnosticCollection, outputChannel) {
    if (document.languageId !== 'aven') {
        return;
    }

    const filePath = document.uri.fsPath;
    const avenc = findAvencBinary();

    const child = spawn(avenc, ['check', filePath], {
        cwd: path.dirname(filePath),
        env: process.env
    });

    let stdout = '';
    let stderr = '';

    child.stdout.on('data', data => {
        stdout += data.toString();
    });

    child.stderr.on('data', data => {
        stderr += data.toString();
    });

    child.on('close', code => {
        const diagnostics = [];
        const fullOutput = stdout + '\n' + stderr;

        if (code !== 0) {
            // Match "line X, col Y" patterns from parser and dimensional checker
            // Example: "✗ Syntax error at line 6, col 5: expected expression"
            // Example: "Dimensional mismatch: line 6, col 5: cannot add [Gbps] to [MB]"
            const posRegex = /(?:line\s+(\d+),\s*col\s+(\d+)|at\s+line\s+(\d+),\s*col\s+(\d+))/gi;
            let match;
            let foundPos = false;

            while ((match = posRegex.exec(fullOutput)) !== null) {
                foundPos = true;
                const lineNum = parseInt(match[1] || match[3], 10);
                const colNum = parseInt(match[2] || match[4], 10);

                const lineIndex = Math.max(0, lineNum - 1);
                const colIndex = Math.max(0, colNum - 1);

                // Get line text length to highlight cleanly
                let lineLength = 20;
                if (lineIndex < document.lineCount) {
                    lineLength = document.lineAt(lineIndex).text.length;
                }

                const range = new vscode.Range(
                    lineIndex,
                    colIndex,
                    lineIndex,
                    Math.max(colIndex + 1, lineLength)
                );

                // Extract brief message summary
                const lines = fullOutput.split('\n');
                let msg = 'Aven compilation error';
                for (const l of lines) {
                    if (l.includes('line ' + lineNum) || l.includes('Error') || l.includes('✗')) {
                        msg = l.trim();
                        break;
                    }
                }

                const diagnostic = new vscode.Diagnostic(
                    range,
                    msg,
                    vscode.DiagnosticSeverity.Error
                );
                diagnostic.source = 'avenc';
                diagnostics.push(diagnostic);
            }

            if (!foundPos) {
                // General error without exact line/col
                const range = new vscode.Range(0, 0, 0, document.lineCount > 0 ? document.lineAt(0).text.length : 1);
                const firstErrLine = stderr.trim().split('\n')[0] || stdout.trim().split('\n')[0] || 'Aven compilation failed';
                const diagnostic = new vscode.Diagnostic(
                    range,
                    firstErrLine,
                    vscode.DiagnosticSeverity.Error
                );
                diagnostic.source = 'avenc';
                diagnostics.push(diagnostic);
            }
        } else {
            // Check for warnings (e.g. Under-determined systems)
            if (stdout.includes('UNDER-DETERMINED SYSTEM') || stdout.includes('Check WARNING')) {
                const range = new vscode.Range(0, 0, 0, document.lineCount > 0 ? document.lineAt(0).text.length : 1);
                const diagnostic = new vscode.Diagnostic(
                    range,
                    'Model contains under-determined relation blocks (degrees of freedom > 0).',
                    vscode.DiagnosticSeverity.Warning
                );
                diagnostic.source = 'avenc';
                diagnostics.push(diagnostic);
            }
        }

        diagnosticCollection.set(document.uri, diagnostics);
    });

    child.on('error', err => {
        outputChannel.appendLine(`[avenc] failed to execute binary '${avenc}': ${err.message}`);
    });
}

function activate(context) {
    const outputChannel = vscode.window.createOutputChannel('Aven Toolchain');
    const diagnosticCollection = vscode.languages.createDiagnosticCollection('aven');
    context.subscriptions.push(diagnosticCollection);
    context.subscriptions.push(outputChannel);

    outputChannel.appendLine('Aven Language Extension activated for OpenVSCode / Antigravity.');

    // Check on active document if already open
    if (vscode.window.activeTextEditor) {
        updateDiagnostics(vscode.window.activeTextEditor.document, diagnosticCollection, outputChannel);
    }

    // Auto-check on save and open
    context.subscriptions.push(
        vscode.workspace.onDidSaveTextDocument(doc => {
            updateDiagnostics(doc, diagnosticCollection, outputChannel);
        })
    );

    context.subscriptions.push(
        vscode.workspace.onDidOpenTextDocument(doc => {
            updateDiagnostics(doc, diagnosticCollection, outputChannel);
        })
    );

    context.subscriptions.push(
        vscode.workspace.onDidCloseTextDocument(doc => {
            diagnosticCollection.delete(doc.uri);
        })
    );

    // Register Commands
    context.subscriptions.push(
        vscode.commands.registerCommand('aven.check', () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor || editor.document.languageId !== 'aven') {
                vscode.window.showWarningMessage('Open an .aven file to check.');
                return;
            }
            updateDiagnostics(editor.document, diagnosticCollection, outputChannel);
            vscode.window.showInformationMessage(`Checking Aven model: ${path.basename(editor.document.fileName)}`);
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('aven.run', () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor || editor.document.languageId !== 'aven') {
                vscode.window.showWarningMessage('Open an .aven file to run.');
                return;
            }

            const filePath = editor.document.uri.fsPath;
            const avenc = findAvencBinary();

            outputChannel.clear();
            outputChannel.show(true);
            outputChannel.appendLine(`>>> Running: ${avenc} run "${filePath}"\n`);

            const child = spawn(avenc, ['run', filePath], {
                cwd: path.dirname(filePath),
                env: process.env
            });

            child.stdout.on('data', data => outputChannel.append(data.toString()));
            child.stderr.on('data', data => outputChannel.append(data.toString()));

            child.on('close', code => {
                outputChannel.appendLine(`\nProcess exited with code ${code}.`);
            });
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('aven.why', async () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor || editor.document.languageId !== 'aven') {
                vscode.window.showWarningMessage('Open an .aven file to explain causal lineage.');
                return;
            }

            const varName = await vscode.window.showInputBox({
                prompt: 'Enter the variable name to explain causal derivation for (e.g. "cooling", "drives", "cost"):',
                placeHolder: 'variable_name'
            });

            if (!varName) {
                return;
            }

            const filePath = editor.document.uri.fsPath;
            const avenc = findAvencBinary();

            outputChannel.clear();
            outputChannel.show(true);
            outputChannel.appendLine(`>>> Explaining causal lineage: ${avenc} why "${filePath}" ${varName}\n`);

            const child = spawn(avenc, ['why', filePath, varName], {
                cwd: path.dirname(filePath),
                env: process.env
            });

            child.stdout.on('data', data => outputChannel.append(data.toString()));
            child.stderr.on('data', data => outputChannel.append(data.toString()));

            child.on('close', code => {
                outputChannel.appendLine(`\nProcess exited with code ${code}.`);
            });
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('aven.buildC', () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor || editor.document.languageId !== 'aven') {
                vscode.window.showWarningMessage('Open an .aven file to compile to C.');
                return;
            }

            const filePath = editor.document.uri.fsPath;
            const outC = filePath.replace(/\.aven$/, '.c');
            const avenc = findAvencBinary();

            outputChannel.clear();
            outputChannel.show(true);
            outputChannel.appendLine(`>>> Compiling to C: ${avenc} build "${filePath}" --emit-c -o "${outC}"\n`);

            const child = spawn(avenc, ['build', filePath, '--emit-c', '-o', outC], {
                cwd: path.dirname(filePath),
                env: process.env
            });

            child.stdout.on('data', data => outputChannel.append(data.toString()));
            child.stderr.on('data', data => outputChannel.append(data.toString()));

            child.on('close', code => {
                if (code === 0) {
                    vscode.window.showInformationMessage(`Successfully generated C99 code: ${path.basename(outC)}`);
                } else {
                    vscode.window.showErrorMessage(`C code generation failed. See Output Channel.`);
                }
            });
        })
    );
}

function deactivate() {}

module.exports = {
    activate,
    deactivate
};
