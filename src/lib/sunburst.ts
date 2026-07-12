import type { ChartItem } from "$lib/scanner";

const CENTER = 170;
const INNER_RADIUS = 47;
const RING_WIDTH = 37;
const RING_GAP = 3;
const ANGLE_GAP = 0.012;

export interface SunburstSegment {
  item: ChartItem;
  depth: number;
  pathData: string;
  colorIndex: number;
}

export function createSunburst(items: ChartItem[]): SunburstSegment[] {
  const segments: SunburstSegment[] = [];
  appendSegments(segments, items, 0, -Math.PI / 2, Math.PI * 1.5, 0);
  return segments;
}

function appendSegments(
  output: SunburstSegment[],
  items: ChartItem[],
  depth: number,
  startAngle: number,
  endAngle: number,
  colorSeed: number,
) {
  const weights = items.map(itemWeight);
  const total = weights.reduce((sum, weight) => sum + weight, 0);
  if (total <= 0) return;

  let cursor = startAngle;
  items.forEach((item, index) => {
    const span = ((endAngle - startAngle) * weights[index]) / total;
    const itemStart = cursor;
    const itemEnd = cursor + span;
    cursor = itemEnd;

    if (span > ANGLE_GAP * 1.5) {
      const inner = INNER_RADIUS + depth * (RING_WIDTH + RING_GAP);
      output.push({
        item,
        depth,
        pathData: ringArc(
          inner,
          inner + RING_WIDTH,
          itemStart + ANGLE_GAP / 2,
          itemEnd - ANGLE_GAP / 2,
        ),
        colorIndex: (colorSeed + index) % 8,
      });
    }

    if (item.children.length > 0) {
      appendSegments(
        output,
        item.children,
        depth + 1,
        itemStart,
        itemEnd,
        colorSeed + index,
      );
    }
  });
}

function itemWeight(item: ChartItem): number {
  return Math.max(item.allocatedBytes, item.logicalBytes, 1);
}

function ringArc(
  innerRadius: number,
  outerRadius: number,
  startAngle: number,
  endAngle: number,
): string {
  const safeEnd = Math.min(endAngle, startAngle + Math.PI * 2 - 0.0001);
  const outerStart = polar(outerRadius, startAngle);
  const outerEnd = polar(outerRadius, safeEnd);
  const innerEnd = polar(innerRadius, safeEnd);
  const innerStart = polar(innerRadius, startAngle);
  const largeArc = safeEnd - startAngle > Math.PI ? 1 : 0;

  return [
    `M ${outerStart.x} ${outerStart.y}`,
    `A ${outerRadius} ${outerRadius} 0 ${largeArc} 1 ${outerEnd.x} ${outerEnd.y}`,
    `L ${innerEnd.x} ${innerEnd.y}`,
    `A ${innerRadius} ${innerRadius} 0 ${largeArc} 0 ${innerStart.x} ${innerStart.y}`,
    "Z",
  ].join(" ");
}

function polar(radius: number, angle: number) {
  return {
    x: Number((CENTER + radius * Math.cos(angle)).toFixed(3)),
    y: Number((CENTER + radius * Math.sin(angle)).toFixed(3)),
  };
}
