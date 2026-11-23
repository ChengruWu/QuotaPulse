import { describe, expect, it } from "vitest";
import { positionAfterAnchoredResize } from "./windowGeometry";

const monitor = { x: 0, y: 0, width: 1920, height: 1080 };

describe("anchored widget resizing", () => {
  it("keeps a right-snapped window on the right edge when collapsing", () => {
    expect(positionAfterAnchoredResize(
      { x: 1600, y: 240 },
      { width: 320, height: 320 },
      monitor,
      { width: 100, height: 100 },
      16,
    )).toEqual({ x: 1820, y: 240 });
  });

  it("keeps a right-snapped window on the right edge when expanding", () => {
    expect(positionAfterAnchoredResize(
      { x: 1820, y: 240 },
      { width: 100, height: 100 },
      monitor,
      { width: 320, height: 320 },
      16,
    )).toEqual({ x: 1600, y: 240 });
  });

  it("preserves a snapped corner", () => {
    expect(positionAfterAnchoredResize(
      { x: 1604, y: 764 },
      { width: 320, height: 320 },
      monitor,
      { width: 100, height: 100 },
      16,
    )).toEqual({ x: 1820, y: 980 });
  });

  it("does not move a window that is not near a monitor edge", () => {
    expect(positionAfterAnchoredResize(
      { x: 700, y: 240 },
      { width: 320, height: 320 },
      monitor,
      { width: 100, height: 100 },
      16,
    )).toEqual({ x: 700, y: 240 });
  });

  it("supports monitors with negative desktop coordinates", () => {
    expect(positionAfterAnchoredResize(
      { x: -320, y: 200 },
      { width: 320, height: 320 },
      { x: -1920, y: 0, width: 1920, height: 1080 },
      { width: 100, height: 100 },
      16,
    )).toEqual({ x: -100, y: 200 });
  });
});
