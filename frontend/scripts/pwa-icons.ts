/** The manifest's icon list, kept here rather than inline in vite.config.ts so a test can
 *  import it and check every file it names actually exists. */
export const pwaIcons: { src: string; sizes: string; type: string; purpose?: string }[] = [
  { src: 'icon.svg', sizes: 'any', type: 'image/svg+xml', purpose: 'any' },
  { src: 'pwa-192.png', sizes: '192x192', type: 'image/png' },
  { src: 'pwa-512.png', sizes: '512x512', type: 'image/png', purpose: 'any' },
  { src: 'pwa-512-maskable.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
];
