// The two rules that keep the message frame from stealing your scroll position.
//
// Found by a peer session and reproduced here on real mail: the sizer collapsed
// the frame to 0px to measure, so every re-fit shrank the reader's scroll
// container, the browser clamped scrollTop, and the pane jumped to the top.
// Measured before the fix: scrolled to 1465px, forced one re-fit, back to 0.
//
// `resolveFrameHeight` is shared with the other branch's copy of this file so
// the two lineages converge rather than silently diverge.

import { describe, expect, it } from 'vitest';
import {
  resolveFrameHeight,
  FRAME_HEIGHT_PAD,
  FRAME_HEIGHT_MAX
} from './bodyframe-size';

describe('frame sizing', () => {
  it('pads the measured content height', () => {
    expect(resolveFrameHeight(2013, 320)).toBe(2013 + FRAME_HEIGHT_PAD);
  });

  it('writes nothing when the frame already fits', () => {
    // This is what stops the observer feedback loop.
    const settled = 2013 + FRAME_HEIGHT_PAD;
    expect(resolveFrameHeight(2013, settled)).toBeNull();
  });

  it('ignores sub-pixel measurement noise', () => {
    const settled = 2013 + FRAME_HEIGHT_PAD;
    expect(resolveFrameHeight(2012.6, settled)).toBeNull();
  });

  it('never resolves to zero, so the frame is never collapsed', () => {
    // A collapse is what clamped scrollTop and lost the reader's place.
    for (const content of [0, -1, NaN, Infinity]) {
      expect(resolveFrameHeight(content, 2029)).toBeNull();
    }
    // And a positive height always comes back strictly positive.
    expect(resolveFrameHeight(1, 500)).toBeGreaterThan(0);
  });

  it('shrinks as well as grows', () => {
    // Switching to a shorter message must not leave the old tall frame.
    expect(resolveFrameHeight(400, 9182)).toBe(400 + FRAME_HEIGHT_PAD);
  });

  it('clamps a pathological height', () => {
    expect(resolveFrameHeight(9_000_000, 500)).toBe(FRAME_HEIGHT_MAX);
  });

  it('settles: the height it returns is one it will then leave alone', () => {
    // The loop-termination property, stated directly.
    const first = resolveFrameHeight(2057, 320);
    expect(first).not.toBeNull();
    expect(resolveFrameHeight(2057, first as number)).toBeNull();
  });
});
