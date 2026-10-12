-- The browser (digest of its ID) that reported a lease's last media time. One account watching on
-- two devices shares a lease, and their timelines differ, so media time is only compared within
-- the same browser.
ALTER TABLE playback_leases ADD COLUMN media_browser text;
