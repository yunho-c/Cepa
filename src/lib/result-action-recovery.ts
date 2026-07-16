import type { SizeMetric } from "./scanner";

export type NavigationRecovery =
  | {
      kind: "directory";
      nodeId: number;
      metric: SizeMetric;
      focusHeading: true;
    }
  | {
      kind: "metric";
      nodeId: number;
      metric: SizeMetric;
      focusHeading: false;
    }
  | { kind: "chooseDirectory" }
  | { kind: "discardScan" };

export function navigationRecoveryMessage(
  recovery: NavigationRecovery | null,
): string {
  switch (recovery?.kind) {
    case "directory":
      return "The current folder is still open. Try again when you’re ready.";
    case "metric":
      return "The current size view is unchanged. Try again when you’re ready.";
    case "chooseDirectory":
      return "The current scan is unchanged. Try opening the folder picker again.";
    case "discardScan":
      return "The scan is still open. Try again to return home.";
    default:
      return "The current scan is unchanged.";
  }
}
