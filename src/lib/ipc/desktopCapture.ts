// Native desktop window PNG capture plugin wrapper (`desktop-capture`).
import { invokeCmd } from './client';

export interface CaptureRegion {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface CaptureWindowResult {
  pinId: string;
  screenshotRel: string;
  screenshotAt: string;
  width: number;
  height: number;
}

export function captureDesktopWindow(
  pinId: string,
  region?: CaptureRegion | null,
): Promise<CaptureWindowResult> {
  return invokeCmd<CaptureWindowResult>('plugin:desktop-capture|capture_window', {
    options: {
      pinId,
      region: region ?? null,
    },
  });
}
