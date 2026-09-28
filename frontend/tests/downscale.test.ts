import { describe, expect, it, vi } from 'vitest';
import { carryExif, fittedSize, jpegName, KEEP_BELOW_BYTES, MAX_EDGE, shrinkImage, type ShrinkDeps } from '../src/lib/downscale';

/** A JPEG's segments, as bytes: SOI, whatever is passed, a token scan, EOI. */
function jpeg(...segments: number[][]): Uint8Array<ArrayBuffer> {
  return new Uint8Array([0xff, 0xd8, ...segments.flat(), 0xff, 0xda, 0x00, 0x04, 0x01, 0x02, 0x33, 0x44, 0xff, 0xd9]);
}

function segment(marker: number, payload: number[]): number[] {
  const len = payload.length + 2;
  return [0xff, marker, len >> 8, len & 0xff, ...payload];
}

const EXIF = [0x45, 0x78, 0x69, 0x66, 0x00, 0x00]; // "Exif\0\0"

/** An Exif APP1 whose IFD0 holds Orientation (0x0112) plus one other SHORT tag, in either byte order. */
function exifApp1(orientation: number, littleEndian: boolean): number[] {
  const u16 = (n: number) => (littleEndian ? [n & 0xff, n >> 8] : [n >> 8, n & 0xff]);
  const u32 = (n: number) => (littleEndian ? [n & 0xff, (n >> 8) & 0xff, (n >> 16) & 0xff, n >>> 24] : [n >>> 24, (n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff]);
  const tiff = [
    ...(littleEndian ? [0x49, 0x49] : [0x4d, 0x4d]), ...u16(42), ...u32(8),
    ...u16(2),
    ...u16(0x0128), ...u16(3), ...u32(1), ...u16(2), 0, 0, // ResolutionUnit, left alone
    ...u16(0x0112), ...u16(3), ...u32(1), ...u16(orientation), 0, 0,
    ...u32(0),
  ];
  return segment(0xe1, [...EXIF, ...tiff]);
}

/** The Orientation value in the first Exif APP1 of `bytes`, or null. */
function orientationIn(bytes: Uint8Array): number | null {
  for (let i = 2; i + 4 < bytes.length;) {
    const marker = bytes[i + 1];
    const len = (bytes[i + 2] << 8) | bytes[i + 3];
    if (marker === 0xe1) {
      const t = i + 4 + 6;
      const le = bytes[t] === 0x49;
      const u16 = (o: number) => (le ? bytes[t + o] | (bytes[t + o + 1] << 8) : (bytes[t + o] << 8) | bytes[t + o + 1]);
      const entries = u16(8);
      for (let e = 0; e < entries; e++) {
        const at = 10 + e * 12;
        if (u16(at) === 0x0112) return u16(at + 8);
      }
      return null;
    }
    if (marker === 0xda) return null;
    i += 2 + len;
  }
  return null;
}

describe('fittedSize', () => {
  it('scales the long edge down to the limit, keeping the aspect ratio', () => {
    expect(fittedSize(4032, 3024)).toEqual({ width: MAX_EDGE, height: 1920 });
    expect(fittedSize(3024, 4032)).toEqual({ width: 1920, height: MAX_EDGE });
  });

  it('leaves an image that already fits at its own size', () => {
    expect(fittedSize(1600, 1200)).toEqual({ width: 1600, height: 1200 });
  });
});

describe('jpegName', () => {
  it('keeps the name and swaps only the extension', () => {
    expect(jpegName('IMG_0042.HEIC')).toBe('IMG_0042.jpg');
    expect(jpegName('scan.of.receipt.png')).toBe('scan.of.receipt.jpg');
    expect(jpegName('photo')).toBe('photo.jpg');
  });
});

describe('carryExif', () => {
  const encoded = jpeg(segment(0xe0, [0x4a, 0x46, 0x49, 0x46, 0x00, 1, 1, 0, 0, 1, 0, 1, 0, 0]));

  for (const littleEndian of [true, false]) {
    it(`copies the original's Exif into the re-encoded file with orientation reset (${littleEndian ? 'Intel' : 'Motorola'} order)`, () => {
      const original = jpeg(exifApp1(6, littleEndian));
      const out = carryExif(original, encoded);
      expect([...out.slice(0, 2)]).toEqual([0xff, 0xd8]);
      // The pixels are already turned upright, so the tag must say so -- or the server's
      // thumbnail and every viewer would turn them a second time.
      expect(orientationIn(out)).toBe(1);
      expect(out.length).toBe(encoded.length + exifApp1(6, littleEndian).length);
      expect([...out.slice(out.length - encoded.length + 2)]).toEqual([...encoded.slice(2)]);
      expect(orientationIn(original)).toBe(6); // the original is not touched
    });
  }

  it('returns the re-encoded file unchanged when the original has no Exif', () => {
    expect(carryExif(jpeg(), encoded)).toBe(encoded);
    expect(carryExif(new Uint8Array([1, 2, 3]), encoded)).toBe(encoded);
  });
});

describe('shrinkImage', () => {
  const big = KEEP_BELOW_BYTES + 1;

  function file(bytes: Uint8Array<ArrayBuffer> | number, name: string, type: string): File {
    const body = typeof bytes === 'number' ? new Uint8Array(bytes) : bytes;
    return new File([body], name, { type, lastModified: 1_700_000_000_000 });
  }

  function deps(over: Partial<ShrinkDeps> = {}): ShrinkDeps & { encoded: Array<{ width: number; height: number }> } {
    const encoded: Array<{ width: number; height: number }> = [];
    return {
      encoded,
      decode: vi.fn(async () => ({ width: 4000, height: 3000, close: () => {} })),
      encode: vi.fn(async (_img, width: number, height: number) => {
        encoded.push({ width, height });
        return new Blob([jpeg(segment(0xe0, [0, 0]))], { type: 'image/jpeg' });
      }),
      ...over,
    };
  }

  it('leaves a PDF alone without trying to decode it', async () => {
    const d = deps();
    const f = file(big, 'manual.pdf', 'application/pdf');
    expect(await shrinkImage(f, d)).toBe(f);
    expect(d.decode).not.toHaveBeenCalled();
  });

  it('leaves a photo that is already small enough alone', async () => {
    const d = deps();
    const f = file(KEEP_BELOW_BYTES, 'small.jpg', 'image/jpeg');
    expect(await shrinkImage(f, d)).toBe(f);
    expect(d.decode).not.toHaveBeenCalled();
  });

  it('sends the original when the browser cannot decode it', async () => {
    const f = file(big, 'IMG_1.HEIC', 'image/heic');
    expect(await shrinkImage(f, deps({ decode: async () => { throw new DOMException('no', 'InvalidStateError'); } }))).toBe(f);
  });

  it('sends the original when re-encoding does not make it smaller', async () => {
    const f = file(big, 'dense.png', 'image/png');
    expect(await shrinkImage(f, deps({ encode: async () => new Blob([new Uint8Array(big + 10)], { type: 'image/jpeg' }) }))).toBe(f);
  });

  it('scales a large PNG down to a JPEG under the same name', async () => {
    const d = deps();
    const out = await shrinkImage(file(big, 'screenshot.png', 'image/png'), d);
    expect(d.encoded).toEqual([{ width: MAX_EDGE, height: 1920 }]);
    expect(out.name).toBe('screenshot.jpg');
    expect(out.type).toBe('image/jpeg');
    expect(out.size).toBeLessThan(big);
  });

  it('keeps a large JPEG\'s name and carries its Exif over, so the server still reads the capture date', async () => {
    const original = jpeg(exifApp1(6, true), [...new Array(big).fill(0)]);
    const out = await shrinkImage(file(original, 'IMG_2001.JPG', 'image/jpeg'), deps());
    expect(out.name).toBe('IMG_2001.JPG');
    expect(out.type).toBe('image/jpeg');
    expect(orientationIn(new Uint8Array(await out.arrayBuffer()))).toBe(1);
  });
});
