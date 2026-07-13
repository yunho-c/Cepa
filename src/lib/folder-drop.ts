export type FolderDropEvent =
  | { type: "enter"; paths: string[] }
  | { type: "over" }
  | { type: "drop"; paths: string[] }
  | { type: "leave" };

export type FolderDropAction =
  | { kind: "activate"; paths: string[] }
  | { kind: "deactivate" }
  | { kind: "scan"; path: string }
  | { kind: "reject" }
  | { kind: "ignore" };

export function folderDropAction(
  event: FolderDropEvent,
  unavailable: boolean,
): FolderDropAction {
  if (unavailable) {
    return event.type === "leave" || event.type === "drop"
      ? { kind: "deactivate" }
      : { kind: "ignore" };
  }

  switch (event.type) {
    case "enter": {
      const paths = event.paths.filter((path) => path.length > 0);
      return paths.length > 0
        ? { kind: "activate", paths }
        : { kind: "ignore" };
    }
    case "over":
      return { kind: "ignore" };
    case "leave":
      return { kind: "deactivate" };
    case "drop": {
      const paths = event.paths.filter((path) => path.length > 0);
      return paths.length === 1
        ? { kind: "scan", path: paths[0] }
        : { kind: "reject" };
    }
  }
}

export function droppedItemName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  return trimmed.split(/[\\/]/).at(-1) || path;
}
