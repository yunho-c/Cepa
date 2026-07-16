export type AppStatus =
  | "idle"
  | "scanning"
  | "cancelling"
  | "cancelled"
  | "complete"
  | "error";

export function brandActsAsHome(status: AppStatus): boolean {
  return status === "complete";
}
