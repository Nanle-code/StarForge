/**
 * StarForge VS Code Task Provider.
 *
 * Registers tasks of type "starforge" so that tasks.json entries like:
 *
 *   { "type": "starforge", "task": "build" }
 *   { "type": "starforge", "task": "test" }
 *   { "type": "starforge", "task": "deploy", "args": ["--wasm", "target/..."] }
 *
 * are resolved into ShellExecutions that attach the starforge-build problem
 * matcher for automatic error / warning highlighting in the Problems panel.
 */

import * as vscode from "vscode";

// The task definition type must match the `type` property in package.json
// `contributes.taskDefinitions`.
const TASK_TYPE = "starforge";

interface StarforgeTaskDefinition extends vscode.TaskDefinition {
  /** One of: build | test | deploy */
  task: "build" | "test" | "deploy";
  /** Extra CLI args to append */
  args?: string[];
}

// Map task name → CLI sub-command args
const TASK_ARGS: Record<string, string[]> = {
  build: ["contract", "build"],
  test: ["contract", "test"],
  deploy: ["deploy"],
};

// Map task name → VS Code TaskGroup
const TASK_GROUP: Record<string, vscode.TaskGroup> = {
  build: vscode.TaskGroup.Build,
  test: vscode.TaskGroup.Test,
  deploy: vscode.TaskGroup.Build,
};

export class StarforgeTaskProvider implements vscode.TaskProvider {
  static readonly type = TASK_TYPE;

  provideTasks(): vscode.Task[] {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders) {
      return [];
    }

    const tasks: vscode.Task[] = [];
    for (const folder of folders) {
      for (const name of ["build", "test"] as const) {
        tasks.push(makeTask({ type: TASK_TYPE, task: name }, folder));
      }
    }
    return tasks;
  }

  resolveTask(task: vscode.Task): vscode.Task | undefined {
    const def = task.definition as StarforgeTaskDefinition;
    if (!def.task || !TASK_ARGS[def.task]) {
      return undefined;
    }

    // resolveTask must use the same TaskDefinition object the user provided.
    const folder = task.scope as vscode.WorkspaceFolder;
    if (!folder || !folder.uri) {
      return undefined;
    }
    return makeTask(def, folder);
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function getBinaryPath(): string {
  return (
    vscode.workspace
      .getConfiguration("starforge")
      .get<string>("binaryPath") ?? "starforge"
  );
}

function makeTask(
  def: StarforgeTaskDefinition,
  folder: vscode.WorkspaceFolder
): vscode.Task {
  const bin = getBinaryPath();
  const cliArgs = [...(TASK_ARGS[def.task] ?? []), ...(def.args ?? [])];

  const exec = new vscode.ShellExecution(bin, cliArgs, {
    cwd: folder.uri.fsPath,
  });

  const task = new vscode.Task(
    def,
    folder,
    def.task,
    "StarForge",
    exec,
    // Always attach both matchers so both errors and warnings are caught.
    ["$starforge-build", "$starforge-warning"]
  );

  task.group = TASK_GROUP[def.task] ?? vscode.TaskGroup.Build;
  task.presentationOptions = {
    reveal: vscode.TaskRevealKind.Always,
    panel: vscode.TaskPanelKind.Shared,
    showReuseMessage: false,
    clear: false,
  };

  return task;
}
