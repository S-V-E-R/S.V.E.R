-- Under-18 cap (docs/SUPPORT.md): a subscription's renewal is a monthly purchase too. The first
-- payment is already counted through its checkout; renewals are marked so the cap can count them.
ALTER TABLE sub_invoices ADD COLUMN renewal boolean NOT NULL DEFAULT false;
ALTER TABLE platform_invoices ADD COLUMN renewal boolean NOT NULL DEFAULT false;
