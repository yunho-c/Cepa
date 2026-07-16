import { describe, expect, test } from "bun:test";
import {
  nextScanProgressAnnouncement,
  SCAN_PROGRESS_ANNOUNCEMENT_INTERVAL_MS,
  type ScanProgressAnnouncementCheckpoint,
} from "./scan-progress-announcement";

const progress = {
  phase: "scanning" as const,
  entriesScanned: 1,
  allocatedBytes: 0,
  elapsedMs: 25,
};

describe("scan progress announcements", () => {
  test("waits for useful progress and publishes the first observation", () => {
    expect(
      nextScanProgressAnnouncement(
        null,
        { ...progress, entriesScanned: 0, elapsedMs: 0 },
        false,
      ),
    ).toBeNull();

    const update = nextScanProgressAnnouncement(null, progress, false);
    expect(update?.announcement).toBe("Scanned 1 entry and 0 B.");
    expect(update?.checkpoint).toEqual({
      phase: "scanning",
      cancelling: false,
      announcedElapsedMs: 25,
    });
  });

  test("waits two seconds between ordinary visual updates", () => {
    const first = nextScanProgressAnnouncement(null, progress, false)!;
    expect(
      nextScanProgressAnnouncement(
        first.checkpoint,
        {
          ...progress,
          entriesScanned: 18_432,
          allocatedBytes: 302_795_292_672,
          elapsedMs:
            progress.elapsedMs + SCAN_PROGRESS_ANNOUNCEMENT_INTERVAL_MS - 1,
        },
        false,
      ),
    ).toBeNull();

    const next = nextScanProgressAnnouncement(
      first.checkpoint,
      {
        ...progress,
        entriesScanned: 20_000,
        allocatedBytes: 322_122_547_200,
        elapsedMs: progress.elapsedMs + SCAN_PROGRESS_ANNOUNCEMENT_INTERVAL_MS,
      },
      false,
    );
    expect(next?.announcement).toBe("Scanned 20K entries and 300 GB.");
    expect(next?.checkpoint.announcedElapsedMs).toBe(2_025);
  });

  test("publishes finishing and stopping transitions immediately", () => {
    const previous: ScanProgressAnnouncementCheckpoint = {
      phase: "scanning",
      cancelling: false,
      announcedElapsedMs: 8_000,
    };
    const finishing = nextScanProgressAnnouncement(
      previous,
      {
        ...progress,
        phase: "finishing",
        entriesScanned: 18_432,
        elapsedMs: 8_100,
      },
      false,
    )!;
    expect(finishing.announcement).toBe(
      "Finishing the scan after 18K entries.",
    );

    const stopping = nextScanProgressAnnouncement(
      finishing.checkpoint,
      {
        ...progress,
        phase: "finishing",
        entriesScanned: 18_432,
        elapsedMs: 8_150,
      },
      true,
    );
    expect(stopping?.announcement).toBe(
      "Stopping while preparing results for 18K entries.",
    );
  });
});
