-- Optional S.V.E.R logo style (docs/PROFILES.md "Logo style"): which badge the site shows you.
-- Not a gender field; everyone starts on the plain text logo.
ALTER TABLE profiles
    ADD COLUMN logo_style text NOT NULL DEFAULT 'default' CHECK (logo_style IN ('default', 'male', 'female'));
