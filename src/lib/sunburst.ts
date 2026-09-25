import { metricBytes, type ChartItem, type SizeMetric } from "$lib/scanner";

const CENTER = 170;
const INNER_RADIUS = 47;
const RING_WIDTH = 37;
const RING_GAP = 3;
const ANGLE_GAP = 0.012;

export interface SunburstSegment {
  key: string;
  item: ChartItem;
  depth: number;
  pathData: string;
  colorIndex: number;
}

const NAVIGATION_KEYS = new Set([
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "Home",
  "End",
]);

export function createSunburst(
  items: ChartItem[],
  metric: SizeMetric = "allocated",
): SunburstSegment[] {
  const segments: SunburstSegment[] = [];
  appendSegments(
    segments,
    items,
    metric,
    0,
    -Math.PI / 2,
    Math.PI * 1.5,
    0,
    "",
  );
  return segments;
}

export function sunburstNavigationTarget(
  segments: readonly SunburstSegment[],
  currentId: number | null,
  key: string,
): number | null {
  if (!NAVIGATION_KEYS.has(key)) return null;

  const ids = segments.flatMap((segment) =>
    segment.item.id === null ? [] : [segment.item.id],
  );
  if (ids.length === 0) return null;
  if (key === "Home") return ids[0];
  if (key === "End") return ids.at(-1) ?? null;

  const currentIndex = currentId === null ? -1 : ids.indexOf(currentId);
  if (key === "ArrowRight" || key === "ArrowDown") {
    return ids[(currentIndex + 1 + ids.length) % ids.length];
  }

  const previousIndex = currentIndex <= 0 ? ids.length - 1 : currentIndex - 1;
  return ids[previousIndex];
}

function appendSegments(
  output: SunburstSegment[],
  items: ChartItem[],
  metric: SizeMetric,
  depth: number,
  startAngle: number,
  endAngle: number,
  colorSeed: number,
  keyPrefix: string,
) {
  const weights = items.map((item) => itemWeight(item, metric));
  const total = weights.reduce((sum, weight) => sum + weight, 0);
  if (total <= 0) return;

  let cursor = startAngle;
  items.forEach((item, index) => {
    const keyPath = keyPrefix ? `${keyPrefix}.${index}` : String(index);
    const span = ((endAngle - startAngle) * weights[index]) / total;
    const itemStart = cursor;
    const itemEnd = cursor + span;
    cursor = itemEnd;

    if (span > ANGLE_GAP * 1.5) {
      const inner = INNER_RADIUS + depth * (RING_WIDTH + RING_GAP);
      output.push({
        key: item.id === null ? `aggregate-${keyPath}` : `node-${item.id}`,
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
        metric,
        depth + 1,
        itemStart,
        itemEnd,
        colorSeed + index,
        keyPath,
      );
    }
  });
}

function itemWeight(item: ChartItem, metric: SizeMetric): number {
  // Suppressed items still contribute to aggregate byte coverage, but an
  // aggregate with no bytes in this metric must not create a phantom segment.
  if (item.id === null) return metricBytes(item, metric);
  return Math.max(metricBytes(item, metric), 1);
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
