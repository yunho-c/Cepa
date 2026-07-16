export interface InspectorAnnouncementState {
  itemName: string | null;
  isInspectingCompression: boolean;
  compressionLabel: string | null;
  isEstimatingSavings: boolean;
  isCancellingEstimate: boolean;
  estimateLabel: string | null;
  hasEstimateStopError: boolean;
}

export function inspectorAnnouncement(
  state: InspectorAnnouncementState,
): string {
  if (state.itemName === null || state.hasEstimateStopError) return "";
  if (state.isCancellingEstimate) {
    return `Stopping the savings estimate for ${state.itemName}.`;
  }
  if (state.isEstimatingSavings) {
    return `Estimating potential savings for ${state.itemName}.`;
  }
  if (state.estimateLabel !== null) {
    return `Potential savings for ${state.itemName}: ${state.estimateLabel}.`;
  }
  if (state.isInspectingCompression) {
    return `Checking compression for ${state.itemName}.`;
  }
  if (state.compressionLabel !== null) {
    return `Compression for ${state.itemName}: ${state.compressionLabel}.`;
  }
  return "";
}
