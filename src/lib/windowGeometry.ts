export interface PixelPoint {
  x: number;
  y: number;
}

export interface PixelSize {
  width: number;
  height: number;
}

export interface PixelRect extends PixelPoint, PixelSize {}

type AxisAnchor = "start" | "end" | null;

function detectAxisAnchor(startGap: number, endGap: number, tolerance: number): AxisAnchor {
  const startDistance = Math.abs(startGap);
  const endDistance = Math.abs(endGap);

  if (startDistance > tolerance && endDistance > tolerance) return null;
  return startDistance <= endDistance ? "start" : "end";
}

/**
 * Keeps edges that were snapped to a monitor edge anchored after a resize.
 * All values are physical pixels so the calculation also works with display scaling.
 */
export function positionAfterAnchoredResize(
  windowPosition: PixelPoint,
  windowSize: PixelSize,
  monitorBounds: PixelRect,
  resizedWindow: PixelSize,
  tolerance: number,
): PixelPoint {
  const monitorRight = monitorBounds.x + monitorBounds.width;
  const monitorBottom = monitorBounds.y + monitorBounds.height;
  const horizontalAnchor = detectAxisAnchor(
    windowPosition.x - monitorBounds.x,
    monitorRight - (windowPosition.x + windowSize.width),
    tolerance,
  );
  const verticalAnchor = detectAxisAnchor(
    windowPosition.y - monitorBounds.y,
    monitorBottom - (windowPosition.y + windowSize.height),
    tolerance,
  );

  const x = horizontalAnchor === "start"
    ? monitorBounds.x
    : horizontalAnchor === "end"
      ? monitorRight - resizedWindow.width
      : windowPosition.x;
  const y = verticalAnchor === "start"
    ? monitorBounds.y
    : verticalAnchor === "end"
      ? monitorBottom - resizedWindow.height
      : windowPosition.y;

  return { x, y };
}
