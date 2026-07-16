import {
  scanProgressPresentation,
  type ScanPhase,
  type ScanProgress,
} from "./scanner";

export const SCAN_PROGRESS_ANNOUNCEMENT_INTERVAL_MS = 2_000;

export interface ScanProgressAnnouncementCheckpoint {
  phase: ScanPhase;
  cancelling: boolean;
  announcedElapsedMs: number;
}

export interface ScanProgressAnnouncementUpdate {
  checkpoint: ScanProgressAnnouncementCheckpoint;
  announcement: string;
}

/**
 * Keep the visual scan current without asking a polite live region to speak at
 * the bridge's much faster rendering cadence. The first useful update and
 * lifecycle transitions remain immediate; ordinary traversal waits at least
 * two seconds between announcements.
 */
export function nextScanProgressAnnouncement(
  previous: ScanProgressAnnouncementCheckpoint | null,
  progress: Pick<
    ScanProgress,
    "phase" | "entriesScanned" | "allocatedBytes" | "elapsedMs"
  >,
  cancelling: boolean,
): ScanProgressAnnouncementUpdate | null {
  const announcement = scanProgressPresentation(progress, cancelling).announcement;
  if (!announcement) return null;

  const checkpoint: ScanProgressAnnouncementCheckpoint = {
    phase: progress.phase,
    cancelling,
    announcedElapsedMs: Math.max(0, progress.elapsedMs),
  };
  if (previous?.phase === checkpoint.phase && previous.cancelling === cancelling) {
    const elapsedSinceAnnouncement =
      checkpoint.announcedElapsedMs - previous.announcedElapsedMs;
    if (
      elapsedSinceAnnouncement >= 0 &&
      elapsedSinceAnnouncement < SCAN_PROGRESS_ANNOUNCEMENT_INTERVAL_MS
    ) {
      return null;
    }
  }

  return { checkpoint, announcement };
}
