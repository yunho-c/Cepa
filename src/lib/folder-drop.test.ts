import { describe, expect, test } from "bun:test";
import { droppedItemName, folderDropAction } from "./folder-drop";

describe("native folder drops", () => {
  test("activates for a native path and preserves it exactly", () => {
    expect(
      folderDropAction({ type: "enter", paths: ["/Volumes/Archive Folder"] }, false),
    ).toEqual({ kind: "activate", paths: ["/Volumes/Archive Folder"] });
    expect(
      folderDropAction({ type: "drop", paths: ["/Volumes/Archive Folder"] }, false),
    ).toEqual({ kind: "scan", path: "/Volumes/Archive Folder" });
  });

  test("rejects ambiguous drops and ignores empty entry events", () => {
    expect(folderDropAction({ type: "drop", paths: [] }, false)).toEqual({
      kind: "reject",
    });
    expect(
      folderDropAction({ type: "drop", paths: ["/one", "/two"] }, false),
    ).toEqual({ kind: "reject" });
    expect(folderDropAction({ type: "enter", paths: [""] }, false)).toEqual({
      kind: "ignore",
    });
  });

  test("never starts a new scan while another operation is active", () => {
    expect(folderDropAction({ type: "enter", paths: ["/one"] }, true)).toEqual({
      kind: "ignore",
    });
    expect(folderDropAction({ type: "drop", paths: ["/one"] }, true)).toEqual({
      kind: "deactivate",
    });
  });

  test("clears the affordance when the drag leaves", () => {
    expect(folderDropAction({ type: "over" }, false)).toEqual({ kind: "ignore" });
    expect(folderDropAction({ type: "leave" }, false)).toEqual({
      kind: "deactivate",
    });
  });

  test("labels Unix and Windows paths without exposing their full location", () => {
    expect(droppedItemName("/Users/demo/Projects/")).toBe("Projects");
    expect(droppedItemName("C:\\Users\\demo\\Projects")).toBe("Projects");
    expect(droppedItemName("/")).toBe("/");
  });
});
