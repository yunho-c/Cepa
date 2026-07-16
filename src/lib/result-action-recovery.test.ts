import { describe, expect, test } from "bun:test";
import {
  navigationRecoveryMessage,
  type NavigationRecovery,
} from "./result-action-recovery";

describe("completed-result action recovery", () => {
  test("describes the result that remains safe after each failed action", () => {
    const recoveries: Array<[NavigationRecovery | null, string]> = [
      [
        {
          kind: "directory",
          nodeId: 7,
          metric: "allocated",
          focusHeading: true,
        },
        "The current folder is still open. Try again when you’re ready.",
      ],
      [
        {
          kind: "metric",
          nodeId: 7,
          metric: "logical",
          focusHeading: false,
        },
        "The current size view is unchanged. Try again when you’re ready.",
      ],
      [
        { kind: "chooseDirectory" },
        "The current scan is unchanged. Try opening the folder picker again.",
      ],
      [
        { kind: "discardScan" },
        "The scan is still open. Try again to return home.",
      ],
      [null, "The current scan is unchanged."],
    ];

    for (const [recovery, message] of recoveries) {
      expect(navigationRecoveryMessage(recovery)).toBe(message);
    }
  });
});
