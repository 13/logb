import { describe, expect, it } from 'vitest';
import { base64UrlToBytes, pushSupported } from '../src/lib/push';

describe('push', () => {
  it('turns the server key back into bytes, padding and alphabet included', () => {
    // "hello?" in standard base64 is aGVsbG8/ -- base64url spells the slash as an underscore
    // and drops the padding, and both have to be undone.
    expect([...base64UrlToBytes('aGVsbG8_')]).toEqual([...new TextEncoder().encode('hello?')]);
    expect([...base64UrlToBytes('YQ')]).toEqual([97]);
  });

  it('an uncompressed P-256 key comes out 65 bytes long, starting 0x04', () => {
    const key = 'BLn9b-VR0ca83knDNZ32dCHGyjJp-1riX9ZTN40MqV8K_LpQmLqxC_DoHvqvFXO_nGdAB4W9dogZb_sM-uV4JbY';
    const bytes = base64UrlToBytes(key);
    expect(bytes.length).toBe(65);
    expect(bytes[0]).toBe(4);
  });

  it('reports no support where there is no browser', () => {
    expect(pushSupported()).toBe(false);
  });
});
