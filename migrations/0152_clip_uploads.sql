-- Clip uploads from S.V.E.R Desktop (docs/DEVELOPER_PLATFORM.md "Desktop clip upload"): a clip
-- waits as UPLOADING until its file is in storage, then an UPLOAD job checks and remuxes it. A clip
-- that fails the check keeps the reason in `failure`.
ALTER TABLE videos DROP CONSTRAINT IF EXISTS videos_status_check;
ALTER TABLE videos ADD CONSTRAINT videos_status_check CHECK (status IN ('UPLOADING','RECORDING','PROCESSING','READY','DELETING','EXPIRED','DELETED','FAILED'));
ALTER TABLE videos ADD COLUMN failure text;
ALTER TABLE video_jobs DROP CONSTRAINT IF EXISTS video_jobs_kind_check;
ALTER TABLE video_jobs ADD CONSTRAINT video_jobs_kind_check CHECK (kind IN ('SEGMENT','ASSEMBLE','THUMBNAIL','DOWNLOAD','DELETE','FRAMES','UPLOAD'));
