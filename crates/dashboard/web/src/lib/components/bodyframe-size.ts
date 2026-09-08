// Height decision for the auto-sizing message iframe (BodyFrame).
//
// This is the guard that keeps the composer/reader scrollable. The frame is
// driven by a ResizeObserver, which can fire many times a second. The previous
// sizer collapsed the frame to 0px to measure, then wrote a height on EVERY
// callback — and each collapse shrank the scroll container, so the browser
// clamped scrollTop and the page snapped back to the top, ~250 times a second.
// The operator could not scroll past the frame at all.
//
// Two rules break that: never derive height from a collapsed frame (the caller
// measures a content wrapper instead), and only return a height when it has
// actually changed. A steady frame therefore writes nothing, the observer
// quiesces, and scroll position is left alone.

/** Slack added below the measured content so nothing sits flush against the edge. */
export const FRAME_HEIGHT_PAD = 16;
/** Hard ceiling: a pathological email can report an enormous height. */
export const FRAME_HEIGHT_MAX = 20000;
/** Sub-pixel measurement noise that must not trigger a rewrite. */
export const FRAME_HEIGHT_EPSILON = 1;

/**
 * The height the frame should have for `contentHeight`, or `null` when the
 * current height already fits and nothing should be written.
 *
 * `null` is the important case: returning it on an unchanged frame is what
 * stops the observer feedback loop. A positive content height never resolves
 * to 0, so the frame is never collapsed.
 */
export function resolveFrameHeight(
  contentHeight: number,
  currentHeight: number,
  pad: number = FRAME_HEIGHT_PAD,
  max: number = FRAME_HEIGHT_MAX
): number | null {
  if (!Number.isFinite(contentHeight) || contentHeight <= 0) return null;
  const target = Math.min(Math.ceil(contentHeight) + pad, max);
  if (Math.abs(target - currentHeight) <= FRAME_HEIGHT_EPSILON) return null;
  return target;
}
