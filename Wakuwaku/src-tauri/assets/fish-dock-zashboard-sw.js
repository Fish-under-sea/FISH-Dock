// Fish Dock embeds zashboard in a WebView; its stock service worker would keep
// serving a cached index.html and hide this directory's style patch. Replace it
// with a one-shot cleanup: no PWA cache is needed inside the dock.
self.addEventListener('install', () => self.skipWaiting());

self.addEventListener('activate', event => {
  event.waitUntil((async () => {
    const keys = await caches.keys();
    await Promise.all(keys.map(key => caches.delete(key)));
    await self.registration.unregister();
    const clients = await self.clients.matchAll({ type: 'window' });
    for (const client of clients) client.navigate(client.url);
  })());
});
