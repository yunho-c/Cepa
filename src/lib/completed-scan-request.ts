export interface CompletedScanRequest {
  scanId: number;
  sequence: number;
}

export interface CompletedScanRequestState {
  scanId: number | null;
  sequence: number;
  complete: boolean;
}

export function isCurrentCompletedScanRequest(
  request: CompletedScanRequest,
  current: CompletedScanRequestState,
): boolean {
  return (
    current.complete &&
    current.scanId === request.scanId &&
    current.sequence === request.sequence
  );
}
