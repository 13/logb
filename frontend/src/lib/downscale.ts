/**
 * Shrinks a photo in the browser before it is uploaded or queued. A phone photo is 3-8 MB, and
 * on the mobile connection most photos are taken on that is most of the time Save spends
 * waiting; the server only ever shows it as a thumbnail or on a screen, where 2560 px on the
 * long edge is already more than enough.
 *
 * The server reads the capture date (`taken_at`) from the file's own Exif (src/files.rs), and a
 * canvas writes none, so for a JPEG the original's Exif segment is copied into the new file
 * (`carryExif`). Other formats cannot be carried over that way: a HEIC, PNG or WebP above
 * `KEEP_BELOW_BYTES` is converted without its capture date. Below that size a file is sent as
 * it is -- it costs little to upload, and it keeps everything it had.
 */

export const MAX_EDGE = 2560;
export const QUALITY = 0.85;
export const KEEP_BELOW_BYTES = 1.5 * 1024 * 1024;

/** Formats a browser can plausibly decode into a canvas. HEIC only on Safari; elsewhere its
 *  decode fails and the original goes up unchanged. GIF is left out: it may be animated. */
const SHRINKABLE = new Set(['image/jpeg', 'image/png', 'image/webp', 'image/heic', 'image/heif']);

export function fittedSize(width: number, height: number, maxEdge = MAX_EDGE): { width: number; height: number } {
  const scale = Math.min(1, maxEdge / Math.max(width, height));
  return { width: Math.round(width * scale), height: Math.round(height * scale) };
}

/** The same name with a `.jpg` extension, for a file whose type the conversion changed -- the
 *  server takes the type from the extension first (`resolve_mime` in src/files.rs). */
export function jpegName(name: string): string {
  const dot = name.lastIndexOf('.');
  return `${dot > 0 ? name.slice(0, dot) : name}.jpg`;
}

/** Where the Exif APP1 segment of a JPEG starts and ends (marker included), or null. */
function exifSegment(bytes: Uint8Array): { start: number; end: number } | null {
  if (bytes[0] !== 0xff || bytes[1] !== 0xd8) return null;
  let i = 2;
  while (i + 4 <= bytes.length && bytes[i] === 0xff) {
    const marker = bytes[i + 1];
    if (marker === 0xda || marker === 0xd9) return null; // image data: no more metadata
    const end = i + 2 + ((bytes[i + 2] << 8) | bytes[i + 3]);
    const isExif = marker === 0xe1 && String.fromCharCode(...bytes.subarray(i + 4, i + 8)) === 'Exif'
      && bytes[i + 8] === 0 && bytes[i + 9] === 0;
    if (isExif) return end <= bytes.length ? { start: i, end } : null;
    i = end;
  }
  return null;
}

/** Sets IFD0's Orientation tag to 1 ("upright") in a copied Exif segment, in place. The pixels
 *  were already turned upright on decode, so an orientation left at, say, 6 would have the
 *  server's thumbnail and every viewer turn them a second time. */
function resetOrientation(segment: Uint8Array): void {
  const tiff = 10; // marker (2) + length (2) + "Exif\0\0" (6)
  if (segment.length < tiff + 8) return;
  const view = new DataView(segment.buffer, segment.byteOffset, segment.byteLength);
  const le = view.getUint16(tiff) === 0x4949;
  const ifd0 = tiff + view.getUint32(tiff + 4, le);
  if (ifd0 + 2 > segment.length) return;
  const entries = view.getUint16(ifd0, le);
  for (let e = 0; e < entries; e++) {
    const at = ifd0 + 2 + e * 12;
    if (at + 12 > segment.length) return;
    if (view.getUint16(at, le) === 0x0112) {
      view.setUint16(at + 8, 1, le);
      return;
    }
  }
}

/** `encoded` with the Exif segment of `original` put back in right after its SOI marker, and
 *  that segment's orientation reset. `encoded` itself when the original carries no Exif. */
export function carryExif(original: Uint8Array, encoded: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer> {
  const found = exifSegment(original);
  if (!found) return encoded;
  const segment = original.slice(found.start, found.end);
  resetOrientation(segment);
  const out = new Uint8Array(encoded.length + segment.length);
  out.set(encoded.subarray(0, 2));
  out.set(segment, 2);
  out.set(encoded.subarray(2), 2 + segment.length);
  return out;
}

type Decoded = { width: number; height: number; close(): void };

/** The browser half, injected so the decisions above it can be tested without a canvas. */
export interface ShrinkDeps {
  decode(file: Blob): Promise<Decoded>;
  encode(image: Decoded, width: number, height: number): Promise<Blob>;
}

const browserDeps: ShrinkDeps = {
  // `from-image` turns the pixels by the Exif orientation, so a portrait photo stays portrait.
  decode: (file) => createImageBitmap(file, { imageOrientation: 'from-image' }),
  async encode(image, width, height) {
    const canvas = typeof OffscreenCanvas === 'function'
      ? new OffscreenCanvas(width, height)
      : Object.assign(document.createElement('canvas'), { width, height });
    const ctx = canvas.getContext('2d') as CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null;
    if (!ctx) throw new Error('no 2d context');
    ctx.fillStyle = '#fff'; // a transparent PNG would otherwise turn black in a JPEG
    ctx.fillRect(0, 0, width, height);
    ctx.imageSmoothingQuality = 'high';
    ctx.drawImage(image as ImageBitmap, 0, 0, width, height);
    if ('convertToBlob' in canvas) return canvas.convertToBlob({ type: 'image/jpeg', quality: QUALITY });
    return new Promise<Blob>((resolve, reject) =>
      canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('encode failed'))), 'image/jpeg', QUALITY));
  },
};

/**
 * `file` as a JPEG no larger than `MAX_EDGE` on its long edge, or `file` itself whenever that
 * would not help: not a photo, already small, not decodable here, or not smaller once re-encoded.
 * Never throws -- the original is always a fine thing to upload.
 */
export async function shrinkImage(file: File, deps: ShrinkDeps = browserDeps): Promise<File> {
  if (!SHRINKABLE.has(file.type) || file.size <= KEEP_BELOW_BYTES) return file;
  try {
    const image = await deps.decode(file);
    let encoded: Blob;
    try {
      const { width, height } = fittedSize(image.width, image.height);
      encoded = await deps.encode(image, width, height);
    } finally {
      image.close();
    }
    let bytes = new Uint8Array(await encoded.arrayBuffer());
    if (file.type === 'image/jpeg') bytes = carryExif(new Uint8Array(await file.arrayBuffer()), bytes);
    if (bytes.length >= file.size) return file;
    const name = file.type === 'image/jpeg' ? file.name : jpegName(file.name);
    return new File([bytes], name, { type: 'image/jpeg', lastModified: file.lastModified });
  } catch {
    return file;
  }
}
