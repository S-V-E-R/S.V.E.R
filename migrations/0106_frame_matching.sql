-- Take It Down frame matching (docs/TAKE_IT_DOWN.md): 64-bit difference hashes of sampled video
-- frames and of removed media. A near match files a staff report ('removal_match') and hides the
-- content until staff decide.
CREATE TABLE frame_sources (
    source text PRIMARY KEY,
    indexed_at timestamptz NOT NULL DEFAULT now(),
    -- Start of the newest live segment that decoded a frame; the next is due frame_every_ms later.
    decoded_ms bigint
);
CREATE TABLE frame_hashes (
    source text NOT NULL REFERENCES frame_sources(source) ON DELETE CASCADE,
    hash bigint NOT NULL,
    PRIMARY KEY (source, hash)
);
CREATE TABLE removed_frames (
    source text NOT NULL,
    hash bigint NOT NULL,
    scanned boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (source, hash)
);
CREATE INDEX removed_frames_unscanned ON removed_frames(source, hash) WHERE NOT scanned;
-- Hashes of a held image's originals, taken when it is quarantined (NULL until then, and for
-- holds quarantined before this migration until they are indexed from the saved originals);
-- they become removed frames only if the request is upheld.
ALTER TABLE media_removal_holds ADD COLUMN frames bigint[];
ALTER TABLE video_jobs DROP CONSTRAINT IF EXISTS video_jobs_kind_check;
ALTER TABLE video_jobs ADD CONSTRAINT video_jobs_kind_check CHECK (kind IN ('SEGMENT','ASSEMBLE','THUMBNAIL','DOWNLOAD','DELETE','FRAMES'));
ALTER TABLE reports DROP CONSTRAINT reports_reason_check;
ALTER TABLE reports ADD CONSTRAINT reports_reason_check CHECK (reason IN ('spam', 'harassment', 'hate', 'sexual', 'violence', 'impersonation', 'private_information', 'copyright', 'mature_label', 'removal_match', 'other'));
