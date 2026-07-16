export type ScanEntryFailure = "folder" | "picker" | "drop";

export function scanEntryRecoveryMessage(failure: ScanEntryFailure): string {
  switch (failure) {
    case "folder":
      return "Choose another folder or check that this one is still available.";
    case "picker":
      return "Try opening the folder picker again, or enter a path instead.";
    case "drop":
      return "Drop a single folder and try again.";
  }
}

export function scanFailurePresentation(started: boolean): {
  heading: string;
  guidance: string;
} {
  return started
    ? {
        heading: "The scan couldn’t finish.",
        guidance: "No results were saved. Choose another folder or try again.",
      }
    : {
        heading: "That scan didn’t start.",
        guidance: "Check the folder and try again.",
      };
}
