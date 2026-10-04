/* Staff push contains no case details. A stable tag replaces retry duplicates. No page caching. */
self.addEventListener("push", event => {
  const data = event.data ? event.data.json() : {};
  event.waitUntil(self.registration.showNotification("S.V.E.R staff alert", {
    body: "Check urgent removal requests in the staff console.",
    tag: `sver-staff-${String(data.id || "urgent")}`,
    renotify: false,
    data: { url: "/admin/take-it-down" },
    icon: "/icons/icon-192.png",
  }));
});
self.addEventListener("notificationclick", event => {
  event.notification.close();
  event.waitUntil(clients.openWindow("/admin/take-it-down"));
});
