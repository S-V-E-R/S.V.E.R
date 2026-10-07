-- Module 3 viewer integrity: the lease remembers whether its network is a hosting or VPN
-- provider (IPinfo Lite), so the background rescore keeps the signal without storing any address.
ALTER TABLE playback_leases ADD COLUMN hosting boolean NOT NULL DEFAULT false;
