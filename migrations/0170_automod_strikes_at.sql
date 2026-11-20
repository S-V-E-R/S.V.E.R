-- AutoMod only reads the last hour of strikes; the jobs tick now prunes older ones by time.
CREATE INDEX automod_strikes_at ON automod_strikes (at);
