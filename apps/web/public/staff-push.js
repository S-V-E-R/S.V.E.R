/* The site's only service worker (one per scope): staff alerts and go-live alerts. Staff pushes
   contain no case details. A stable tag replaces retry duplicates. No page caching. */
self.addEventListener("push", event => {
  const data = event.data ? event.data.json() : {};
  const url = typeof data.url === "string" && data.url.startsWith("/") && !data.url.startsWith("//") ? data.url : "/admin/take-it-down";
  event.waitUntil(self.registration.showNotification(data.title || "S.V.E.R staff alert", {
    body: data.body || "Check urgent removal requests in the staff console.",
    tag: data.tag || `sver-staff-${String(data.id || "urgent")}`,
    renotify: false,
    data: { url },
    icon: "/icons/icon-192.png",
  }));
});
self.addEventListener("notificationclick", event => {
  event.notification.close();
  event.waitUntil(clients.openWindow(event.notification.data?.url || "/admin/take-it-down"));
});
